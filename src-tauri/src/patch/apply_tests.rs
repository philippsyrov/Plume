//! Tests for `patch::apply`. Split into a sibling file via
//! `#[path]` so the production module stays under the
//! decomposition cap. See D24 for the same pattern.

use super::*;
use std::fs;
use std::path::PathBuf;

/// Minimal tempdir helper. Mirrors `safety::path::tests::TempDir`
/// without pulling in the `tempfile` crate.
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
            "plume-apply-test-{}-{}-{}",
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

// ─── Happy paths ────────────────────────────────────────────────────────────

#[test]
fn happy_modify_writes_post_image_and_creates_checkpoint() {
    let td = TempDir::new("happy-mod");
    let root = canon_root(&td);
    fs::write(root.join("hello.txt"), "a\nb\nc\n").unwrap();

    let diff = "--- a/hello.txt\n\
        +++ b/hello.txt\n\
        @@ -1,3 +1,3 @@\n\
         a\n\
        -b\n\
        +B\n\
         c\n";

    let resp = apply_patch(&root, diff);
    match resp {
        PatchApplyResponse::Ok(ok) => {
            assert!(ok.applied);
            assert!(!ok.checkpoint.is_empty(), "checkpoint id missing");
            assert_eq!(ok.touched.len(), 1);
            assert_eq!(ok.touched[0].path, "hello.txt");
            assert_eq!(ok.touched[0].change_type, PatchChangeType::Modify);
            // Post-image is "a\nB\nc\n" = 6 bytes.
            assert_eq!(ok.touched[0].bytes_written, 6);
        }
        PatchApplyResponse::Err(e) => panic!("expected ok, got {:?}", e),
    }

    let post = fs::read_to_string(root.join("hello.txt")).unwrap();
    assert_eq!(post, "a\nB\nc\n");

    // Checkpoint directory exists with the pre-image copy.
    let mut checkpoints: Vec<_> = fs::read_dir(root.join(".plume").join("checkpoints"))
        .unwrap()
        .filter_map(|e| e.ok())
        .collect();
    assert_eq!(checkpoints.len(), 1);
    let cp_dir = checkpoints.pop().unwrap().path();
    let saved = fs::read_to_string(cp_dir.join("files").join("hello.txt")).unwrap();
    assert_eq!(saved, "a\nb\nc\n");
    // Manifest entry exists.
    let manifest = fs::read_to_string(cp_dir.join("manifest.json")).unwrap();
    assert!(manifest.contains("\"hello.txt\""), "manifest missing path");
    assert!(
        manifest.contains("\"modify\""),
        "manifest missing change_type"
    );
}

#[test]
fn happy_create_writes_new_file() {
    let td = TempDir::new("happy-create");
    let root = canon_root(&td);

    let diff = "--- /dev/null\n\
        +++ b/src/new.rs\n\
        @@ -0,0 +1,2 @@\n\
        +fn main() {}\n\
        +// hello\n";

    let resp = apply_patch(&root, diff);
    match resp {
        PatchApplyResponse::Ok(ok) => {
            assert_eq!(ok.touched.len(), 1);
            assert_eq!(ok.touched[0].path, "src/new.rs");
            assert_eq!(ok.touched[0].change_type, PatchChangeType::Create);
        }
        PatchApplyResponse::Err(e) => panic!("expected ok, got {:?}", e),
    }

    let created = fs::read_to_string(root.join("src").join("new.rs")).unwrap();
    assert_eq!(created, "fn main() {}\n// hello\n");
}

#[test]
fn happy_delete_removes_file() {
    let td = TempDir::new("happy-del");
    let root = canon_root(&td);
    fs::write(root.join("doomed.txt"), "one\ntwo\n").unwrap();

    let diff = "--- a/doomed.txt\n\
        +++ /dev/null\n\
        @@ -1,2 +0,0 @@\n\
        -one\n\
        -two\n";

    let resp = apply_patch(&root, diff);
    match resp {
        PatchApplyResponse::Ok(ok) => {
            assert_eq!(ok.touched.len(), 1);
            assert_eq!(ok.touched[0].change_type, PatchChangeType::Delete);
            assert_eq!(ok.touched[0].bytes_written, 0);
        }
        PatchApplyResponse::Err(e) => panic!("expected ok, got {:?}", e),
    }

    assert!(!root.join("doomed.txt").exists());
    // Checkpoint preserved the pre-image so D32 revert can undo.
    let mut checkpoints: Vec<_> = fs::read_dir(root.join(".plume").join("checkpoints"))
        .unwrap()
        .filter_map(|e| e.ok())
        .collect();
    let cp = checkpoints.pop().unwrap().path();
    let saved = fs::read_to_string(cp.join("files").join("doomed.txt")).unwrap();
    assert_eq!(saved, "one\ntwo\n");
}

#[test]
fn happy_multi_file_modify_create_delete() {
    let td = TempDir::new("happy-multi");
    let root = canon_root(&td);
    fs::write(root.join("a.txt"), "alpha\n").unwrap();
    fs::write(root.join("b.txt"), "beta\n").unwrap();

    let diff = "--- a/a.txt\n\
        +++ b/a.txt\n\
        @@ -1,1 +1,1 @@\n\
        -alpha\n\
        +ALPHA\n\
        --- /dev/null\n\
        +++ b/c.txt\n\
        @@ -0,0 +1,1 @@\n\
        +gamma\n\
        --- a/b.txt\n\
        +++ /dev/null\n\
        @@ -1,1 +0,0 @@\n\
        -beta\n";

    let resp = apply_patch(&root, diff);
    match resp {
        PatchApplyResponse::Ok(ok) => assert_eq!(ok.touched.len(), 3),
        PatchApplyResponse::Err(e) => panic!("expected ok, got {:?}", e),
    }

    assert_eq!(fs::read_to_string(root.join("a.txt")).unwrap(), "ALPHA\n");
    assert_eq!(fs::read_to_string(root.join("c.txt")).unwrap(), "gamma\n");
    assert!(!root.join("b.txt").exists());
}

#[test]
fn apply_accepts_fenced_diff_block() {
    let td = TempDir::new("fenced");
    let root = canon_root(&td);
    fs::write(root.join("f.txt"), "x\n").unwrap();

    let diff = "```diff\n--- a/f.txt\n+++ b/f.txt\n@@ -1,1 +1,1 @@\n-x\n+X\n```";

    let resp = apply_patch(&root, diff);
    assert!(matches!(resp, PatchApplyResponse::Ok(_)));
    assert_eq!(fs::read_to_string(root.join("f.txt")).unwrap(), "X\n");
}

// ─── Safety: path escape / symlink ──────────────────────────────────────────

#[test]
fn rejects_path_escape_via_parent_dir() {
    let td = TempDir::new("escape");
    let root = canon_root(&td);

    let diff = "--- a/../etc/passwd\n\
        +++ b/../etc/passwd\n\
        @@ -1,1 +1,1 @@\n\
        -a\n\
        +A\n";

    let resp = apply_patch(&root, diff);
    match resp {
        PatchApplyResponse::Err(e) => {
            assert_eq!(e.reason, PatchApplyFailure::ValidationFailed);
            assert!(
                e.details.iter().any(|d| d.message.contains("..")),
                "no '..' detail in {:?}",
                e.details
            );
        }
        PatchApplyResponse::Ok(_) => panic!("expected validationFailed"),
    }
}

#[cfg(unix)]
#[test]
fn rejects_symlink_escape() {
    use std::os::unix::fs::symlink;
    let td_root = TempDir::new("sym-r");
    let td_outside = TempDir::new("sym-o");
    let root = canon_root(&td_root);

    let outside_file = td_outside.path().join("target.txt");
    fs::write(&outside_file, "outside\n").unwrap();
    let link_inside = td_root.path().join("link.txt");
    symlink(&outside_file, &link_inside).unwrap();

    let diff = "--- a/link.txt\n\
        +++ b/link.txt\n\
        @@ -1,1 +1,1 @@\n\
        -outside\n\
        +pwned\n";

    let resp = apply_patch(&root, diff);
    match resp {
        PatchApplyResponse::Err(e) => {
            assert_eq!(e.reason, PatchApplyFailure::ValidationFailed);
        }
        PatchApplyResponse::Ok(_) => panic!("expected validationFailed for symlink"),
    }
    // The outside file must not have been written.
    let outside_post = fs::read_to_string(&outside_file).unwrap();
    assert_eq!(outside_post, "outside\n");
}

// ─── Pre-image / scope ──────────────────────────────────────────────────────

#[test]
fn rejects_preimage_mismatch_without_writing() {
    let td = TempDir::new("mismatch");
    let root = canon_root(&td);
    fs::write(root.join("drifted.txt"), "actual\n").unwrap();

    // Diff thinks the file says "expected"; disk says "actual".
    let diff = "--- a/drifted.txt\n\
        +++ b/drifted.txt\n\
        @@ -1,1 +1,1 @@\n\
        -expected\n\
        +NEW\n";

    let resp = apply_patch(&root, diff);
    match resp {
        PatchApplyResponse::Err(e) => {
            assert_eq!(e.reason, PatchApplyFailure::PreImageMismatch);
            assert!(!e.details.is_empty());
            assert_eq!(e.details[0].path, "drifted.txt");
        }
        PatchApplyResponse::Ok(_) => panic!("expected preImageMismatch"),
    }
    // Disk unchanged.
    assert_eq!(
        fs::read_to_string(root.join("drifted.txt")).unwrap(),
        "actual\n"
    );
    // Checkpoint NOT created — we abort before touching .plume/.
    assert!(!root.join(".plume").join("checkpoints").exists());
}

#[test]
fn rejects_rename_as_scope_unsupported() {
    let td = TempDir::new("rename-rej");
    let root = canon_root(&td);
    fs::write(root.join("old.rs"), "x\n").unwrap();

    let diff = "--- a/old.rs\n\
        +++ b/new.rs\n\
        @@ -1,1 +1,1 @@\n\
        -x\n\
        +X\n";

    let resp = apply_patch(&root, diff);
    match resp {
        PatchApplyResponse::Err(e) => {
            assert_eq!(e.reason, PatchApplyFailure::ScopeUnsupported);
            assert!(e.details.iter().any(|d| d.message.contains("rename")));
        }
        PatchApplyResponse::Ok(_) => panic!("expected scopeUnsupported"),
    }
    // Disk unchanged.
    assert_eq!(fs::read_to_string(root.join("old.rs")).unwrap(), "x\n");
    assert!(!root.join("new.rs").exists());
}

#[test]
fn rejects_create_when_target_already_exists() {
    let td = TempDir::new("create-exists");
    let root = canon_root(&td);
    fs::write(root.join("there.txt"), "already\n").unwrap();

    let diff = "--- /dev/null\n\
        +++ b/there.txt\n\
        @@ -0,0 +1,1 @@\n\
        +new\n";

    let resp = apply_patch(&root, diff);
    match resp {
        PatchApplyResponse::Err(e) => {
            assert_eq!(e.reason, PatchApplyFailure::PreImageMismatch);
            assert!(e.details[0].message.contains("already exists"));
        }
        PatchApplyResponse::Ok(_) => panic!("expected preImageMismatch"),
    }
    assert_eq!(
        fs::read_to_string(root.join("there.txt")).unwrap(),
        "already\n"
    );
}

// ─── Atomic rollback ────────────────────────────────────────────────────────

#[test]
fn rolls_back_when_mid_apply_write_fails() {
    // Setup: two-file diff. First file modifies a real file
    // successfully. Second file is a create whose parent path is
    // already taken by a regular file — `create_dir_all(parent)`
    // fails, surfacing `writeFailed`. The first file's
    // post-image must be rolled back to its pre-image.
    let td = TempDir::new("rollback");
    let root = canon_root(&td);
    fs::write(root.join("first.txt"), "before\n").unwrap();
    // `blocker` is a regular file at the path that the second
    // plan wants to use as a directory.
    fs::write(root.join("blocker"), "i am a file\n").unwrap();

    let diff = "--- a/first.txt\n\
        +++ b/first.txt\n\
        @@ -1,1 +1,1 @@\n\
        -before\n\
        +AFTER\n\
        --- /dev/null\n\
        +++ b/blocker/cannot_go_here.txt\n\
        @@ -0,0 +1,1 @@\n\
        +nope\n";

    let resp = apply_patch(&root, diff);
    match resp {
        PatchApplyResponse::Err(e) => {
            assert_eq!(e.reason, PatchApplyFailure::WriteFailed);
            assert_eq!(e.details[0].path, "blocker/cannot_go_here.txt");
        }
        PatchApplyResponse::Ok(_) => panic!("expected writeFailed"),
    }
    // First file rolled back to pre-image.
    assert_eq!(
        fs::read_to_string(root.join("first.txt")).unwrap(),
        "before\n",
        "first.txt did not roll back"
    );
    // Blocker untouched.
    assert_eq!(
        fs::read_to_string(root.join("blocker")).unwrap(),
        "i am a file\n"
    );
}

// ─── Serialisation surface ──────────────────────────────────────────────────

#[test]
fn ok_response_serialises_with_camel_case() {
    let resp = PatchApplyResponse::Ok(PatchApplyOk {
        applied: true,
        checkpoint: "deadbeef".to_string(),
        touched: vec![PatchAppliedFile {
            path: "src/foo.rs".to_string(),
            change_type: PatchChangeType::Modify,
            bytes_written: 42,
        }],
    });
    let json = serde_json::to_value(&resp).unwrap();
    assert_eq!(json["applied"], serde_json::json!(true));
    assert_eq!(json["checkpoint"], serde_json::json!("deadbeef"));
    let t = &json["touched"][0];
    assert_eq!(t["path"], serde_json::json!("src/foo.rs"));
    assert_eq!(t["changeType"], serde_json::json!("modify"));
    assert_eq!(t["bytesWritten"], serde_json::json!(42));
}

#[test]
fn err_response_serialises_with_camel_case() {
    let resp = PatchApplyResponse::Err(PatchApplyErr {
        applied: false,
        reason: PatchApplyFailure::PreImageMismatch,
        details: vec![PatchFailureDetail {
            path: "x.rs".to_string(),
            hunk_index: Some(2),
            message: "drifted".to_string(),
        }],
    });
    let json = serde_json::to_value(&resp).unwrap();
    assert_eq!(json["applied"], serde_json::json!(false));
    assert_eq!(json["reason"], serde_json::json!("preImageMismatch"));
    let d = &json["details"][0];
    assert_eq!(d["path"], serde_json::json!("x.rs"));
    assert_eq!(d["hunkIndex"], serde_json::json!(2));
    assert_eq!(d["message"], serde_json::json!("drifted"));
}

#[test]
fn err_response_omits_empty_details() {
    let resp = PatchApplyResponse::Err(PatchApplyErr {
        applied: false,
        reason: PatchApplyFailure::CheckpointFailed,
        details: Vec::new(),
    });
    let json = serde_json::to_value(&resp).unwrap();
    assert!(
        json.get("details").is_none(),
        "empty details should be skipped"
    );
}
