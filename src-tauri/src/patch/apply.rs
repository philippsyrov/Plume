//! D31: `patch.apply` — the first writing verb. Applies a
//! previously-validated unified diff inside the trusted project
//! root, with pre-image verification, filesystem-backed
//! checkpoint, and all-or-nothing semantics.
//!
//! See `docs/PATCH_APPLY_DESIGN.md` for the full design. Highlights:
//!
//!   * **D31 scope:** modify + create + delete. Rename is rejected
//!     with `reason: 'scopeUnsupported'` (validator still classifies
//!     it; apply refuses). Rename apply is reserved for a follow-up
//!     slice.
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
//!     is already a dependency.
//!   * **Revert:** verb + UI deferred to a follow-up slice.
//!     Checkpoint creation still lands so the revert slice just
//!     needs the inverse-apply path.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

use crate::patch::parse::{parse_diff, ChangeType, HunkLine, ParsedFile, ParsedHunk};
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
    /// state of every touched file. Reserved for a follow-up
    /// `patch.revert(checkpoint)` slice.
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
    /// Diff includes a change type the current slice doesn't
    /// implement. D31 supports modify / create / delete; rename
    /// is reserved for a follow-up slice.
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
    //    paths so the dedup pass below (and the plans further down)
    //    operate on the same canonical form the validator produced
    //    — `x.txt` and `./x.txt` collapse to a single key here.
    let normalized_paths: Vec<String> = match validate_patch(project_root, diff) {
        PatchValidateResponse::Ok(ok) => ok.touches.into_iter().map(|t| t.path).collect(),
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
    if parsed.len() != normalized_paths.len() {
        return err(
            PatchApplyFailure::ValidationFailed,
            vec![PatchFailureDetail {
                path: String::new(),
                hunk_index: None,
                message: format!(
                    "internal: parser and validator disagreed on file count ({} vs {})",
                    parsed.len(),
                    normalized_paths.len()
                ),
            }],
        );
    }

    // 3. Reject rename early. The validator classifies but apply
    //    refuses; the frontend's pill renders the `scopeUnsupported`
    //    reason.
    for (file, normalized) in parsed.iter().zip(normalized_paths.iter()) {
        if file.change_type == ChangeType::Rename {
            return err(
                PatchApplyFailure::ScopeUnsupported,
                vec![PatchFailureDetail {
                    path: normalized.clone(),
                    hunk_index: None,
                    message: "rename apply is reserved for a follow-up slice; this slice supports modify, create, delete"
                        .to_string(),
                }],
            );
        }
    }

    // 3a. Reject duplicate normalized target paths. Two file groups
    //     for the same path describe an ambiguous operation: both
    //     plans read the pre-image at planning time (before any
    //     write), so the second write would silently shadow the
    //     first's post-image. Dedup against the validator's
    //     normalized paths so `./x.txt` and `x.txt` collapse — the
    //     filesystem treats them as the same file, and so should
    //     our duplicate check.
    let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
    for normalized in &normalized_paths {
        if !seen.insert(normalized.as_str()) {
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
    }

    // 4. Build a per-file plan: pre-image verification + post-image
    //    computation. Atomic reject on any mismatch — `details`
    //    accumulates every failure, no file writes. Pass the
    //    validator's normalized path so the plan's identity (used
    //    on the wire AND for the rel_path join) matches what
    //    `patch.validate` reported.
    let mut plans: Vec<ApplyPlan> = Vec::new();
    let mut mismatch_details: Vec<PatchFailureDetail> = Vec::new();
    for (file, normalized) in parsed.iter().zip(normalized_paths.iter()) {
        match plan_file(project_root, file, normalized) {
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

fn apply_mutex() -> &'static Mutex<()> {
    static MUTEX: std::sync::OnceLock<Mutex<()>> = std::sync::OnceLock::new();
    MUTEX.get_or_init(|| Mutex::new(()))
}

// ─── Per-file planning ───────────────────────────────────────────────────────

struct ApplyPlan {
    /// Project-relative, forward-slash. Matches `PatchTouch.path`
    /// produced by the validator — same canonical form, no
    /// duplicated normalization logic.
    path: String,
    change_type: ChangeType,
    /// Project-relative `PathBuf` (joins against `project_root`).
    rel_path: PathBuf,
    /// Pre-image bytes from disk. `Some` for modify and delete;
    /// `None` for create.
    pre_image_bytes: Option<Vec<u8>>,
    /// Post-image bytes to write. `Some` for modify and create;
    /// `None` for delete (the file disappears).
    post_image_bytes: Option<Vec<u8>>,
    /// Directories that DID NOT exist at plan time but will be
    /// created during execute (via `create_dir_all` on the target's
    /// parent). Populated only for `Create`-typed plans. Rollback
    /// uses this list — and ONLY this list — to prune the parent
    /// chain, so a pre-existing empty directory the user kept
    /// around survives an aborted apply. Order is deepest-first so
    /// `remove_dir`'s empty-only semantics fall out naturally:
    /// remove the deepest empty dir first, which makes its parent
    /// empty, which can then be removed, and so on up the chain.
    created_dirs: Vec<PathBuf>,
}

fn plan_file(
    project_root: &Path,
    file: &ParsedFile,
    normalized_path: &str,
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
            // Record which ancestor dirs this apply will need to
            // create. Walk up from the target's parent; stop at the
            // first existing ancestor (or at project_root). The
            // recorded list bounds what rollback is allowed to
            // delete — a pre-existing empty directory is never
            // touched.
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
            let post_str = create_from_hunks(&file.hunks, normalized_path)?;
            Ok(ApplyPlan {
                path: normalized_path.to_string(),
                change_type: file.change_type,
                rel_path,
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
                pre_image_bytes: Some(pre_bytes),
                post_image_bytes: None,
                created_dirs: Vec::new(),
            })
        }
        ChangeType::Rename => unreachable!("rename rejected before plan_file"),
    }
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

// ─── Hunk application ────────────────────────────────────────────────────────

/// Walk the pre-image, splicing each hunk's `+`/` ` lines in
/// place of its `-`/` ` lines. Returns the post-image text on
/// success; any context or delete-line mismatch fails with a
/// `preImageMismatch` detail naming the hunk.
///
/// Trailing-newline handling: D31 preserves the pre-image's
/// trailing-newline state. The `\ No newline at end of file`
/// marker is dropped at parse time, so a diff that explicitly
/// flips the state will produce subtly wrong output — see
/// `docs/PATCH_APPLY_DESIGN.md § Open questions § Final-line-newline`.
fn apply_hunks_to(
    pre: &str,
    hunks: &[ParsedHunk],
    file_path: &str,
) -> Result<String, Vec<PatchFailureDetail>> {
    let had_trailing_newline = pre.ends_with('\n');
    let pre_lines: Vec<&str> = if pre.is_empty() {
        Vec::new()
    } else {
        let mut lines: Vec<&str> = pre.split('\n').collect();
        if had_trailing_newline {
            // `split('\n')` on "a\n" gives ["a", ""] — pop the
            // empty trailing so `pre_lines.len()` is the line
            // count.
            lines.pop();
        }
        lines
    };

    let mut out: Vec<String> = Vec::with_capacity(pre_lines.len());
    let mut pre_idx: usize = 0;

    for (hunk_idx, hunk) in hunks.iter().enumerate() {
        let hunk_start_0 = hunk.old_start.saturating_sub(1) as usize;
        if hunk_start_0 < pre_idx {
            return Err(vec![PatchFailureDetail {
                path: file_path.to_string(),
                hunk_index: Some((hunk_idx + 1) as u32),
                message: format!(
                    "hunk start line {} overlaps a previous hunk's range",
                    hunk.old_start
                ),
            }]);
        }
        while pre_idx < hunk_start_0 && pre_idx < pre_lines.len() {
            out.push(pre_lines[pre_idx].to_string());
            pre_idx += 1;
        }
        for hunk_line in &hunk.lines {
            match hunk_line {
                HunkLine::Context(text) => {
                    if pre_idx >= pre_lines.len() {
                        return Err(vec![PatchFailureDetail {
                            path: file_path.to_string(),
                            hunk_index: Some((hunk_idx + 1) as u32),
                            message: format!(
                                "hunk extends past end of file (expected context line {:?})",
                                text
                            ),
                        }]);
                    }
                    if pre_lines[pre_idx] != text {
                        return Err(vec![PatchFailureDetail {
                            path: file_path.to_string(),
                            hunk_index: Some((hunk_idx + 1) as u32),
                            message: format!(
                                "context mismatch at pre-image line {}: expected {:?}, got {:?}",
                                pre_idx + 1,
                                text,
                                pre_lines[pre_idx]
                            ),
                        }]);
                    }
                    out.push(text.clone());
                    pre_idx += 1;
                }
                HunkLine::Delete(text) => {
                    if pre_idx >= pre_lines.len() {
                        return Err(vec![PatchFailureDetail {
                            path: file_path.to_string(),
                            hunk_index: Some((hunk_idx + 1) as u32),
                            message: format!(
                                "hunk extends past end of file (expected to delete {:?})",
                                text
                            ),
                        }]);
                    }
                    if pre_lines[pre_idx] != text {
                        return Err(vec![PatchFailureDetail {
                            path: file_path.to_string(),
                            hunk_index: Some((hunk_idx + 1) as u32),
                            message: format!(
                                "delete-line mismatch at pre-image line {}: expected {:?}, got {:?}",
                                pre_idx + 1,
                                text,
                                pre_lines[pre_idx]
                            ),
                        }]);
                    }
                    pre_idx += 1;
                }
                HunkLine::Add(text) => {
                    out.push(text.clone());
                }
            }
        }
    }
    while pre_idx < pre_lines.len() {
        out.push(pre_lines[pre_idx].to_string());
        pre_idx += 1;
    }
    let mut result = out.join("\n");
    if had_trailing_newline && !result.is_empty() {
        result.push('\n');
    }
    Ok(result)
}

/// Build a newly-created file from a create-diff's hunks. The hunks
/// should contain only `+` and ` ` (treated as additions); a `-`
/// line inside a create-diff is malformed.
fn create_from_hunks(
    hunks: &[ParsedHunk],
    file_path: &str,
) -> Result<String, Vec<PatchFailureDetail>> {
    let mut out: Vec<String> = Vec::new();
    for (hunk_idx, hunk) in hunks.iter().enumerate() {
        for hunk_line in &hunk.lines {
            match hunk_line {
                HunkLine::Add(text) | HunkLine::Context(text) => out.push(text.clone()),
                HunkLine::Delete(_) => {
                    return Err(vec![PatchFailureDetail {
                        path: file_path.to_string(),
                        hunk_index: Some((hunk_idx + 1) as u32),
                        message: "create-diff contains a delete line".to_string(),
                    }]);
                }
            }
        }
    }
    let mut result = out.join("\n");
    if !result.is_empty() {
        result.push('\n');
    }
    Ok(result)
}

// ─── Plan execution + atomic write ───────────────────────────────────────────

struct ApplyError(String);
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
        ChangeType::Rename => unreachable!("rename rejected earlier"),
    }
}

/// Sibling-tempfile + rename. Same-directory rename is atomic on
/// POSIX (`renameat` per-directory semantics). The tempfile name
/// uses nanoseconds for uniqueness and a leading `.` so it's
/// hidden on macOS/Linux file managers.
fn write_atomic(abs_path: &Path, bytes: &[u8]) -> Result<(), ApplyError> {
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

// ─── Checkpoint ──────────────────────────────────────────────────────────────

struct Checkpoint {
    id: String,
    dir: PathBuf,
}

#[derive(Serialize, Deserialize)]
struct Manifest {
    id: String,
    entries: Vec<ManifestEntry>,
}

#[derive(Serialize, Deserialize)]
struct ManifestEntry {
    path: String,
    change_type: String,
}

fn create_checkpoint(project_root: &Path, plans: &[ApplyPlan]) -> Result<Checkpoint, ApplyError> {
    let id = checkpoint_id();

    // Hostile-environment guard: if `.plume/` or `.plume/checkpoints/`
    // are pre-planted symlinks, `fs::create_dir_all` would follow
    // them and write checkpoint files outside the project root.
    // Reject the symlink before any create. Then belt-and-braces:
    // canonicalize the final checkpoints root and assert it stays
    // inside the project tree — catches any subtler escape the
    // explicit check missed.
    let plume_dir = project_root.join(".plume");
    ensure_not_symlink(&plume_dir, ".plume")?;
    fs::create_dir_all(&plume_dir).map_err(|e| ApplyError(format!("create .plume/: {}", e)))?;

    let checkpoints_root = plume_dir.join("checkpoints");
    ensure_not_symlink(&checkpoints_root, ".plume/checkpoints")?;
    fs::create_dir_all(&checkpoints_root)
        .map_err(|e| ApplyError(format!("create .plume/checkpoints/: {}", e)))?;

    // Canonicalize + starts_with check. `project_root` is already
    // canonical at the command boundary (the trust gate calls
    // `canonicalize_root`), so a starts_with against the live
    // canonical checkpoints path catches anything the symlink
    // check above missed.
    let canon_root = fs::canonicalize(project_root)
        .map_err(|e| ApplyError(format!("canonicalize project root: {}", e)))?;
    let canon_checkpoints = fs::canonicalize(&checkpoints_root).map_err(|e| {
        ApplyError(format!(
            "canonicalize {}: {}",
            checkpoints_root.display(),
            e
        ))
    })?;
    if !canon_checkpoints.starts_with(&canon_root) {
        return Err(ApplyError(format!(
            ".plume/checkpoints/ canonicalized to {} (outside project root {})",
            canon_checkpoints.display(),
            canon_root.display()
        )));
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // Best-effort lockdown — open question #4 in the design doc.
        // Failure here doesn't block the apply; we just log via the
        // `_ =` discard.
        let _ = fs::set_permissions(&checkpoints_root, fs::Permissions::from_mode(0o700));
    }

    let dir = checkpoints_root.join(&id);
    fs::create_dir(&dir).map_err(|e| ApplyError(format!("create checkpoint dir: {}", e)))?;
    let files_dir = dir.join("files");
    fs::create_dir(&files_dir).map_err(|e| ApplyError(format!("create files dir: {}", e)))?;

    let mut manifest_entries: Vec<ManifestEntry> = Vec::new();
    for plan in plans {
        let entry = ManifestEntry {
            path: plan.path.clone(),
            change_type: change_type_string(plan.change_type),
        };
        if let Some(pre) = &plan.pre_image_bytes {
            let dest = files_dir.join(&plan.rel_path);
            if let Some(parent) = dest.parent() {
                fs::create_dir_all(parent)
                    .map_err(|e| ApplyError(format!("checkpoint parent dir: {}", e)))?;
            }
            fs::write(&dest, pre)
                .map_err(|e| ApplyError(format!("checkpoint write {}: {}", dest.display(), e)))?;
        }
        manifest_entries.push(entry);
    }

    let manifest = Manifest {
        id: id.clone(),
        entries: manifest_entries,
    };
    let manifest_path = dir.join("manifest.json");
    let manifest_json = serde_json::to_string_pretty(&manifest)
        .map_err(|e| ApplyError(format!("serialize manifest: {}", e)))?;
    fs::write(&manifest_path, manifest_json)
        .map_err(|e| ApplyError(format!("write manifest: {}", e)))?;

    Ok(Checkpoint { id, dir })
}

/// Reject any pre-existing path that's a symlink — `fs::create_dir_all`
/// would follow it and write checkpoint files outside the project
/// root. Missing (NotFound) is fine; we'll create the path as a
/// regular directory.
fn ensure_not_symlink(path: &Path, label: &str) -> Result<(), ApplyError> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => Err(ApplyError(format!(
            "{label} is a symlink; refusing to write checkpoint through it"
        ))),
        Ok(_) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(ApplyError(format!("stat {label}: {e}"))),
    }
}

/// Sortable id with enough randomness to avoid collisions between
/// applies happening within the same nanosecond on the same
/// machine. The design says ULID-shaped; this is close enough —
/// 32 lowercase-hex chars, time-sortable.
fn checkpoint_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let pid = std::process::id() as u128;
    // Pack: 96 bits of timestamp (more than enough for nanos
    // through ~2200), 32 bits of pid for uniqueness across
    // concurrent processes. Time goes in the high half so a
    // string sort is also a time sort.
    let combined = (nanos << 32) | (pid & 0xFFFFFFFF);
    format!("{:032x}", combined)
}

fn rollback_apply(
    project_root: &Path,
    checkpoint: &Checkpoint,
    applied_plans: &[ApplyPlan],
) -> Result<(), ApplyError> {
    // Roll back in reverse to undo creates before potentially
    // recreating their parent's pre-image. Practically: the order
    // doesn't matter because each plan operates on its own path,
    // but reverse-order keeps the trace easier to read in an OS log.
    for plan in applied_plans.iter().rev() {
        let abs_path = project_root.join(&plan.rel_path);
        match plan.change_type {
            ChangeType::Modify => {
                let saved = checkpoint.dir.join("files").join(&plan.rel_path);
                let saved_bytes = fs::read(&saved)
                    .map_err(|e| ApplyError(format!("read saved {}: {}", saved.display(), e)))?;
                write_atomic(&abs_path, &saved_bytes)?;
            }
            ChangeType::Create => {
                let _ = fs::remove_file(&abs_path);
                // Only prune the directories THIS apply created.
                // `plan.created_dirs` was recorded at plan time —
                // before any execute — by walking up from the
                // target's parent and stopping at the first
                // pre-existing ancestor. A pre-existing empty
                // directory the user kept around is therefore
                // NEVER in this list and survives rollback.
                //
                // `remove_dir` only succeeds on empty directories,
                // so it's also a belt-and-braces guard against the
                // race where another plan in the same apply (or an
                // external process) dropped a file under the dir
                // between plan and rollback time.
                for dir in &plan.created_dirs {
                    let _ = fs::remove_dir(dir);
                }
            }
            ChangeType::Delete => {
                let saved = checkpoint.dir.join("files").join(&plan.rel_path);
                let saved_bytes = fs::read(&saved)
                    .map_err(|e| ApplyError(format!("read saved {}: {}", saved.display(), e)))?;
                if let Some(parent) = abs_path.parent() {
                    fs::create_dir_all(parent)
                        .map_err(|e| ApplyError(format!("recreate parent: {}", e)))?;
                }
                fs::write(&abs_path, saved_bytes)
                    .map_err(|e| ApplyError(format!("restore delete: {}", e)))?;
            }
            ChangeType::Rename => unreachable!(),
        }
    }
    Ok(())
}

/// Best-effort: prune checkpoints older than 30 days, keep the
/// most recent 20. The id is sortable, so a name-descending sort
/// approximates a time-descending sort. Failures are swallowed —
/// a cleanup hiccup must not affect the apply we just did.
fn gc_checkpoints(project_root: &Path) -> Result<(), ApplyError> {
    let root = project_root.join(".plume").join("checkpoints");
    let Ok(entries) = fs::read_dir(&root) else {
        return Ok(());
    };
    let mut entries_vec: Vec<_> = entries.filter_map(|e| e.ok()).collect();
    entries_vec.sort_by_key(|e| std::cmp::Reverse(e.file_name()));
    let now = SystemTime::now();
    let max_age = std::time::Duration::from_secs(30 * 24 * 60 * 60);
    for (idx, entry) in entries_vec.iter().enumerate() {
        let too_old = entry
            .metadata()
            .ok()
            .and_then(|m| m.modified().ok())
            .map(|t| now.duration_since(t).map(|d| d > max_age).unwrap_or(false))
            .unwrap_or(false);
        if idx >= 20 || too_old {
            let _ = fs::remove_dir_all(entry.path());
        }
    }
    Ok(())
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn change_type_string(ct: ChangeType) -> String {
    match ct {
        ChangeType::Modify => "modify",
        ChangeType::Create => "create",
        ChangeType::Delete => "delete",
        ChangeType::Rename => "rename",
    }
    .to_string()
}

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
