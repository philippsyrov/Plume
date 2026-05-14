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
use std::path::{Component, Path, PathBuf};

use serde::Serialize;

use crate::patch::apply::{apply_mutex, write_atomic, ApplyError, PatchFailureDetail};
use crate::patch::checkpoint::{
    read_checkpoint, read_checkpoint_image_safely, CheckpointReadError, ManifestEntry,
    MANIFEST_VERSION_CURRENT,
};
use crate::patch::parse::ChangeType;
use crate::patch::validate::PatchChangeType;
use crate::safety::path::ensure_inside;

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

// ─── Per-entry planning ──────────────────────────────────────────────────────

struct RevertPlan {
    change_type: ChangeType,
    /// For modify/create/delete: the touched path. For rename:
    /// the NEW (post-apply) path — i.e. where the file currently
    /// sits.
    target_rel: PathBuf,
    target_normalized: String,
    /// For rename: the OLD (pre-apply) path. The file will be
    /// at this path after revert succeeds.
    rename_from_rel: Option<PathBuf>,
    rename_from_normalized: Option<String>,
    /// Pre-image bytes read from `files/<...>`. `Some` for
    /// modify/delete/rename (rename's pre-image lives under the
    /// OLD path); `None` for create.
    pre_image: Option<Vec<u8>>,
}

impl RevertPlan {
    /// The path we surface on the wire as the "restored" path.
    /// For rename revert that's the old path (where the file
    /// ends up); for everything else it's the touched path
    /// itself.
    fn user_facing_path(&self) -> &str {
        match self.change_type {
            ChangeType::Rename => self
                .rename_from_normalized
                .as_deref()
                .unwrap_or(&self.target_normalized),
            _ => &self.target_normalized,
        }
    }
}

fn plan_revert_entry(
    project_root: &Path,
    checkpoint_dir: &Path,
    entry: &ManifestEntry,
) -> Result<RevertPlan, Vec<PatchFailureDetail>> {
    let change_type = parse_change_type(&entry.change_type).map_err(|msg| {
        vec![PatchFailureDetail {
            path: entry.path.clone(),
            hunk_index: None,
            message: msg,
        }]
    })?;

    // SECURITY: manifest.json lives at `.plume/checkpoints/<id>/`
    // — inside the project root, so the user (or any process the
    // user runs) can edit it between apply and revert. Treat the
    // entry paths as untrusted strings: re-run the same lexical
    // and ancestor-canonicalize safety the validator applies to
    // diff-side paths. Without this guard, a tampered
    // `entry.path: "../outside.txt"` would let revert write
    // outside the project root.
    let target_rel = validate_manifest_path(project_root, &entry.path, "manifest path")?;

    let (rename_from_rel, rename_from_normalized) = match change_type {
        ChangeType::Rename => match &entry.renamed_from {
            Some(from) => {
                let from_rel = validate_manifest_path(project_root, from, "manifest renamed_from")?;
                (Some(from_rel), Some(from.clone()))
            }
            None => {
                return Err(vec![PatchFailureDetail {
                    path: entry.path.clone(),
                    hunk_index: None,
                    message: "rename entry missing renamed_from in manifest".to_string(),
                }]);
            }
        },
        _ => (None, None),
    };

    let target_abs = project_root.join(&target_rel);

    // Drift check: compare current disk state against the
    // expected post-apply state we stored at apply time. Pass the
    // already-validated `target_rel` so the image lookup under
    // `post/` can't be smuggled outside the checkpoint subtree by
    // a tampered manifest path.
    drift_check(
        checkpoint_dir,
        &entry.path,
        &target_rel,
        &target_abs,
        change_type,
    )?;

    // Rename-specific drift: the old path must not exist. After a
    // successful apply, apply moved the file from old → new. If
    // the user re-created old (e.g., they wrote a new file there
    // thinking it was deleted), the `fs::rename(new, old)` revert
    // would do does in POSIX — and silently destroy whatever they
    // created at old. Reject up front instead.
    if matches!(change_type, ChangeType::Rename) {
        let from_rel = rename_from_rel.as_deref().expect("rename has from");
        let from_abs = project_root.join(from_rel);
        if fs::symlink_metadata(&from_abs).is_ok() {
            return Err(vec![PatchFailureDetail {
                path: rename_from_normalized
                    .clone()
                    .unwrap_or_else(|| entry.path.clone()),
                hunk_index: None,
                message: format!(
                    "drift: rename source {} is present on disk again (apply moved it away); refusing to overwrite",
                    rename_from_normalized.as_deref().unwrap_or("")
                ),
            }]);
        }
    }

    // Load the pre-image bytes if there should be any for this
    // change type. Modify/delete: under the touched path. Rename:
    // under the OLD path. Create: no pre-image (the file didn't
    // exist before apply).
    let pre_image = match change_type {
        ChangeType::Modify | ChangeType::Delete => {
            Some(load_pre_image(checkpoint_dir, &target_rel, &entry.path)?)
        }
        ChangeType::Rename => {
            let from_rel = rename_from_rel.as_deref().expect("rename has from");
            let from_norm = rename_from_normalized
                .as_deref()
                .expect("rename has normalized from");
            Some(load_pre_image(checkpoint_dir, from_rel, from_norm)?)
        }
        ChangeType::Create => None,
    };

    Ok(RevertPlan {
        change_type,
        target_rel,
        target_normalized: entry.path.clone(),
        rename_from_rel,
        rename_from_normalized,
        pre_image,
    })
}

/// D33 hardening: lexical + ancestor-canonicalize check on a path
/// pulled out of a checkpoint manifest. The manifest is on-disk
/// inside the project root and therefore user-editable; this
/// helper applies the same defense the validator runs on diff-side
/// paths. Returns the normalized project-RELATIVE path on success
/// (caller joins with project_root as needed) or a single
/// PatchFailureDetail on rejection.
fn validate_manifest_path(
    project_root: &Path,
    raw: &str,
    label: &str,
) -> Result<PathBuf, Vec<PatchFailureDetail>> {
    let deny = |msg: String| -> Vec<PatchFailureDetail> {
        vec![PatchFailureDetail {
            path: raw.to_string(),
            hunk_index: None,
            message: msg,
        }]
    };

    if raw.is_empty() {
        return Err(deny(format!("{}: empty path", label)));
    }
    if raw.contains('\0') {
        return Err(deny(format!("{}: NUL byte in path", label)));
    }
    let p = Path::new(raw);
    if p.is_absolute() {
        return Err(deny(format!(
            "{}: absolute path not allowed: {}",
            label, raw
        )));
    }

    let mut normalised = PathBuf::new();
    for component in p.components() {
        match component {
            Component::ParentDir => {
                return Err(deny(format!(
                    "{}: path contains '..' component: {}",
                    label, raw
                )));
            }
            Component::Prefix(_) | Component::RootDir => {
                return Err(deny(format!(
                    "{}: absolute path not allowed: {}",
                    label, raw
                )));
            }
            Component::CurDir => {}
            Component::Normal(seg) => normalised.push(seg),
        }
    }
    if normalised.as_os_str().is_empty() {
        return Err(deny(format!(
            "{}: path resolves to nothing: {}",
            label, raw
        )));
    }

    // Walk up the joined path to the first existing ancestor and
    // ensure it stays inside the project. Mirrors the validator's
    // `ensure_inside_or_existing_ancestor` for diff paths. The
    // canonical project root always exists, so the walk
    // terminates.
    let joined = project_root.join(&normalised);
    let mut current: &Path = joined.as_path();
    loop {
        if fs::symlink_metadata(current).is_ok() {
            if ensure_inside(project_root, current).is_err() {
                return Err(deny(format!("{}: escapes project root: {}", label, raw)));
            }
            break;
        }
        match current.parent() {
            Some(parent) if parent != current => current = parent,
            _ => break,
        }
    }

    Ok(normalised)
}

/// Compare disk state at `target_abs` to the expected post-apply
/// state stored under `<checkpoint_dir>/post/<entry.path>` (or
/// "absence" for delete entries). On mismatch returns a per-file
/// detail; the caller accumulates and surfaces all at once.
///
/// `target_rel` is the project-relative path already validated by
/// `validate_manifest_path`. We use it (rather than the raw
/// manifest string) to look the image up under `post/` so a
/// tampered manifest with a `..`-laden path can't smuggle the read
/// outside the checkpoint subtree. `entry_path` is the raw string,
/// retained only for user-facing error messages.
fn drift_check(
    checkpoint_dir: &Path,
    entry_path: &str,
    target_rel: &Path,
    target_abs: &Path,
    change_type: ChangeType,
) -> Result<(), Vec<PatchFailureDetail>> {
    match change_type {
        ChangeType::Delete => {
            // Post-state is "file should not exist." Anything on
            // disk at the touched path is drift — either the user
            // re-created the file or some unrelated process did.
            if fs::symlink_metadata(target_abs).is_ok() {
                return Err(vec![PatchFailureDetail {
                    path: entry_path.to_string(),
                    hunk_index: None,
                    message: format!(
                        "drift: {} should not exist (apply deleted it), but is present on disk",
                        entry_path
                    ),
                }]);
            }
            Ok(())
        }
        ChangeType::Modify | ChangeType::Create | ChangeType::Rename => {
            // For all three the post-state is a file at the
            // touched path with specific bytes. D33's
            // `create_checkpoint` always writes a `post/` entry
            // (even for pure rename, where post bytes == pre
            // bytes), so a missing post-image here is a checkpoint
            // corruption case, not a legitimate "pure rename"
            // fallback. Treat NotFound as drift — the safer
            // failure mode than silently accepting any file at
            // the path.
            let expected = match read_checkpoint_image_safely(checkpoint_dir, "post", target_rel) {
                Ok(bytes) => bytes,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    return Err(vec![PatchFailureDetail {
                        path: entry_path.to_string(),
                        hunk_index: None,
                        message: format!(
                            "checkpoint missing post-image for {}; cannot drift-detect",
                            entry_path
                        ),
                    }]);
                }
                Err(e) => {
                    return Err(vec![PatchFailureDetail {
                        path: entry_path.to_string(),
                        hunk_index: None,
                        message: format!("read checkpoint post-image: {}", e),
                    }]);
                }
            };
            let actual = match fs::read(target_abs) {
                Ok(bytes) => bytes,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    return Err(vec![PatchFailureDetail {
                        path: entry_path.to_string(),
                        hunk_index: None,
                        message: format!(
                            "drift: {} is missing on disk (apply left it present)",
                            entry_path
                        ),
                    }]);
                }
                Err(e) => {
                    return Err(vec![PatchFailureDetail {
                        path: entry_path.to_string(),
                        hunk_index: None,
                        message: format!("read current state: {}", e),
                    }]);
                }
            };
            if expected != actual {
                return Err(vec![PatchFailureDetail {
                    path: entry_path.to_string(),
                    hunk_index: None,
                    message: format!(
                        "drift: {} content differs from the post-apply state ({} bytes expected, {} on disk)",
                        entry_path,
                        expected.len(),
                        actual.len()
                    ),
                }]);
            }
            Ok(())
        }
    }
}

fn load_pre_image(
    checkpoint_dir: &Path,
    rel: &Path,
    label: &str,
) -> Result<Vec<u8>, Vec<PatchFailureDetail>> {
    read_checkpoint_image_safely(checkpoint_dir, "files", rel).map_err(|e| {
        vec![PatchFailureDetail {
            path: label.to_string(),
            hunk_index: None,
            message: format!(
                "read checkpoint pre-image {}: {}",
                checkpoint_dir.join("files").join(rel).display(),
                e
            ),
        }]
    })
}

// `read_checkpoint_image_safely` lives in `checkpoint.rs` so both
// `revert` and `apply` rollback can share the symlink/hardlink
// defense. The `use` line at the top of this file brings it in.

fn parse_change_type(s: &str) -> Result<ChangeType, String> {
    match s {
        "modify" => Ok(ChangeType::Modify),
        "create" => Ok(ChangeType::Create),
        "delete" => Ok(ChangeType::Delete),
        "rename" => Ok(ChangeType::Rename),
        other => Err(format!("unknown change_type {:?} in manifest", other)),
    }
}

fn change_type_to_wire(ct: ChangeType) -> PatchChangeType {
    match ct {
        ChangeType::Modify => PatchChangeType::Modify,
        ChangeType::Create => PatchChangeType::Create,
        ChangeType::Delete => PatchChangeType::Delete,
        ChangeType::Rename => PatchChangeType::Rename,
    }
}

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
