//! D35 split: rollback path for `patch.apply`.
//!
//! `rollback_apply` undoes a partially-completed apply by walking
//! the already-executed plans in reverse and restoring each one's
//! pre-execute state from the checkpoint that was taken before the
//! first write. It runs only on a mid-apply write failure; a
//! pre-image mismatch rejects before any plan executes, and a
//! successful apply never enters this path.
//!
//! D33 moved the manifest types + on-disk read/write/GC helpers
//! into `checkpoint.rs`; D35 moves the rollback path here so
//! `apply.rs` stays under the decomposition cap. No behavior
//! change.
//!
//! The rename branch reads the pre-image via
//! `read_checkpoint_image_safely` (symlink/hardlink-rejecting),
//! which is the same defense `revert.rs` uses. See the Codex
//! re-review note on D33 for why every checkpoint-image read goes
//! through that helper.

use std::fs;
use std::path::Path;

use crate::patch::apply::{write_atomic, ApplyError, ApplyPlan};
use crate::patch::checkpoint::{read_checkpoint_image_safely, Checkpoint};
use crate::patch::parse::ChangeType;

pub(crate) fn rollback_apply(
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
                let saved_bytes =
                    read_checkpoint_image_safely(&checkpoint.dir, "files", &plan.rel_path)
                        .map_err(|e| {
                            ApplyError(format!(
                                "read saved {}: {}",
                                checkpoint.dir.join("files").join(&plan.rel_path).display(),
                                e
                            ))
                        })?;
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
                let saved_bytes =
                    read_checkpoint_image_safely(&checkpoint.dir, "files", &plan.rel_path)
                        .map_err(|e| {
                            ApplyError(format!(
                                "read saved {}: {}",
                                checkpoint.dir.join("files").join(&plan.rel_path).display(),
                                e
                            ))
                        })?;
                if let Some(parent) = abs_path.parent() {
                    fs::create_dir_all(parent)
                        .map_err(|e| ApplyError(format!("recreate parent: {}", e)))?;
                }
                fs::write(&abs_path, saved_bytes)
                    .map_err(|e| ApplyError(format!("restore delete: {}", e)))?;
            }
            ChangeType::Rename => {
                // Inverse of execute: rename new path back to old
                // path, then if there was a body write, overwrite
                // the old path with the saved pre-image. The
                // tempfile from `write_atomic` doesn't survive a
                // successful execute, so the only state to undo is
                // the rename + the (possibly edited) destination
                // file.
                let from_rel = plan
                    .rename_from_rel_path
                    .as_ref()
                    .ok_or_else(|| ApplyError("rename rollback missing source path".to_string()))?;
                let from_abs = project_root.join(from_rel);
                // Rename back. If the destination doesn't exist
                // (execute didn't complete the rename), silently
                // proceed — we still want to clean up any partial
                // tempfile state via the pre-image restore below.
                if abs_path.exists() {
                    if let Some(parent) = from_abs.parent() {
                        let _ = fs::create_dir_all(parent);
                    }
                    fs::rename(&abs_path, &from_abs).map_err(|e| {
                        ApplyError(format!(
                            "rollback rename {} -> {}: {}",
                            abs_path.display(),
                            from_abs.display(),
                            e
                        ))
                    })?;
                }
                // For rename-with-edits, restore the pre-image to
                // the source path. For pure rename the bytes are
                // already correct (we just renamed the original
                // file back) and there's nothing to overwrite.
                if plan.post_image_bytes.is_some() {
                    let saved_bytes =
                        read_checkpoint_image_safely(&checkpoint.dir, "files", from_rel).map_err(
                            |e| {
                                ApplyError(format!(
                                    "read saved {}: {}",
                                    checkpoint.dir.join("files").join(from_rel).display(),
                                    e
                                ))
                            },
                        )?;
                    write_atomic(&from_abs, &saved_bytes)?;
                }
                // Prune any parent dirs THIS apply created on the
                // new-path side. Same created_dirs guarantee as
                // Create: pre-existing empty dirs are not in the
                // list and survive rollback.
                for dir in &plan.created_dirs {
                    let _ = fs::remove_dir(dir);
                }
            }
        }
    }
    Ok(())
}
