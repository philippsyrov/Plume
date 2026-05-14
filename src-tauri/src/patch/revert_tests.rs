//! Tests for `patch::revert`. Split into a sibling file via
//! `#[path]` so the production module stays under the
//! decomposition cap. Same pattern as `apply_tests.rs`.

use super::*;
use crate::patch::apply::{apply_patch, PatchApplyResponse};
use std::fs;
use std::path::PathBuf;

// ─── Tempdir helper (mirrors apply_tests.rs) ────────────────────────────────

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(label: &str) -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock before epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "plume-revert-test-{}-{}-{}",
            label,
            std::process::id(),
            nanos
        ));
        fs::create_dir_all(&path).expect("create tempdir");
        Self { path }
    }
    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn canon_root(td: &TempDir) -> PathBuf {
    fs::canonicalize(td.path()).expect("canonicalize tempdir")
}

/// Apply a diff and extract the checkpoint id from a successful
/// `PatchApplyOk`. Panics if apply failed — tests should set up a
/// valid pre-image before calling.
fn apply_and_get_checkpoint(root: &Path, diff: &str) -> String {
    match apply_patch(root, diff) {
        PatchApplyResponse::Ok(ok) => ok.checkpoint,
        PatchApplyResponse::Err(e) => panic!("apply failed: {:?}", e),
    }
}

// ─── Happy paths: modify / create / delete ──────────────────────────────────

#[test]
fn revert_modify_restores_pre_image() {
    let td = TempDir::new("rev-mod");
    let root = canon_root(&td);
    fs::write(root.join("a.txt"), "a\nb\nc\n").unwrap();
    let diff = "--- a/a.txt\n\
        +++ b/a.txt\n\
        @@ -1,3 +1,3 @@\n\
         a\n\
        -b\n\
        +B\n\
         c\n";
    let id = apply_and_get_checkpoint(&root, diff);
    assert_eq!(fs::read_to_string(root.join("a.txt")).unwrap(), "a\nB\nc\n");

    let resp = revert_patch(&root, &id);
    match resp {
        PatchRevertResponse::Ok(ok) => {
            assert!(ok.reverted);
            assert_eq!(ok.restored.len(), 1);
            assert_eq!(ok.restored[0].path, "a.txt");
            assert_eq!(ok.restored[0].change_type, PatchChangeType::Modify);
        }
        PatchRevertResponse::Err(e) => panic!("expected ok, got {:?}", e),
    }
    assert_eq!(fs::read_to_string(root.join("a.txt")).unwrap(), "a\nb\nc\n");
}

#[test]
fn revert_create_removes_the_file() {
    let td = TempDir::new("rev-create");
    let root = canon_root(&td);
    let diff = "--- /dev/null\n\
        +++ b/new.txt\n\
        @@ -0,0 +1,2 @@\n\
        +one\n\
        +two\n";
    let id = apply_and_get_checkpoint(&root, diff);
    assert!(root.join("new.txt").exists());

    let resp = revert_patch(&root, &id);
    match resp {
        PatchRevertResponse::Ok(ok) => {
            assert_eq!(ok.restored.len(), 1);
            assert_eq!(ok.restored[0].path, "new.txt");
            assert_eq!(ok.restored[0].change_type, PatchChangeType::Create);
        }
        PatchRevertResponse::Err(e) => panic!("expected ok, got {:?}", e),
    }
    assert!(!root.join("new.txt").exists());
}

#[test]
fn revert_delete_restores_the_file() {
    let td = TempDir::new("rev-del");
    let root = canon_root(&td);
    fs::write(root.join("doomed.txt"), "line\n").unwrap();
    let diff = "--- a/doomed.txt\n\
        +++ /dev/null\n\
        @@ -1,1 +0,0 @@\n\
        -line\n";
    let id = apply_and_get_checkpoint(&root, diff);
    assert!(!root.join("doomed.txt").exists());

    let resp = revert_patch(&root, &id);
    match resp {
        PatchRevertResponse::Ok(ok) => {
            assert_eq!(ok.restored[0].path, "doomed.txt");
            assert_eq!(ok.restored[0].change_type, PatchChangeType::Delete);
        }
        PatchRevertResponse::Err(e) => panic!("expected ok, got {:?}", e),
    }
    assert_eq!(
        fs::read_to_string(root.join("doomed.txt")).unwrap(),
        "line\n"
    );
}

#[test]
fn revert_rename_with_edits_restores_old_path_and_pre_image() {
    let td = TempDir::new("rev-ren-edits");
    let root = canon_root(&td);
    fs::write(root.join("old.txt"), "a\nb\n").unwrap();
    let diff = "--- a/old.txt\n\
        +++ b/new.txt\n\
        @@ -1,2 +1,2 @@\n\
         a\n\
        -b\n\
        +B\n";
    let id = apply_and_get_checkpoint(&root, diff);
    assert!(!root.join("old.txt").exists());
    assert_eq!(fs::read_to_string(root.join("new.txt")).unwrap(), "a\nB\n");

    let resp = revert_patch(&root, &id);
    match resp {
        PatchRevertResponse::Ok(ok) => {
            assert_eq!(ok.restored.len(), 1);
            // user_facing_path = the OLD path; that's where the
            // file ends up after a rename revert.
            assert_eq!(ok.restored[0].path, "old.txt");
            assert_eq!(ok.restored[0].change_type, PatchChangeType::Rename);
        }
        PatchRevertResponse::Err(e) => panic!("expected ok, got {:?}", e),
    }
    assert!(!root.join("new.txt").exists());
    assert_eq!(fs::read_to_string(root.join("old.txt")).unwrap(), "a\nb\n");
}

// ─── Drift detection ────────────────────────────────────────────────────────

#[test]
fn revert_rejects_on_modify_drift() {
    let td = TempDir::new("rev-drift-mod");
    let root = canon_root(&td);
    fs::write(root.join("a.txt"), "a\nb\nc\n").unwrap();
    let diff = "--- a/a.txt\n\
        +++ b/a.txt\n\
        @@ -1,3 +1,3 @@\n\
         a\n\
        -b\n\
        +B\n\
         c\n";
    let id = apply_and_get_checkpoint(&root, diff);
    // Edit the post-apply state — that's the "user changed it
    // since apply" scenario revert exists to detect.
    fs::write(root.join("a.txt"), "a\nDIFFERENT\nc\n").unwrap();

    let resp = revert_patch(&root, &id);
    match resp {
        PatchRevertResponse::Err(e) => {
            assert_eq!(e.reason, PatchRevertFailure::Drift);
            assert!(e.details.iter().any(|d| d.path == "a.txt"));
        }
        PatchRevertResponse::Ok(_) => panic!("expected drift rejection"),
    }
    // No write happened.
    assert_eq!(
        fs::read_to_string(root.join("a.txt")).unwrap(),
        "a\nDIFFERENT\nc\n"
    );
}

#[test]
fn revert_rejects_on_delete_drift_when_file_recreated() {
    let td = TempDir::new("rev-drift-del");
    let root = canon_root(&td);
    fs::write(root.join("doomed.txt"), "line\n").unwrap();
    let diff = "--- a/doomed.txt\n\
        +++ /dev/null\n\
        @@ -1,1 +0,0 @@\n\
        -line\n";
    let id = apply_and_get_checkpoint(&root, diff);
    // User recreated the file with different content since apply.
    fs::write(root.join("doomed.txt"), "user added this\n").unwrap();

    let resp = revert_patch(&root, &id);
    match resp {
        PatchRevertResponse::Err(e) => assert_eq!(e.reason, PatchRevertFailure::Drift),
        PatchRevertResponse::Ok(_) => panic!("expected drift rejection"),
    }
}

#[test]
fn revert_rejects_on_create_drift_when_user_edited() {
    let td = TempDir::new("rev-drift-create");
    let root = canon_root(&td);
    let diff = "--- /dev/null\n\
        +++ b/new.txt\n\
        @@ -0,0 +1,1 @@\n\
        +one\n";
    let id = apply_and_get_checkpoint(&root, diff);
    // User edited the file Plume just created.
    fs::write(root.join("new.txt"), "userEdited\n").unwrap();

    let resp = revert_patch(&root, &id);
    match resp {
        PatchRevertResponse::Err(e) => assert_eq!(e.reason, PatchRevertFailure::Drift),
        PatchRevertResponse::Ok(_) => panic!("expected drift rejection"),
    }
}

// ─── Idempotency: second revert rejects ─────────────────────────────────────

#[test]
fn second_revert_with_same_id_rejects() {
    let td = TempDir::new("rev-twice");
    let root = canon_root(&td);
    fs::write(root.join("x.txt"), "1\n").unwrap();
    let diff = "--- a/x.txt\n\
        +++ b/x.txt\n\
        @@ -1,1 +1,1 @@\n\
        -1\n\
        +2\n";
    let id = apply_and_get_checkpoint(&root, diff);

    // First revert succeeds.
    match revert_patch(&root, &id) {
        PatchRevertResponse::Ok(_) => {}
        PatchRevertResponse::Err(e) => panic!("first revert: {:?}", e),
    }
    // Second revert with the same id: disk now matches the
    // PRE-apply state, not the post-apply state, so drift detect
    // rejects.
    match revert_patch(&root, &id) {
        PatchRevertResponse::Err(e) => {
            assert_eq!(e.reason, PatchRevertFailure::Drift);
        }
        PatchRevertResponse::Ok(_) => panic!("second revert should have rejected"),
    }
}

// ─── Unknown / malformed checkpoint id ──────────────────────────────────────

#[test]
fn revert_unknown_id_returns_unknown_checkpoint() {
    let td = TempDir::new("rev-unknown");
    let root = canon_root(&td);
    let resp = revert_patch(&root, "deadbeef00000000000000000000000000");
    match resp {
        PatchRevertResponse::Err(e) => assert_eq!(e.reason, PatchRevertFailure::UnknownCheckpoint),
        PatchRevertResponse::Ok(_) => panic!("expected unknownCheckpoint"),
    }
}

#[test]
fn revert_rejects_path_escape_in_id() {
    let td = TempDir::new("rev-escape");
    let root = canon_root(&td);
    // `..` in the id would let an attacker name a path outside
    // `.plume/checkpoints/` if we let it through.
    let resp = revert_patch(&root, "../../etc/passwd");
    match resp {
        PatchRevertResponse::Err(e) => {
            assert_eq!(e.reason, PatchRevertFailure::UnknownCheckpoint);
            assert!(e.details[0].message.contains("invalid checkpoint id"));
        }
        PatchRevertResponse::Ok(_) => panic!("expected rejection"),
    }
}

#[test]
fn revert_rejects_empty_id() {
    let td = TempDir::new("rev-empty");
    let root = canon_root(&td);
    let resp = revert_patch(&root, "");
    match resp {
        PatchRevertResponse::Err(e) => assert_eq!(e.reason, PatchRevertFailure::UnknownCheckpoint),
        PatchRevertResponse::Ok(_) => panic!("expected rejection"),
    }
}

// ─── Symlink defense on .plume/ ─────────────────────────────────────────────

#[cfg(unix)]
#[test]
fn revert_rejects_symlinked_plume_dir() {
    use std::os::unix::fs::symlink;
    let td = TempDir::new("rev-symlink");
    let root = canon_root(&td);
    // First apply something so a checkpoint exists; remember
    // the id BEFORE we wreck the .plume/ dir.
    fs::write(root.join("a.txt"), "a\n").unwrap();
    let diff = "--- a/a.txt\n\
        +++ b/a.txt\n\
        @@ -1,1 +1,1 @@\n\
        -a\n\
        +A\n";
    let id = apply_and_get_checkpoint(&root, diff);
    // Now replace .plume/ with a symlink to a temp directory
    // OUTSIDE the project. If `read_checkpoint`'s symlink guard
    // is missing, revert would happily read manifests from there
    // and write files based on what it found.
    let outside_td = TempDir::new("rev-symlink-outside");
    fs::remove_dir_all(root.join(".plume")).unwrap();
    symlink(outside_td.path(), root.join(".plume")).unwrap();

    let resp = revert_patch(&root, &id);
    match resp {
        PatchRevertResponse::Err(e) => {
            assert_eq!(e.reason, PatchRevertFailure::UnknownCheckpoint);
            assert!(
                e.details[0].message.contains("symlink"),
                "expected symlink rejection, got: {:?}",
                e.details[0].message
            );
        }
        PatchRevertResponse::Ok(_) => panic!("expected symlink-guard rejection"),
    }
}

// ─── Version gate: D31-vintage manifests reject ─────────────────────────────

#[test]
fn revert_rejects_v1_manifest_as_unsupported() {
    let td = TempDir::new("rev-v1");
    let root = canon_root(&td);
    fs::write(root.join("a.txt"), "a\n").unwrap();
    let diff = "--- a/a.txt\n\
        +++ b/a.txt\n\
        @@ -1,1 +1,1 @@\n\
        -a\n\
        +A\n";
    let id = apply_and_get_checkpoint(&root, diff);

    // Rewrite the manifest to drop the version field so it
    // deserializes as `version: 0` — the D31 shape.
    let manifest_path = root
        .join(".plume")
        .join("checkpoints")
        .join(&id)
        .join("manifest.json");
    let json = fs::read_to_string(&manifest_path).unwrap();
    // Strip the `"version": N,` line.
    let mut filtered: String = json
        .lines()
        .filter(|line| !line.trim_start().starts_with("\"version\""))
        .collect::<Vec<_>>()
        .join("\n");
    if !filtered.ends_with('\n') {
        filtered.push('\n');
    }
    fs::write(&manifest_path, filtered).unwrap();
    // Also remove the post/ tree to simulate a real D31 checkpoint.
    let post_dir = root
        .join(".plume")
        .join("checkpoints")
        .join(&id)
        .join("post");
    let _ = fs::remove_dir_all(&post_dir);

    let resp = revert_patch(&root, &id);
    match resp {
        PatchRevertResponse::Err(e) => {
            assert_eq!(e.reason, PatchRevertFailure::UnsupportedCheckpoint);
        }
        PatchRevertResponse::Ok(_) => panic!("expected unsupportedCheckpoint"),
    }
    // The pre-image is still there — the doc-string promise.
    let files_dir = root
        .join(".plume")
        .join("checkpoints")
        .join(&id)
        .join("files");
    assert!(files_dir.join("a.txt").exists());
}

// ─── Multi-file: all-or-nothing on partial drift ────────────────────────────

#[test]
fn revert_multi_file_rejects_atomically_when_one_drifted() {
    let td = TempDir::new("rev-multi-drift");
    let root = canon_root(&td);
    fs::write(root.join("a.txt"), "1\n").unwrap();
    fs::write(root.join("b.txt"), "2\n").unwrap();
    let diff = "--- a/a.txt\n\
        +++ b/a.txt\n\
        @@ -1,1 +1,1 @@\n\
        -1\n\
        +1!\n\
        --- a/b.txt\n\
        +++ b/b.txt\n\
        @@ -1,1 +1,1 @@\n\
        -2\n\
        +2!\n";
    let id = apply_and_get_checkpoint(&root, diff);
    // Drift on b.txt only.
    fs::write(root.join("b.txt"), "user changed it\n").unwrap();

    let resp = revert_patch(&root, &id);
    match resp {
        PatchRevertResponse::Err(e) => {
            assert_eq!(e.reason, PatchRevertFailure::Drift);
        }
        PatchRevertResponse::Ok(_) => panic!("expected drift"),
    }
    // a.txt was NOT restored despite being drift-clean — the
    // all-or-nothing contract.
    assert_eq!(fs::read_to_string(root.join("a.txt")).unwrap(), "1!\n");
    assert_eq!(
        fs::read_to_string(root.join("b.txt")).unwrap(),
        "user changed it\n"
    );
}
