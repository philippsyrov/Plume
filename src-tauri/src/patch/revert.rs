//! D33: `patch.revert` — the inverse of `patch.apply`.
//!
//! Reads a previously-created checkpoint, drift-detects against
//! the diff's post-apply state, then applies the inverse of every
//! manifest entry. All-or-nothing across the whole checkpoint:
//! any drift → reject without writing; any mid-revert write
//! failure → rollback via an in-memory snapshot captured before
//! the first revert write.
//!
//! Scope decisions (see brief + `docs/PATCH_APPLY_DESIGN.md`):
//!
//!   * **Supported change types:** modify, create, delete, rename.
//!     A D33-vintage checkpoint always has the data revert needs
//!     for all four (pre-image under `files/`, post-image under
//!     `post/`).
//!   * **D31 checkpoints reject.** A D31-vintage checkpoint has no
//!     `post/` tree and no `version` field in the manifest. We
//!     can't drift-detect without a post-image signature, so
//!     revert rejects with `unsupportedCheckpoint`. The pre-image
//!     copies under `.plume/checkpoints/<id>/files/` are still
//!     there for manual recovery if the user really needs to roll
//!     a D31 apply back.
//!   * **Drift is binary.** If any single touched file's current
//!     content disagrees with the expected post-apply state, the
//!     whole revert rejects with `drift` and per-file `details`.
//!     There is no force / discard-local-edits flag yet — the
//!     design defers it to a slice that adds the necessary
//!     approval prompt.
//!   * **Idempotency.** Not idempotent. Reverting twice with the
//!     same id reverts once, then the second call rejects with
//!     `drift` (because disk no longer matches the expected
//!     post-apply state) or `unknownCheckpoint` (if GC removed the
//!     directory).
//!   * **Redo checkpoint.** The design says to take a fresh
//!     checkpoint of the post-apply state before revert writes so
//!     the user can later "redo" by reverting that new id. For
//!     this slice we capture the pre-revert bytes IN MEMORY and
//!     use them only for crash-free rollback on mid-revert write
//!     failure. A durable redo checkpoint is a follow-up: it adds
//!     filesystem churn for a feature (redo button) that doesn't
//!     ship in D33.
//!
//! Concurrency: takes the same process-wide `apply_mutex()` as
//! `apply_patch`, so a revert can't interleave with an in-flight
//! apply on the same project root.

use std::fs;
use std::path::Path;

use serde::Serialize;

use crate::patch::apply::{apply_mutex, write_atomic, ApplyError, PatchFailureDetail};
use crate::patch::checkpoint::{read_checkpoint, CheckpointReadError, MANIFEST_VERSION_CURRENT};
use crate::patch::parse::ChangeType;
use crate::patch::revert_planning::{change_type_to_wire, plan_revert_entry, RevertPlan};
use crate::patch::validate::PatchChangeType;

// ─── On-wire types ───────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum PatchRevertResponse {
    Ok(PatchRevertOk),
    Err(PatchRevertErr),
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PatchRevertOk {
    /// Always `true`. Discriminator the TS layer matches on.
    pub reverted: bool,
    pub restored: Vec<PatchRestoredFile>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PatchRestoredFile {
    /// Project-relative path of the file the revert touched. For a
    /// rename revert this is the OLD path (where the file is after
    /// revert), matching the user's mental model of "we undid the
    /// rename."
    pub path: String,
    pub change_type: PatchChangeType,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PatchRevertErr {
    /// Always `false`. Discriminator the TS layer matches on.
    pub reverted: bool,
    pub reason: PatchRevertFailure,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub details: Vec<PatchFailureDetail>,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum PatchRevertFailure {
    /// Checkpoint id not in the store, or the manifest is missing.
    /// Also raised for malformed ids (path-escape attempts, empty
    /// strings, etc.).
    UnknownCheckpoint,
    /// At least one touched file's current content does not match
    /// the expected post-apply state. The user has edited since
    /// apply; a silent revert would destroy their work.
    Drift,
    /// A mid-revert write failed. The in-memory snapshot rollback
    /// attempted to put things back; if rollback ALSO failed the
    /// message carries both errors.
    WriteFailed,
    /// Checkpoint exists but predates `patch.revert` support (no
    /// `post/` tree, no manifest version). Revert can't drift-
    /// detect without an expected-post-apply signature, so it
    /// refuses rather than risk destroying user edits.
    UnsupportedCheckpoint,
}

// ─── Public entry point ──────────────────────────────────────────────────────

/// Revert a previously-applied patch by checkpoint id. See module
/// docs and `docs/PATCH_APPLY_DESIGN.md § Revert flow`.
///
/// `project_root` must be the trust-gated, canonicalized project
/// root — the caller (the `patch_revert` command handler) is
/// responsible for confirming trust before invoking this.
pub fn revert_patch(project_root: &Path, checkpoint_id: &str) -> PatchRevertResponse {
    let _guard = apply_mutex().lock().unwrap_or_else(|e| e.into_inner());

    // 1. Read the manifest (typed error → wire reason).
    let (manifest, checkpoint_dir) = match read_checkpoint(project_root, checkpoint_id) {
        Ok(pair) => pair,
        Err(CheckpointReadError::Unknown(msg)) => {
            return err(
                PatchRevertFailure::UnknownCheckpoint,
                vec![PatchFailureDetail {
                    path: String::new(),
                    hunk_index: None,
                    message: msg,
                }],
            );
        }
        Err(CheckpointReadError::Io(msg)) => {
            return err(
                PatchRevertFailure::UnknownCheckpoint,
                vec![PatchFailureDetail {
                    path: String::new(),
                    hunk_index: None,
                    message: msg,
                }],
            );
        }
    };

    // 2. Version gate. D31 checkpoints (version 0 after `default`)
    //    lack the post-image data we need for drift detection.
    if manifest.version < MANIFEST_VERSION_CURRENT {
        return err(
            PatchRevertFailure::UnsupportedCheckpoint,
            vec![PatchFailureDetail {
                path: String::new(),
                hunk_index: None,
                message: format!(
                    "checkpoint version {} predates revert support (current {}); the pre-image is still under .plume/checkpoints/<id>/files/ for manual recovery",
                    manifest.version, MANIFEST_VERSION_CURRENT
                ),
            }],
        );
    }

    // 3. Build a per-entry plan. Each plan captures the inverse
    //    operation AND the expected post-apply bytes for drift
    //    detection. Plans are NOT executed yet — we want atomic
    //    rejection across the whole revert.
    let mut plans: Vec<RevertPlan> = Vec::with_capacity(manifest.entries.len());
    let mut drift_details: Vec<PatchFailureDetail> = Vec::new();
    for entry in &manifest.entries {
        match plan_revert_entry(project_root, &checkpoint_dir, entry) {
            Ok(plan) => plans.push(plan),
            Err(mut errs) => drift_details.append(&mut errs),
        }
    }
    if !drift_details.is_empty() {
        return err(PatchRevertFailure::Drift, drift_details);
    }

    // 4. Capture pre-revert state into memory for rollback. For
    //    each plan we snapshot whatever file content / existence
    //    state we'd need to restore if a later plan fails. This
    //    is the design's "fresh checkpoint of the post-apply
    //    state" requirement, just kept in memory rather than on
    //    disk for this slice. Memory cost is bounded by the total
    //    size of files touched by the apply we're undoing — which
    //    is what `patch.apply` itself just read into memory, so
    //    same envelope.
    let snapshots: Vec<RevertSnapshot> = plans
        .iter()
        .map(|p| snapshot_pre_revert(project_root, p))
        .collect();

    // 5. Apply each revert plan sequentially. On first failure,
    //    roll back via the in-memory snapshots and surface
    //    `writeFailed`.
    let mut restored: Vec<PatchRestoredFile> = Vec::new();
    for (idx, plan) in plans.iter().enumerate() {
        match execute_revert(project_root, plan) {
            Ok(restored_path) => restored.push(PatchRestoredFile {
                path: restored_path,
                change_type: change_type_to_wire(plan.change_type),
            }),
            Err(e) => {
                let rb = rollback_revert(project_root, &plans[..idx], &snapshots[..idx]).err();
                let msg = match rb {
                    Some(rb_err) => format!("{} (rollback also failed: {})", e.0, rb_err.0),
                    None => e.0,
                };
                return err(
                    PatchRevertFailure::WriteFailed,
                    vec![PatchFailureDetail {
                        path: plan.user_facing_path().to_string(),
                        hunk_index: None,
                        message: msg,
                    }],
                );
            }
        }
    }

    PatchRevertResponse::Ok(PatchRevertOk {
        reverted: true,
        restored,
    })
}

fn err(reason: PatchRevertFailure, details: Vec<PatchFailureDetail>) -> PatchRevertResponse {
    PatchRevertResponse::Err(PatchRevertErr {
        reverted: false,
        reason,
        details,
    })
}

// ─── Per-entry planning ─────────────────────────────────────────────────────
//
// D35 moved planning (`RevertPlan`, `plan_revert_entry`,
// `validate_manifest_path`, `drift_check`, `load_pre_image`,
// `change_type_to_wire`) into the sibling `revert_planning`
// module so revert.rs stays under the decomposition cap. The
// `use` line at the top brings them back into scope.

// ─── Snapshot + execute + rollback ───────────────────────────────────────────

/// Pre-revert disk state captured per plan. Lives in memory only;
/// dropped after a successful revert. Rollback uses the matching
/// `applied` slice of these to undo a partial revert.
struct RevertSnapshot {
    /// Some(bytes) iff a file existed at the relevant pre-revert
    /// path. The "relevant" path is the apply's touched path for
    /// modify/delete/create; for rename it's the apply's NEW path
    /// (where the file lived after apply, which revert is about
    /// to move/edit).
    target_bytes: Option<Vec<u8>>,
    /// For rename: bytes at the OLD path BEFORE revert ran. Should
    /// almost always be None at this point (apply moved the file
    /// away from there), but capture it anyway so a hostile race
    /// where someone re-created the old path between apply and
    /// revert can be unwound.
    rename_from_bytes: Option<Vec<u8>>,
}

fn snapshot_pre_revert(project_root: &Path, plan: &RevertPlan) -> RevertSnapshot {
    let target_abs = project_root.join(&plan.target_rel);
    let target_bytes = fs::read(&target_abs).ok();
    let rename_from_bytes = plan.rename_from_rel.as_ref().and_then(|rel| {
        let abs = project_root.join(rel);
        fs::read(&abs).ok()
    });
    RevertSnapshot {
        target_bytes,
        rename_from_bytes,
    }
}

fn execute_revert(project_root: &Path, plan: &RevertPlan) -> Result<String, ApplyError> {
    let target_abs = project_root.join(&plan.target_rel);
    match plan.change_type {
        ChangeType::Modify => {
            let pre = plan
                .pre_image
                .as_deref()
                .ok_or_else(|| ApplyError("modify revert missing pre-image".to_string()))?;
            write_atomic(&target_abs, pre)?;
            Ok(plan.target_normalized.clone())
        }
        ChangeType::Create => {
            // Apply created the file; revert removes it.
            fs::remove_file(&target_abs)
                .map_err(|e| ApplyError(format!("remove {}: {}", target_abs.display(), e)))?;
            Ok(plan.target_normalized.clone())
        }
        ChangeType::Delete => {
            // Apply removed the file; revert restores from the
            // saved pre-image. Recreate any pruned parent dirs.
            let pre = plan
                .pre_image
                .as_deref()
                .ok_or_else(|| ApplyError("delete revert missing pre-image".to_string()))?;
            if let Some(parent) = target_abs.parent() {
                fs::create_dir_all(parent)
                    .map_err(|e| ApplyError(format!("recreate parent: {}", e)))?;
            }
            write_atomic(&target_abs, pre)?;
            Ok(plan.target_normalized.clone())
        }
        ChangeType::Rename => {
            // Apply moved old → new (and maybe edited). Revert
            // moves new → old, then if there was a body change,
            // restores the saved pre-image at the old path.
            let from_rel = plan
                .rename_from_rel
                .as_ref()
                .ok_or_else(|| ApplyError("rename revert missing source path".to_string()))?;
            let from_abs = project_root.join(from_rel);
            // Planning already drift-rejected if from_abs existed,
            // but a process outside Plume could have re-created it
            // in the microseconds between plan and execute. POSIX
            // `fs::rename` would silently overwrite. Re-check just
            // before the rename; on hit, abort the entire revert
            // via ApplyError (the outer rollback will then restore
            // any prior plans via the in-memory snapshot).
            if fs::symlink_metadata(&from_abs).is_ok() {
                return Err(ApplyError(format!(
                    "rename revert refused: source path {} re-appeared on disk between plan and execute",
                    from_abs.display()
                )));
            }
            if let Some(parent) = from_abs.parent() {
                fs::create_dir_all(parent)
                    .map_err(|e| ApplyError(format!("recreate from parent: {}", e)))?;
            }
            fs::rename(&target_abs, &from_abs).map_err(|e| {
                ApplyError(format!(
                    "rename revert {} -> {}: {}",
                    target_abs.display(),
                    from_abs.display(),
                    e
                ))
            })?;
            // If there was a body change at apply time, restore
            // the pre-image. `apply_patch` stored the body's
            // pre-image at `files/<from>` and the post-image at
            // `post/<to>`; the rename above already put the
            // file's bytes at the old path, but those bytes are
            // the (edited) post-image, not the pre-image. We
            // need the pre-image. Distinguish by comparing the
            // saved pre-image to the file's current bytes —
            // they'll differ for rename-with-edits.
            if let Some(pre) = plan.pre_image.as_deref() {
                let after_rename = fs::read(&from_abs)
                    .map_err(|e| ApplyError(format!("read after rename: {}", e)))?;
                if after_rename != pre {
                    write_atomic(&from_abs, pre)?;
                }
            }
            Ok(plan
                .rename_from_normalized
                .clone()
                .unwrap_or_else(|| plan.target_normalized.clone()))
        }
    }
}

/// Undo a partial revert. `applied[i]` is the slice of plans
/// whose `execute_revert` succeeded; `snapshots[i]` is the disk
/// state captured BEFORE those reverts ran. Reverse-iterate so
/// we undo most-recent first.
fn rollback_revert(
    project_root: &Path,
    applied: &[RevertPlan],
    snapshots: &[RevertSnapshot],
) -> Result<(), ApplyError> {
    if applied.len() != snapshots.len() {
        return Err(ApplyError(format!(
            "rollback snapshot/plan length mismatch ({} vs {})",
            snapshots.len(),
            applied.len()
        )));
    }
    for (plan, snap) in applied.iter().zip(snapshots.iter()).rev() {
        let target_abs = project_root.join(&plan.target_rel);
        match plan.change_type {
            ChangeType::Modify | ChangeType::Create | ChangeType::Delete => {
                match &snap.target_bytes {
                    Some(bytes) => {
                        if let Some(parent) = target_abs.parent() {
                            fs::create_dir_all(parent).map_err(|e| {
                                ApplyError(format!("rollback recreate parent: {}", e))
                            })?;
                        }
                        write_atomic(&target_abs, bytes)?;
                    }
                    None => {
                        // File didn't exist pre-revert. Undo our
                        // restore by removing whatever's there.
                        let _ = fs::remove_file(&target_abs);
                    }
                }
            }
            ChangeType::Rename => {
                let from_rel = plan
                    .rename_from_rel
                    .as_ref()
                    .ok_or_else(|| ApplyError("rollback rename missing from".to_string()))?;
                let from_abs = project_root.join(from_rel);
                // Step 1: remove whatever revert produced at the
                // old path (best-effort).
                let _ = fs::remove_file(&from_abs);
                // Step 2: rename back from old → new IF the new
                // path is now empty AND we'd captured pre-revert
                // bytes at the new path.
                if let Some(bytes) = &snap.target_bytes {
                    if let Some(parent) = target_abs.parent() {
                        fs::create_dir_all(parent)
                            .map_err(|e| ApplyError(format!("rollback recreate parent: {}", e)))?;
                    }
                    write_atomic(&target_abs, bytes)?;
                }
                // Step 3: restore old-path content if a snapshot
                // showed something there (hostile race coverage).
                if let Some(bytes) = &snap.rename_from_bytes {
                    if let Some(parent) = from_abs.parent() {
                        fs::create_dir_all(parent).map_err(|e| {
                            ApplyError(format!("rollback recreate from parent: {}", e))
                        })?;
                    }
                    write_atomic(&from_abs, bytes)?;
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "revert_tests.rs"]
mod tests;
