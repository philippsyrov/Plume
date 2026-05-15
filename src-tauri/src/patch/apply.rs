//! D31: `patch.apply` — the first writing verb. Applies a
//! previously-validated unified diff inside the trusted project
//! root, with pre-image verification, filesystem-backed
//! checkpoint, and all-or-nothing semantics.
//!
//! See `docs/PATCH_APPLY_DESIGN.md` for the full design. Highlights:
//!
//!   * **Scope:** modify + create + delete (D31) + rename (D33).
//!     The applier writes all four change types; the
//!     `scopeUnsupported` variant stays on the wire for future
//!     change shapes (e.g. binary patches) without churning the
//!     TS union.
//!   * **Re-validation:** the diff is validated server-side every
//!     time, even when the frontend already showed a green pill. The
//!     client cannot smuggle past path-safety by sending a different
//!     diff than what the validator saw.
//!   * **Atomicity:** all-or-nothing across the whole patch. If any
//!     pre-image hunk fails, nothing writes. If a mid-apply write
//!     fails, we roll back via the checkpoint that was taken before
//!     the first write.
//!   * **Checkpoint storage:** `.plume/checkpoints/<id>/` per the
//!     design. Manifest format is JSON (not TOML as the design doc
//!     loosely mentions) so we don't add a `toml` crate — `serde_json`
//!     is already a dependency. The manifest writer + reader and the
//!     GC live in the sibling `checkpoint` module (D33 split).
//!   * **Revert:** `patch.revert` ships in D33 via `revert.rs`. It
//!     consumes the checkpoint this module creates (manifest
//!     `version: 2` + `post/` subtree).

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;

use serde::Serialize;

use crate::patch::apply_hunks::{apply_hunks_to, create_from_hunks};
use crate::patch::apply_rollback::rollback_apply;
use crate::patch::checkpoint::{create_checkpoint, gc_checkpoints};
use crate::patch::parse::{parse_diff, ChangeType, ParsedFile};
use crate::patch::validate::{validate_patch, PatchChangeType, PatchValidateResponse};

// ─── On-wire types ───────────────────────────────────────────────────────────

/// On-wire response. Untagged so the JSON shape is either
/// `{"applied": true, "checkpoint": "...", "touched": [...]}` or
/// `{"applied": false, "reason": "...", "details": [...]}`. The TS
/// layer narrows on `resp.applied`.
#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum PatchApplyResponse {
    Ok(PatchApplyOk),
    Err(PatchApplyErr),
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PatchApplyOk {
    /// Always `true`. Discriminator the TS layer matches on.
    pub applied: bool,
    /// Opaque id of the checkpoint that captured the pre-apply
    /// state of every touched file. D33's `patch.revert` reads
    /// this back to undo the apply.
    pub checkpoint: String,
    pub touched: Vec<PatchAppliedFile>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PatchAppliedFile {
    pub path: String,
    pub change_type: PatchChangeType,
    /// Post-apply file size on disk. `0` for a delete.
    pub bytes_written: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PatchApplyErr {
    /// Always `false`. Discriminator the TS layer matches on.
    pub applied: bool,
    pub reason: PatchApplyFailure,
    /// Per-file (or per-hunk) detail. Skipped when empty so the
    /// JSON stays compact for reasons that don't carry per-file
    /// breakdown.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub details: Vec<PatchFailureDetail>,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum PatchApplyFailure {
    /// Re-validation rejected the diff. `details` mirrors the
    /// validator's `errors[]` so the frontend can show the same
    /// kind of pill it would for an `invalid` validate response.
    ValidationFailed,
    /// At least one hunk's pre-image did not match disk. No file
    /// was written. `details` names every file-and-hunk that
    /// disagreed.
    PreImageMismatch,
    /// Could not create the pre-apply checkpoint (disk full,
    /// permission denied, `.plume/` is a hostile symlink, etc.).
    /// No file was written.
    CheckpointFailed,
    /// A mid-apply write failed AFTER one or more files had
    /// already been written. The applier rolled back; `details`
    /// names the file that failed and the OS error.
    WriteFailed,
    /// Diff includes a change type or operation shape the applier
    /// doesn't support. D31 used this to reject `rename`; D33 lifts
    /// that — the applier now writes modify / create / delete /
    /// rename. The variant stays on the wire for forward-compat:
    /// a future binary-patch shape, an unrecognized validator
    /// change type, or a similar "valid diff in concept, but not
    /// in this slice" case can map here without churning the TS
    /// union.
    #[allow(dead_code)]
    ScopeUnsupported,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PatchFailureDetail {
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hunk_index: Option<u32>,
    pub message: String,
}

// ─── Public entry point ──────────────────────────────────────────────────────

/// Apply `diff` to `project_root`. All-or-nothing across the
/// patch. See module docs and `docs/PATCH_APPLY_DESIGN.md`.
///
/// `project_root` must be the trust-gated, canonicalized project
/// root — the caller (the `patch_apply` command handler) is
/// responsible for confirming trust before invoking this function.
pub fn apply_patch(project_root: &Path, diff: &str) -> PatchApplyResponse {
    // Serialize concurrent applies. The design's open question #6
    // notes that a per-project mutex is the cleanest answer; this
    // process-wide mutex is the simpler floor that gives the same
    // safety for the single-window app D31 actually ships into.
    let _guard = apply_mutex().lock().unwrap_or_else(|e| e.into_inner());

    // 1. Re-validate. The frontend's validation result is a UI
    //    hint, not a security artifact — we validate again here so
    //    a swapped renderer or a future bug can't send a diff the
    //    validator never saw. Capture the validator's normalized
    //    paths (and `renamed_from` paths for renames) so the dedup
    //    pass below and the planner operate on the same canonical
    //    form the validator produced — `x.txt` and `./x.txt`
    //    collapse to a single key here.
    let normalized_touches: Vec<(String, Option<String>)> = match validate_patch(project_root, diff)
    {
        PatchValidateResponse::Ok(ok) => ok
            .touches
            .into_iter()
            .map(|t| (t.path, t.renamed_from))
            .collect(),
        PatchValidateResponse::Err(e) => {
            return err(
                PatchApplyFailure::ValidationFailed,
                e.errors
                    .into_iter()
                    .map(|err| PatchFailureDetail {
                        path: err.path.unwrap_or_default(),
                        hunk_index: None,
                        message: err.message,
                    })
                    .collect(),
            );
        }
    };

    // 2. Re-parse so we have hunk bodies (the validator's `Ok`
    //    response carries only counts, not bodies). The parser is
    //    pure CPU work — fine to run twice for a small diff.
    let parsed = match parse_diff(diff) {
        Ok(p) => p,
        Err(_) => {
            return err(
                PatchApplyFailure::ValidationFailed,
                vec![PatchFailureDetail {
                    path: String::new(),
                    hunk_index: None,
                    message: "diff re-parse failed after validate succeeded".to_string(),
                }],
            );
        }
    };
    // Belt-and-braces: validate and parse iterate the same file
    // groups in the same order, so a successful validate produces
    // exactly one `touch` per parsed file. If those ever drift,
    // fail loudly rather than silently zip a mismatched pair.
    if parsed.len() != normalized_touches.len() {
        return err(
            PatchApplyFailure::ValidationFailed,
            vec![PatchFailureDetail {
                path: String::new(),
                hunk_index: None,
                message: format!(
                    "internal: parser and validator disagreed on file count ({} vs {})",
                    parsed.len(),
                    normalized_touches.len()
                ),
            }],
        );
    }

    // 3. Reject duplicate normalized target paths. Two file groups
    //    for the same path describe an ambiguous operation: both
    //    plans read the pre-image at planning time (before any
    //    write), so the second write would silently shadow the
    //    first's post-image. Dedup against the validator's
    //    normalized paths so `./x.txt` and `x.txt` collapse — the
    //    filesystem treats them as the same file, and so should
    //    our duplicate check. For renames we ALSO add the source
    //    path to the dedup set — a rename's source and a separate
    //    modify of the same path would both try to read the file
    //    at plan time and produce inconsistent results at write
    //    time.
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    for (normalized, renamed_from) in &normalized_touches {
        if !seen.insert(normalized.clone()) {
            return err(
                PatchApplyFailure::ValidationFailed,
                vec![PatchFailureDetail {
                    path: normalized.clone(),
                    hunk_index: None,
                    message: format!(
                        "duplicate target path '{}' in diff: one apply call cannot describe two operations on the same file",
                        normalized
                    ),
                }],
            );
        }
        if let Some(from) = renamed_from {
            if !seen.insert(from.clone()) {
                return err(
                    PatchApplyFailure::ValidationFailed,
                    vec![PatchFailureDetail {
                        path: from.clone(),
                        hunk_index: None,
                        message: format!(
                            "rename source '{}' conflicts with another touched path in the same diff",
                            from
                        ),
                    }],
                );
            }
        }
    }

    // 4. Build a per-file plan: pre-image verification + post-image
    //    computation. Atomic reject on any mismatch — `details`
    //    accumulates every failure, no file writes. Pass the
    //    validator's normalized path so the plan's identity (used
    //    on the wire AND for the rel_path join) matches what
    //    `patch.validate` reported.
    let mut plans: Vec<ApplyPlan> = Vec::new();
    let mut mismatch_details: Vec<PatchFailureDetail> = Vec::new();
    for (file, (normalized, renamed_from)) in parsed.iter().zip(normalized_touches.iter()) {
        match plan_file(project_root, file, normalized, renamed_from.as_deref()) {
            Ok(plan) => plans.push(plan),
            Err(mut errs) => mismatch_details.append(&mut errs),
        }
    }
    if !mismatch_details.is_empty() {
        return err(PatchApplyFailure::PreImageMismatch, mismatch_details);
    }

    // 5. Take the checkpoint BEFORE any write. If this fails,
    //    nothing's been touched.
    let checkpoint = match create_checkpoint(project_root, &plans) {
        Ok(c) => c,
        Err(e) => {
            return err(
                PatchApplyFailure::CheckpointFailed,
                vec![PatchFailureDetail {
                    path: String::new(),
                    hunk_index: None,
                    message: e.0,
                }],
            );
        }
    };

    // 6. Apply each plan sequentially. On first write failure,
    //    roll back everything applied so far and surface
    //    `writeFailed`.
    let mut touched: Vec<PatchAppliedFile> = Vec::new();
    for (idx, plan) in plans.iter().enumerate() {
        match execute_plan(project_root, plan) {
            Ok(bytes) => touched.push(PatchAppliedFile {
                path: plan.path.clone(),
                change_type: change_type_to_wire(plan.change_type),
                bytes_written: bytes,
            }),
            Err(e) => {
                let rollback_err = rollback_apply(project_root, &checkpoint, &plans[..idx]).err();
                let msg = match rollback_err {
                    Some(re) => format!("{} (rollback also failed: {})", e.0, re.0),
                    None => e.0,
                };
                return err(
                    PatchApplyFailure::WriteFailed,
                    vec![PatchFailureDetail {
                        path: plan.path.clone(),
                        hunk_index: None,
                        message: msg,
                    }],
                );
            }
        }
    }

    // 7. Opportunistic GC of older checkpoints. Best-effort; a
    //    failed prune does not affect the just-applied patch.
    let _ = gc_checkpoints(project_root);

    PatchApplyResponse::Ok(PatchApplyOk {
        applied: true,
        checkpoint: checkpoint.id,
        touched,
    })
}

fn err(reason: PatchApplyFailure, details: Vec<PatchFailureDetail>) -> PatchApplyResponse {
    PatchApplyResponse::Err(PatchApplyErr {
        applied: false,
        reason,
        details,
    })
}

/// Process-wide mutex used by both `apply_patch` and `revert_patch`
/// to serialize concurrent disk-mutating operations on the same
/// project. A per-project mutex is the eventual cleaner answer
/// (open question #6 in the design doc); this is the floor that
/// gives the same safety for Plume's single-window assumption.
pub(crate) fn apply_mutex() -> &'static Mutex<()> {
    static MUTEX: std::sync::OnceLock<Mutex<()>> = std::sync::OnceLock::new();
    MUTEX.get_or_init(|| Mutex::new(()))
}

// ─── Per-file planning ───────────────────────────────────────────────────────

pub(crate) struct ApplyPlan {
    /// Project-relative, forward-slash. Matches `PatchTouch.path`
    /// produced by the validator — same canonical form, no
    /// duplicated normalization logic. For renames this is the
    /// NEW (post-rename) path.
    pub(crate) path: String,
    pub(crate) change_type: ChangeType,
    /// Project-relative `PathBuf` (joins against `project_root`).
    /// For renames this is the NEW (post-rename) path.
    pub(crate) rel_path: PathBuf,
    /// D33: source path for renames in normalized string form;
    /// `None` for everything else. Mirrors `PatchTouch.renamed_from`.
    pub(crate) rename_from_normalized: Option<String>,
    /// D33: source path for renames as `PathBuf`; `None` for
    /// everything else.
    pub(crate) rename_from_rel_path: Option<PathBuf>,
    /// Pre-image bytes from disk. `Some` for modify, delete, and
    /// rename; `None` for create. For rename the bytes are read
    /// from the OLD path (`rename_from_rel_path`).
    pub(crate) pre_image_bytes: Option<Vec<u8>>,
    /// Post-image bytes to write. `Some` for modify, create, and
    /// rename-with-hunks; `None` for delete and for rename-only
    /// (no body change — the post-image equals the pre-image and
    /// the rename itself is the entire operation).
    pub(crate) post_image_bytes: Option<Vec<u8>>,
    /// Directories that DID NOT exist at plan time but will be
    /// created during execute (via `create_dir_all` on the target's
    /// parent). Populated for `Create` plans AND for `Rename` plans
    /// where the new path's parent chain doesn't exist yet.
    /// Rollback uses this list — and ONLY this list — to prune the
    /// parent chain, so a pre-existing empty directory the user
    /// kept around survives an aborted apply. Order is deepest-first
    /// so `remove_dir`'s empty-only semantics fall out naturally.
    pub(crate) created_dirs: Vec<PathBuf>,
}

fn plan_file(
    project_root: &Path,
    file: &ParsedFile,
    normalized_path: &str,
    normalized_renamed_from: Option<&str>,
) -> Result<ApplyPlan, Vec<PatchFailureDetail>> {
    let rel_path = PathBuf::from(normalized_path);
    let abs_path = project_root.join(&rel_path);

    match file.change_type {
        ChangeType::Modify => {
            let pre_bytes = fs::read(&abs_path).map_err(|e| {
                vec![PatchFailureDetail {
                    path: normalized_path.to_string(),
                    hunk_index: None,
                    message: format!("cannot read pre-image: {}", e),
                }]
            })?;
            let pre_str = bytes_to_utf8(&pre_bytes, normalized_path)?;
            let post_str = apply_hunks_to(&pre_str, &file.hunks, normalized_path)?;
            Ok(ApplyPlan {
                path: normalized_path.to_string(),
                change_type: file.change_type,
                rel_path,
                rename_from_normalized: None,
                rename_from_rel_path: None,
                pre_image_bytes: Some(pre_bytes),
                post_image_bytes: Some(post_str.into_bytes()),
                created_dirs: Vec::new(),
            })
        }
        ChangeType::Create => {
            // File must not exist at apply time. The validator's
            // path-safety already canonicalized the ancestor chain,
            // so a symlinked-out parent rejects upstream.
            if fs::symlink_metadata(&abs_path).is_ok() {
                return Err(vec![PatchFailureDetail {
                    path: normalized_path.to_string(),
                    hunk_index: None,
                    message: "create-diff target already exists on disk".to_string(),
                }]);
            }
            let created_dirs = plan_created_dirs(project_root, &abs_path);
            let post_str = create_from_hunks(&file.hunks, normalized_path)?;
            Ok(ApplyPlan {
                path: normalized_path.to_string(),
                change_type: file.change_type,
                rel_path,
                rename_from_normalized: None,
                rename_from_rel_path: None,
                pre_image_bytes: None,
                post_image_bytes: Some(post_str.into_bytes()),
                created_dirs,
            })
        }
        ChangeType::Delete => {
            let pre_bytes = fs::read(&abs_path).map_err(|e| {
                vec![PatchFailureDetail {
                    path: normalized_path.to_string(),
                    hunk_index: None,
                    message: format!("cannot read pre-image: {}", e),
                }]
            })?;
            let pre_str = bytes_to_utf8(&pre_bytes, normalized_path)?;
            // Verify the hunks describe a complete deletion: walking
            // the pre-image with the hunks should produce an empty
            // post-image. If it doesn't, the diff is a partial
            // deletion (not a full delete) — reject.
            let post_str = apply_hunks_to(&pre_str, &file.hunks, normalized_path)?;
            if !post_str.is_empty() {
                return Err(vec![PatchFailureDetail {
                    path: normalized_path.to_string(),
                    hunk_index: None,
                    message:
                        "delete-diff produced non-empty post-image; partial deletion not supported"
                            .to_string(),
                }]);
            }
            Ok(ApplyPlan {
                path: normalized_path.to_string(),
                change_type: file.change_type,
                rel_path,
                rename_from_normalized: None,
                rename_from_rel_path: None,
                pre_image_bytes: Some(pre_bytes),
                post_image_bytes: None,
                created_dirs: Vec::new(),
            })
        }
        ChangeType::Rename => {
            // The validator guarantees a rename diff carries a
            // normalized `renamed_from`; if it's somehow missing
            // here that's a parser/validator bug, not a runtime
            // condition the user can produce, so surface it as a
            // validation failure rather than panic.
            let rename_from = match normalized_renamed_from {
                Some(p) => p,
                None => {
                    return Err(vec![PatchFailureDetail {
                        path: normalized_path.to_string(),
                        hunk_index: None,
                        message: "rename diff missing source path".to_string(),
                    }]);
                }
            };
            let from_rel = PathBuf::from(rename_from);
            let from_abs = project_root.join(&from_rel);

            // Source must exist on disk. The validator's
            // path-safety already canonicalized both names, so a
            // symlinked-out source rejects upstream.
            let pre_bytes = fs::read(&from_abs).map_err(|e| {
                vec![PatchFailureDetail {
                    path: rename_from.to_string(),
                    hunk_index: None,
                    message: format!("cannot read rename source pre-image: {}", e),
                }]
            })?;

            // Destination must NOT exist. We refuse to silently
            // clobber an unrelated file the user might have created
            // in the meantime. This mirrors the create-diff guard.
            if fs::symlink_metadata(&abs_path).is_ok() {
                return Err(vec![PatchFailureDetail {
                    path: normalized_path.to_string(),
                    hunk_index: None,
                    message: "rename target already exists on disk; refusing to clobber"
                        .to_string(),
                }]);
            }

            // Compute the post-image. A rename diff MAY carry hunks
            // (rename-with-edits); a pure rename has none, in which
            // case the post-image equals the pre-image and we leave
            // `post_image_bytes = None` to signal "no body write
            // needed after the rename."
            let post_bytes: Option<Vec<u8>> = if file.hunks.is_empty() {
                None
            } else {
                let pre_str = bytes_to_utf8(&pre_bytes, rename_from)?;
                let post_str = apply_hunks_to(&pre_str, &file.hunks, normalized_path)?;
                Some(post_str.into_bytes())
            };

            // The new path may live in a directory chain that
            // doesn't exist yet (rename across directories). Track
            // those for rollback exactly like the Create branch.
            let created_dirs = plan_created_dirs(project_root, &abs_path);

            Ok(ApplyPlan {
                path: normalized_path.to_string(),
                change_type: file.change_type,
                rel_path,
                rename_from_normalized: Some(rename_from.to_string()),
                rename_from_rel_path: Some(from_rel),
                pre_image_bytes: Some(pre_bytes),
                post_image_bytes: post_bytes,
                created_dirs,
            })
        }
    }
}

/// Walk up from the target's parent recording non-existing ancestors
/// inside the project root, deepest-first. Used by Create AND Rename
/// plans so rollback knows exactly which directories THIS apply made,
/// and so a pre-existing empty dir the user kept around survives.
fn plan_created_dirs(project_root: &Path, abs_path: &Path) -> Vec<PathBuf> {
    let mut created_dirs: Vec<PathBuf> = Vec::new();
    let mut cur = abs_path.parent();
    while let Some(dir) = cur {
        if dir == project_root || !dir.starts_with(project_root) {
            break;
        }
        if dir.exists() {
            break;
        }
        created_dirs.push(dir.to_path_buf());
        cur = dir.parent();
    }
    created_dirs
}

fn bytes_to_utf8(bytes: &[u8], path: &str) -> Result<String, Vec<PatchFailureDetail>> {
    std::str::from_utf8(bytes)
        .map(|s| s.to_string())
        .map_err(|e| {
            vec![PatchFailureDetail {
                path: path.to_string(),
                hunk_index: None,
                message: format!("file is not valid UTF-8 ({}); patch.apply is text-only", e),
            }]
        })
}

// D35 moved `apply_hunks_to` and `create_from_hunks` to the sibling
// `apply_hunks` module so apply.rs stays under the decomposition cap.
// The `use` line at the top brings them back into scope.

// ─── Plan execution + atomic write ───────────────────────────────────────────

pub(crate) struct ApplyError(pub(crate) String);
impl std::fmt::Display for ApplyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

fn execute_plan(project_root: &Path, plan: &ApplyPlan) -> Result<u64, ApplyError> {
    let abs_path = project_root.join(&plan.rel_path);
    match plan.change_type {
        ChangeType::Modify => {
            let post = plan
                .post_image_bytes
                .as_deref()
                .ok_or_else(|| ApplyError("modify plan has no post-image".to_string()))?;
            write_atomic(&abs_path, post)?;
            Ok(post.len() as u64)
        }
        ChangeType::Create => {
            if let Some(parent) = abs_path.parent() {
                fs::create_dir_all(parent).map_err(|e| {
                    ApplyError(format!("create_dir_all {}: {}", parent.display(), e))
                })?;
            }
            let post = plan
                .post_image_bytes
                .as_deref()
                .ok_or_else(|| ApplyError("create plan has no post-image".to_string()))?;
            write_atomic(&abs_path, post)?;
            Ok(post.len() as u64)
        }
        ChangeType::Delete => {
            fs::remove_file(&abs_path)
                .map_err(|e| ApplyError(format!("remove_file {}: {}", abs_path.display(), e)))?;
            Ok(0)
        }
        ChangeType::Rename => {
            // D33: rename is `fs::rename(old, new)` plus an
            // optional body write for rename-with-edits. Same-FS
            // rename is atomic; cross-FS may not be (no rename24
            // on Linux pre-5.2, etc.), but the project tree is
            // overwhelmingly single-FS so `fs::rename` is the
            // right primitive. If the rename does cross a FS
            // boundary and fails with EXDEV, the user sees the
            // typed OS error in `writeFailed`.
            let from_rel = plan
                .rename_from_rel_path
                .as_ref()
                .ok_or_else(|| ApplyError("rename plan has no source path".to_string()))?;
            let from_abs = project_root.join(from_rel);
            if let Some(parent) = abs_path.parent() {
                fs::create_dir_all(parent).map_err(|e| {
                    ApplyError(format!("create_dir_all {}: {}", parent.display(), e))
                })?;
            }
            fs::rename(&from_abs, &abs_path).map_err(|e| {
                ApplyError(format!(
                    "rename {} -> {}: {}",
                    from_abs.display(),
                    abs_path.display(),
                    e
                ))
            })?;
            // Rename-with-edits: write the post-image body to the
            // new path via the same atomic sibling-tempfile pattern
            // as modify. Pure rename has `post_image_bytes == None`
            // and we report the unchanged pre-image size.
            //
            // SELF-ROLLBACK: if the body write fails AFTER the
            // rename succeeded, the outer rollback path receives
            // `plans[..idx]` — the slice of plans BEFORE this one
            // — and so wouldn't see the partial-rename state we
            // just left on disk. Undo the rename in-place before
            // returning Err. Best-effort: if the reverse rename
            // itself fails (extremely unlikely; same-dir, same FS
            // we just used), surface both errors. The outer
            // rollback will then handle prior plans only, which is
            // correct since our state is now consistent.
            match plan.post_image_bytes.as_deref() {
                Some(post) => match write_atomic(&abs_path, post) {
                    Ok(()) => Ok(post.len() as u64),
                    Err(write_err) => {
                        let undo = fs::rename(&abs_path, &from_abs);
                        let msg = match undo {
                            Ok(_) => {
                                // Reverse rename succeeded. Match
                                // the outer rollback's `Rename`
                                // branch and prune any dirs THIS
                                // plan created on the new-path side
                                // — the outer rollback receives
                                // `plans[..idx]` and so wouldn't
                                // otherwise see them. `remove_dir`
                                // only succeeds on empty dirs, same
                                // belt-and-braces as the outer path.
                                for dir in &plan.created_dirs {
                                    let _ = fs::remove_dir(dir);
                                }
                                write_err.0
                            }
                            Err(undo_err) => format!(
                                "{} (rename self-rollback also failed: rename {} -> {}: {})",
                                write_err.0,
                                abs_path.display(),
                                from_abs.display(),
                                undo_err
                            ),
                        };
                        Err(ApplyError(msg))
                    }
                },
                None => {
                    let bytes = plan
                        .pre_image_bytes
                        .as_deref()
                        .map(|b| b.len() as u64)
                        .unwrap_or(0);
                    Ok(bytes)
                }
            }
        }
    }
}

/// Sibling-tempfile + rename. Same-directory rename is atomic on
/// POSIX (`renameat` per-directory semantics). The tempfile name
/// uses nanoseconds for uniqueness and a leading `.` so it's
/// hidden on macOS/Linux file managers.
pub(crate) fn write_atomic(abs_path: &Path, bytes: &[u8]) -> Result<(), ApplyError> {
    let parent = abs_path
        .parent()
        .ok_or_else(|| ApplyError(format!("target {} has no parent", abs_path.display())))?;
    let file_name = abs_path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| ApplyError(format!("target {} has no filename", abs_path.display())))?;
    let nanos = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let tmp_path = parent.join(format!(".{}.plume-{}.tmp", file_name, nanos));
    {
        let mut f = fs::File::create(&tmp_path)
            .map_err(|e| ApplyError(format!("create temp {}: {}", tmp_path.display(), e)))?;
        if let Err(e) = f.write_all(bytes) {
            let _ = fs::remove_file(&tmp_path);
            return Err(ApplyError(format!(
                "write temp {}: {}",
                tmp_path.display(),
                e
            )));
        }
        if let Err(e) = f.sync_all() {
            let _ = fs::remove_file(&tmp_path);
            return Err(ApplyError(format!(
                "sync temp {}: {}",
                tmp_path.display(),
                e
            )));
        }
    }
    fs::rename(&tmp_path, abs_path).map_err(|e| {
        let _ = fs::remove_file(&tmp_path);
        ApplyError(format!("rename -> {}: {}", abs_path.display(), e))
    })?;
    Ok(())
}

// ─── Checkpoint + rollback ──────────────────────────────────────────────────
//
// D33 moved the manifest types + on-disk read/write/GC helpers
// into `checkpoint.rs` so both apply and revert can call them.
// D35 moved the apply-side `rollback_apply` into the sibling
// `apply_rollback` module for the same reason. Both are wired
// back in via the `use` lines at the top of this file.

fn change_type_to_wire(ct: ChangeType) -> PatchChangeType {
    match ct {
        ChangeType::Modify => PatchChangeType::Modify,
        ChangeType::Create => PatchChangeType::Create,
        ChangeType::Delete => PatchChangeType::Delete,
        ChangeType::Rename => PatchChangeType::Rename,
    }
}

#[cfg(test)]
#[path = "apply_tests.rs"]
mod tests;
