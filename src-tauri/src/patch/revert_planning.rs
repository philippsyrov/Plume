//! D35 split: per-entry planning extracted from `revert.rs`.
//!
//! Contains everything needed to turn a checkpoint manifest entry
//! into a `RevertPlan`:
//!
//! * `RevertPlan` itself — the in-memory description of one
//!   manifest entry's inverse operation, including the pre-image
//!   bytes that `execute_revert` will write back.
//! * `plan_revert_entry` — the entry point, called once per
//!   manifest entry by `revert_patch`.
//! * `validate_manifest_path` — lexical + ancestor-canonicalize
//!   safety check on user-editable manifest paths.
//! * `drift_check` — compares disk state to the expected
//!   post-apply state stored under `<checkpoint_dir>/post/`.
//! * `load_pre_image` — reads pre-image bytes from `files/`,
//!   guarded by the same symlink/hardlink defense as drift_check.
//! * `parse_change_type` + `change_type_to_wire` — string ↔ enum
//!   converters for the manifest's `change_type` field.
//!
//! Lives in its own file because `revert.rs` crossed the
//! decomposition cap; this is D35's amber-cleanup companion to
//! the D33 checkpoint extraction. No behavior change.

use std::fs;
use std::path::{Component, Path, PathBuf};

use crate::patch::apply::PatchFailureDetail;
use crate::patch::checkpoint::{read_checkpoint_image_safely, ManifestEntry};
use crate::patch::parse::ChangeType;
use crate::patch::validate::PatchChangeType;
use crate::safety::path::ensure_inside;

pub(crate) struct RevertPlan {
    pub(crate) change_type: ChangeType,
    /// For modify/create/delete: the touched path. For rename:
    /// the NEW (post-apply) path — i.e. where the file currently
    /// sits.
    pub(crate) target_rel: PathBuf,
    pub(crate) target_normalized: String,
    /// For rename: the OLD (pre-apply) path. The file will be
    /// at this path after revert succeeds.
    pub(crate) rename_from_rel: Option<PathBuf>,
    pub(crate) rename_from_normalized: Option<String>,
    /// Pre-image bytes read from `files/<...>`. `Some` for
    /// modify/delete/rename (rename's pre-image lives under the
    /// OLD path); `None` for create.
    pub(crate) pre_image: Option<Vec<u8>>,
}

impl RevertPlan {
    /// The path we surface on the wire as the "restored" path.
    /// For rename revert that's the old path (where the file
    /// ends up); for everything else it's the touched path
    /// itself.
    pub(crate) fn user_facing_path(&self) -> &str {
        match self.change_type {
            ChangeType::Rename => self
                .rename_from_normalized
                .as_deref()
                .unwrap_or(&self.target_normalized),
            _ => &self.target_normalized,
        }
    }
}

pub(crate) fn plan_revert_entry(
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

fn parse_change_type(s: &str) -> Result<ChangeType, String> {
    match s {
        "modify" => Ok(ChangeType::Modify),
        "create" => Ok(ChangeType::Create),
        "delete" => Ok(ChangeType::Delete),
        "rename" => Ok(ChangeType::Rename),
        other => Err(format!("unknown change_type {:?} in manifest", other)),
    }
}

pub(crate) fn change_type_to_wire(ct: ChangeType) -> PatchChangeType {
    match ct {
        ChangeType::Modify => PatchChangeType::Modify,
        ChangeType::Create => PatchChangeType::Create,
        ChangeType::Delete => PatchChangeType::Delete,
        ChangeType::Rename => PatchChangeType::Rename,
    }
}
