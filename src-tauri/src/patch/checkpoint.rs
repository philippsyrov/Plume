//! D33 split: filesystem-checkpoint primitives.
//!
//! Pre-D33 lived inside `apply.rs` since only `apply_patch`
//! needed them. D33's `revert_patch` needed the read side AND
//! apply.rs crossed the 1200-line red guardrail, so this module
//! is the home for the manifest type, the on-disk layout helpers,
//! and the read/write/GC entry points.
//!
//! Disk layout, mirroring `docs/PATCH_APPLY_DESIGN.md § Checkpoint
//! storage`:
//!
//! ```text
//! <project>/.plume/checkpoints/<checkpointId>/
//!   manifest.json          one entry per touched path
//!   files/                 pre-image copies, keyed by rel path
//!     src/foo.rs           (rename pre-images go under the OLD path)
//!   post/                  post-image copies, keyed by rel path (D33)
//!     src/foo.rs           (rename post-images go under the NEW path;
//!                           pure rename has no post entry)
//! ```
//!
//! Manifest schema is versioned: D31 wrote no `version` field
//! (deserializes as 0), D33 stamps `version: 2`. The bump is what
//! gates revert against a checkpoint that lacks the `post/` tree.

use std::fs;
use std::path::{Component, Path, PathBuf};
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

use crate::patch::apply::{ApplyError, ApplyPlan};
use crate::patch::parse::ChangeType;

/// In-process handle for a freshly-created checkpoint. `dir`
/// points at `.plume/checkpoints/<id>/`; rollback paths read
/// `files/<rel-path>` under it.
pub(crate) struct Checkpoint {
    pub(crate) id: String,
    pub(crate) dir: PathBuf,
}

/// D33: manifest format version. Bumped from implicit-1 (D31, no
/// `version` field on disk) to 2 once `post/` storage landed. The
/// version gates `patch.revert`: a D31-vintage checkpoint (no
/// `post/`) cannot be reverted because we have no signature of
/// the post-apply state to drift-detect against, so revert
/// rejects with `unsupportedCheckpoint`. New apply calls always
/// stamp the current version.
pub(crate) const MANIFEST_VERSION_CURRENT: u32 = 2;

#[derive(Serialize, Deserialize)]
pub(crate) struct Manifest {
    pub(crate) id: String,
    /// D33: missing on D31-vintage checkpoints. `#[serde(default)]`
    /// gives those `version == 0`; the revert path uses
    /// `version >= MANIFEST_VERSION_CURRENT` as its compatibility
    /// gate.
    #[serde(default)]
    pub(crate) version: u32,
    pub(crate) entries: Vec<ManifestEntry>,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct ManifestEntry {
    pub(crate) path: String,
    pub(crate) change_type: String,
    /// D33: present only on rename entries. Source path in the
    /// validator's normalized form.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) renamed_from: Option<String>,
}

/// D33: typed error from `read_checkpoint`. `Unknown` maps to
/// `PatchRevertFailure::UnknownCheckpoint`; `Io` is anything else
/// (permission denied, hostile symlink, manifest parse failure)
/// and surfaces under `UnknownCheckpoint` too on the wire — the
/// distinction matters only for log readability inside the
/// revert path.
pub(crate) enum CheckpointReadError {
    Unknown(String),
    Io(String),
}

pub(crate) fn create_checkpoint(
    project_root: &Path,
    plans: &[ApplyPlan],
) -> Result<Checkpoint, ApplyError> {
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
    // D33: post-image bytes go under `post/`, mirroring the
    // project-relative path. Revert reads this back as the
    // expected-post-apply state for drift detection.
    let post_dir = dir.join("post");
    fs::create_dir(&post_dir).map_err(|e| ApplyError(format!("create post dir: {}", e)))?;

    let mut manifest_entries: Vec<ManifestEntry> = Vec::new();
    for plan in plans {
        let entry = ManifestEntry {
            path: plan.path.clone(),
            change_type: change_type_string(plan.change_type),
            renamed_from: plan.rename_from_normalized.clone(),
        };
        // Pre-image storage. Modify/delete store under the
        // touched path; rename stores under the OLD path (the
        // file's identity at apply time).
        if let Some(pre) = &plan.pre_image_bytes {
            let pre_rel = match plan.change_type {
                ChangeType::Rename => plan
                    .rename_from_rel_path
                    .as_deref()
                    .unwrap_or(plan.rel_path.as_path()),
                _ => plan.rel_path.as_path(),
            };
            let dest = files_dir.join(pre_rel);
            if let Some(parent) = dest.parent() {
                fs::create_dir_all(parent)
                    .map_err(|e| ApplyError(format!("checkpoint parent dir: {}", e)))?;
            }
            fs::write(&dest, pre)
                .map_err(|e| ApplyError(format!("checkpoint write {}: {}", dest.display(), e)))?;
        }
        // D33: post-image storage. Modify / create / rename ALL
        // write to `post/<path>` so revert always has bytes to
        // drift-check against. For a pure rename (no body change)
        // the post-image equals the pre-image; we still copy them
        // under `post/` so the revert path doesn't have to know
        // whether the rename carried edits. Delete is the only
        // change type with `post_image_bytes == None` — absence
        // IS the post-state, and revert checks that the file is
        // missing on disk.
        let post_bytes_for_storage: Option<&[u8]> = match plan.change_type {
            ChangeType::Rename if plan.post_image_bytes.is_none() => {
                // Pure rename: post bytes equal pre-image bytes.
                plan.pre_image_bytes.as_deref()
            }
            _ => plan.post_image_bytes.as_deref(),
        };
        if let Some(post) = post_bytes_for_storage {
            let dest = post_dir.join(&plan.rel_path);
            if let Some(parent) = dest.parent() {
                fs::create_dir_all(parent)
                    .map_err(|e| ApplyError(format!("checkpoint post parent: {}", e)))?;
            }
            fs::write(&dest, post).map_err(|e| {
                ApplyError(format!("checkpoint post write {}: {}", dest.display(), e))
            })?;
        }
        manifest_entries.push(entry);
    }

    let manifest = Manifest {
        id: id.clone(),
        version: MANIFEST_VERSION_CURRENT,
        entries: manifest_entries,
    };
    let manifest_path = dir.join("manifest.json");
    let manifest_json = serde_json::to_string_pretty(&manifest)
        .map_err(|e| ApplyError(format!("serialize manifest: {}", e)))?;
    fs::write(&manifest_path, manifest_json)
        .map_err(|e| ApplyError(format!("write manifest: {}", e)))?;

    Ok(Checkpoint { id, dir })
}

/// D33: shared helper used by `revert.rs` to read a checkpoint
/// directory back. Kept here next to the writer so the disk
/// layout has a single owner. The path-safety belt-and-braces
/// (ensure_not_symlink + canonicalize) mirror `create_checkpoint`'s
/// guards so a hostile `.plume/` symlink can't redirect revert's
/// reads either.
pub(crate) fn read_checkpoint(
    project_root: &Path,
    checkpoint_id: &str,
) -> Result<(Manifest, PathBuf), CheckpointReadError> {
    // Path-safety on the id itself. The id is opaque to the user
    // but the revert command takes it from a payload, so treat it
    // like any other untrusted string. Reject anything that could
    // escape the checkpoints directory.
    if checkpoint_id.is_empty()
        || checkpoint_id.contains('/')
        || checkpoint_id.contains('\\')
        || checkpoint_id.contains("..")
        || checkpoint_id.contains('\0')
    {
        return Err(CheckpointReadError::Unknown(format!(
            "invalid checkpoint id format: {:?}",
            checkpoint_id
        )));
    }

    let plume_dir = project_root.join(".plume");
    if let Err(e) = ensure_not_symlink(&plume_dir, ".plume") {
        return Err(CheckpointReadError::Io(e.0));
    }
    let checkpoints_root = plume_dir.join("checkpoints");
    if let Err(e) = ensure_not_symlink(&checkpoints_root, ".plume/checkpoints") {
        return Err(CheckpointReadError::Io(e.0));
    }

    let dir = checkpoints_root.join(checkpoint_id);
    if !dir.exists() {
        return Err(CheckpointReadError::Unknown(format!(
            "checkpoint {} not found",
            checkpoint_id
        )));
    }
    if let Err(e) = ensure_not_symlink(&dir, "checkpoint directory") {
        return Err(CheckpointReadError::Io(e.0));
    }

    // Canonicalize the checkpoint dir and assert it stays inside
    // the project. Same belt-and-braces as `create_checkpoint`.
    let canon_root = fs::canonicalize(project_root)
        .map_err(|e| CheckpointReadError::Io(format!("canonicalize project root: {}", e)))?;
    let canon_dir = fs::canonicalize(&dir)
        .map_err(|e| CheckpointReadError::Io(format!("canonicalize {}: {}", dir.display(), e)))?;
    if !canon_dir.starts_with(&canon_root) {
        return Err(CheckpointReadError::Io(format!(
            "checkpoint {} canonicalized outside project root",
            checkpoint_id
        )));
    }

    let manifest_path = dir.join("manifest.json");
    let raw = fs::read_to_string(&manifest_path).map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => CheckpointReadError::Unknown(format!(
            "manifest.json missing for checkpoint {}",
            checkpoint_id
        )),
        _ => CheckpointReadError::Io(format!("read manifest: {}", e)),
    })?;
    let manifest: Manifest = serde_json::from_str(&raw)
        .map_err(|e| CheckpointReadError::Io(format!("parse manifest: {}", e)))?;
    Ok((manifest, dir))
}

/// Read bytes from `<checkpoint_dir>/<kind>/<rel>` while rejecting
/// any symlinked or hardlinked component in the path. The
/// checkpoint subtree (`files/` and `post/`) is editable by anyone
/// with write access to the project root — same trust posture as
/// the manifest. `read_checkpoint`'s symlink defense covers the
/// top-level `.plume/checkpoints/<id>/` directory only; this
/// helper extends the defense to every component of the image
/// path so a tampered `files/leak.txt → /etc/passwd` symlink (or
/// a hardlink alias) can't smuggle outside bytes into a delete
/// revert or apply rollback.
///
/// Returns the same `io::Error` shape as `fs::read` so callers can
/// preserve their existing `NotFound` handling. Symlink / hardlink
/// / non-Normal-component rejections surface as
/// `ErrorKind::PermissionDenied`.
pub(crate) fn read_checkpoint_image_safely(
    checkpoint_dir: &Path,
    kind: &str,
    rel: &Path,
) -> std::io::Result<Vec<u8>> {
    use std::io;

    // The subtree root (`files/` or `post/`) must itself not be a
    // symlink. A missing subtree surfaces as NotFound — same shape
    // as the old `fs::read` did — so callers' NotFound branches
    // keep working.
    let mut current = checkpoint_dir.join(kind);
    let meta = fs::symlink_metadata(&current)?;
    if meta.file_type().is_symlink() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("refusing checkpoint subtree {}: symlink", current.display()),
        ));
    }
    // Walk every component of `rel` and reject any symlink along
    // the way. `rel` is supposed to contain only `Normal`
    // components (validated upstream by `validate_manifest_path`
    // for revert / by a typed PathBuf for rollback), but match
    // defensively: any `..` or absolute prefix here would still
    // escape via canonicalize-through-symlink, so refuse outright.
    for comp in rel.components() {
        let seg = match comp {
            Component::Normal(s) => s,
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!(
                        "non-Normal component in checkpoint image rel path: {:?}",
                        rel
                    ),
                ));
            }
        };
        current = current.join(seg);
        let meta = fs::symlink_metadata(&current)?;
        if meta.file_type().is_symlink() {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!(
                    "refusing checkpoint image path {}: symlink",
                    current.display()
                ),
            ));
        }
    }
    // Leaf-level hardlink-alias check (Unix). `create_checkpoint`
    // writes fresh files via `fs::write`, so the legit value is
    // always 1; nlink > 1 means something planted a hardlink to
    // coerce a read from outside the subtree. Coarse policy
    // matches `safety::path::ensure_no_hardlink_alias` for
    // prompt/file reads.
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let meta = fs::symlink_metadata(&current)?;
        if meta.file_type().is_file() && meta.nlink() > 1 {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!(
                    "refusing checkpoint image {}: file has {} hardlinks (expected 1)",
                    current.display(),
                    meta.nlink()
                ),
            ));
        }
    }
    fs::read(&current)
}

/// Reject any pre-existing path that's a symlink — `fs::create_dir_all`
/// would follow it and write checkpoint files outside the project
/// root. Missing (NotFound) is fine; we'll create the path as a
/// regular directory.
pub(crate) fn ensure_not_symlink(path: &Path, label: &str) -> Result<(), ApplyError> {
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

/// Best-effort: prune checkpoints older than 30 days, keep the
/// most recent 20. The id is sortable, so a name-descending sort
/// approximates a time-descending sort. Failures are swallowed —
/// a cleanup hiccup must not affect the apply we just did.
pub(crate) fn gc_checkpoints(project_root: &Path) -> Result<(), ApplyError> {
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

fn change_type_string(ct: ChangeType) -> String {
    match ct {
        ChangeType::Modify => "modify",
        ChangeType::Create => "create",
        ChangeType::Delete => "delete",
        ChangeType::Rename => "rename",
    }
    .to_string()
}
