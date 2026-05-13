//! Orchestrator for `patch.validate`.
//!
//! Runs the parser, then enforces project-root path safety on each
//! file. Composes the on-wire response shape.
//!
//! Path-safety policy is layered:
//!   1. Lexical: reject NUL, empty, absolute paths, `..`
//!      components. These rules apply even when the file does not
//!      yet exist (the "create" case).
//!   2. Existing-file canonicalize: when the joined path exists on
//!      disk, `safety::path::ensure_inside` catches symlink
//!      escapes too. We don't refuse paths just because they don't
//!      yet exist — a create-diff legitimately targets a missing
//!      file.

use std::path::{Component, Path, PathBuf};

use serde::Serialize;

use crate::patch::parse::{parse_diff, ChangeType, ParseError};
use crate::safety::path::ensure_inside;

/// On-wire response. Untagged so the JSON looks like
/// `{"ok": true, "touches": [...], "hunks": 4}` or
/// `{"ok": false, "errors": [...]}` — matching what
/// `docs/IPC_CONTRACT.md § patch` documents and giving TypeScript
/// a clean discriminated union to narrow on `resp.ok`.
#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum PatchValidateResponse {
    Ok(PatchValidateOk),
    Err(PatchValidateErr),
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PatchValidateOk {
    /// Always `true`. Discriminator the TS layer matches on.
    pub ok: bool,
    pub touches: Vec<PatchTouch>,
    /// Total hunks across all touched files.
    pub hunks: u32,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PatchValidateErr {
    /// Always `false`. Discriminator the TS layer matches on.
    pub ok: bool,
    /// At least one entry. The frontend uses `errors[0]` as the
    /// headline for the validation pill; the rest are available
    /// for a "see all" surface if we ever add one.
    pub errors: Vec<PatchValidationError>,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PatchTouch {
    /// Project-relative, normalised (no `./` segments) path. For
    /// renames this is the destination; the source is in
    /// `renamed_from`.
    pub path: String,
    pub hunks: u32,
    pub change_type: PatchChangeType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub renamed_from: Option<String>,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum PatchChangeType {
    Modify,
    Create,
    Delete,
    Rename,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PatchValidationError {
    pub kind: PatchValidationErrorKind,
    /// Human-readable text. Always populated; the kind is the
    /// machine-stable discriminator.
    pub message: String,
    /// Diff path the error attached to, when applicable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// 1-based line offset inside the post-fence-extraction body,
    /// when applicable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum PatchValidationErrorKind {
    /// Input had no diff content (empty, no fence, or fence with
    /// no diff inside).
    NoDiffBlock,
    /// File group had no `@@` hunk header.
    NoHunks,
    /// Generic shape failure — malformed hunk header, plus before
    /// minus, etc. See `message` for specifics.
    Malformed,
    /// `--- /dev/null` AND `+++ /dev/null` in the same file group.
    DevNullBoth,
    /// Path escapes the project root via `..` segments or via a
    /// symlinked-out target the canonicaliser caught.
    PathEscape,
    /// Path was absolute (`/etc/passwd`, `C:\Windows\...`) or
    /// contained a NUL byte.
    AbsolutePath,
}

/// Validate a model-emitted unified diff against a trusted project
/// root. Returns `Ok(Valid {...})` on success, `Ok(Invalid {...})`
/// when the diff parsed-or-path-safety-checked into one or more
/// structured errors. The `Result` outer layer is reserved for
/// truly unexpected internal failures; today this function never
/// returns `Err`.
///
/// Note this is read-only: no disk writes, no patch apply, no
/// checkpoint. The brief for D16 is explicit about that.
pub fn validate_patch(project_root: &Path, diff: &str) -> PatchValidateResponse {
    let parsed = match parse_diff(diff) {
        Ok(files) => files,
        Err(e) => return parse_error_to_response(e),
    };

    let mut errors: Vec<PatchValidationError> = Vec::new();
    let mut touches: Vec<PatchTouch> = Vec::new();
    let mut total_hunks: u32 = 0;

    for file in parsed.iter() {
        let mut file_errors: Vec<PatchValidationError> = Vec::new();
        let normalized_path = match check_diff_path(project_root, &file.path) {
            Ok(p) => Some(p),
            Err(e) => {
                file_errors.push(e);
                None
            }
        };
        let normalized_renamed_from = if let Some(from) = file.renamed_from.as_deref() {
            match check_diff_path(project_root, from) {
                Ok(p) => Some(p),
                Err(e) => {
                    file_errors.push(e);
                    None
                }
            }
        } else {
            None
        };

        if !file_errors.is_empty() {
            errors.append(&mut file_errors);
            continue;
        }

        // Safe to unwrap: empty file_errors means both checks
        // populated their `Ok`.
        let path = normalized_path.expect("normalised path missing despite no errors");
        let touch = PatchTouch {
            path,
            hunks: file.hunk_count,
            change_type: map_change_type(file.change_type),
            renamed_from: normalized_renamed_from,
        };
        total_hunks = total_hunks.saturating_add(touch.hunks);
        touches.push(touch);
    }

    if !errors.is_empty() {
        return PatchValidateResponse::Err(PatchValidateErr { ok: false, errors });
    }

    PatchValidateResponse::Ok(PatchValidateOk {
        ok: true,
        touches,
        hunks: total_hunks,
    })
}

fn map_change_type(c: ChangeType) -> PatchChangeType {
    match c {
        ChangeType::Modify => PatchChangeType::Modify,
        ChangeType::Create => PatchChangeType::Create,
        ChangeType::Delete => PatchChangeType::Delete,
        ChangeType::Rename => PatchChangeType::Rename,
    }
}

fn parse_error_to_response(e: ParseError) -> PatchValidateResponse {
    let err = match e {
        ParseError::NoDiffBlock => PatchValidationError {
            kind: PatchValidationErrorKind::NoDiffBlock,
            message: "no unified-diff content found in the reply".to_string(),
            path: None,
            line: None,
        },
        ParseError::NoHunks { path, line } => PatchValidationError {
            kind: PatchValidationErrorKind::NoHunks,
            message: format!("file '{}' has no hunks (`@@` headers)", path),
            path: Some(path),
            line: Some(line),
        },
        ParseError::DevNullBoth { line } => PatchValidationError {
            kind: PatchValidationErrorKind::DevNullBoth,
            message: "both sides of the file headers were /dev/null".to_string(),
            path: None,
            line: Some(line),
        },
        ParseError::Malformed { line, message } => PatchValidationError {
            kind: PatchValidationErrorKind::Malformed,
            message,
            path: None,
            line: Some(line),
        },
    };
    PatchValidateResponse::Err(PatchValidateErr {
        ok: false,
        errors: vec![err],
    })
}

/// Apply path safety to a diff-side path. Returns the project-
/// relative, normalised path on success. Existing-file
/// canonicalisation is layered on top of the lexical check so a
/// symlink in the project that points outside is also caught.
fn check_diff_path(project_root: &Path, raw: &str) -> Result<String, PatchValidationError> {
    if raw.is_empty() {
        return Err(PatchValidationError {
            kind: PatchValidationErrorKind::Malformed,
            message: "empty file path in diff".to_string(),
            path: None,
            line: None,
        });
    }
    if raw.contains('\0') {
        return Err(PatchValidationError {
            kind: PatchValidationErrorKind::AbsolutePath,
            message: format!("path contains NUL byte: {:?}", raw),
            path: Some(raw.to_string()),
            line: None,
        });
    }
    if raw == "/dev/null" {
        // Shouldn't reach here — parser canonicalises this into a
        // `change_type` flag — but defending the boundary anyway.
        return Err(PatchValidationError {
            kind: PatchValidationErrorKind::Malformed,
            message: "/dev/null appeared as a real file path".to_string(),
            path: Some(raw.to_string()),
            line: None,
        });
    }

    let p = Path::new(raw);
    if p.is_absolute() {
        return Err(PatchValidationError {
            kind: PatchValidationErrorKind::AbsolutePath,
            message: format!("absolute path is not allowed: {}", raw),
            path: Some(raw.to_string()),
            line: None,
        });
    }

    let mut normalised = PathBuf::new();
    for component in p.components() {
        match component {
            Component::ParentDir => {
                return Err(PatchValidationError {
                    kind: PatchValidationErrorKind::PathEscape,
                    message: format!("path contains '..' component: {}", raw),
                    path: Some(raw.to_string()),
                    line: None,
                });
            }
            Component::Prefix(_) | Component::RootDir => {
                return Err(PatchValidationError {
                    kind: PatchValidationErrorKind::AbsolutePath,
                    message: format!("absolute path is not allowed: {}", raw),
                    path: Some(raw.to_string()),
                    line: None,
                });
            }
            Component::CurDir => {}
            Component::Normal(seg) => normalised.push(seg),
        }
    }

    if normalised.as_os_str().is_empty() {
        return Err(PatchValidationError {
            kind: PatchValidationErrorKind::Malformed,
            message: format!("file path resolves to nothing: {}", raw),
            path: Some(raw.to_string()),
            line: None,
        });
    }

    // Lexical check is sufficient for non-existing (create) files.
    // For existing files, also canonicalize so a symlink in the
    // working tree that points outside the root gets caught.
    let joined = project_root.join(&normalised);
    if joined.exists() {
        if let Err(e) = ensure_inside(project_root, &joined) {
            return Err(PatchValidationError {
                kind: PatchValidationErrorKind::PathEscape,
                message: format!("path escapes project root: {} ({})", raw, e),
                path: Some(raw.to_string()),
                line: None,
            });
        }
    }

    // Re-stringify with forward slashes so the wire shape is
    // platform-independent. `PathBuf` uses backslashes on Windows.
    let mut out = String::new();
    for (i, seg) in normalised.iter().enumerate() {
        if i > 0 {
            out.push('/');
        }
        out.push_str(&seg.to_string_lossy());
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

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
                "plume-patch-test-{}-{}-{}",
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

    const HAPPY_DIFF: &str = "--- a/src/foo.rs\n\
        +++ b/src/foo.rs\n\
        @@ -1,3 +1,3 @@\n\
         a\n\
        -b\n\
        +B\n\
         c\n";

    #[test]
    fn valid_diff_returns_touches_and_hunks() {
        let td = TempDir::new("happy");
        let root = canon_root(&td);
        let resp = validate_patch(&root, HAPPY_DIFF);
        match resp {
            PatchValidateResponse::Ok(ok) => {
                assert!(ok.ok);
                assert_eq!(ok.touches.len(), 1);
                assert_eq!(ok.touches[0].path, "src/foo.rs");
                assert_eq!(ok.touches[0].hunks, 1);
                assert_eq!(ok.touches[0].change_type, PatchChangeType::Modify);
                assert_eq!(ok.hunks, 1);
            }
            PatchValidateResponse::Err(e) => panic!("expected ok, got errors {:?}", e.errors),
        }
    }

    #[test]
    fn multi_file_diff_sums_hunks() {
        let td = TempDir::new("multi");
        let root = canon_root(&td);
        let input = "--- a/x\n\
            +++ b/x\n\
            @@ -1,1 +1,1 @@\n\
            -a\n\
            +A\n\
            @@ -10,1 +10,1 @@\n\
            -b\n\
            +B\n\
            --- a/y\n\
            +++ b/y\n\
            @@ -1,1 +1,1 @@\n\
            -c\n\
            +C\n";
        let resp = validate_patch(&root, input);
        match resp {
            PatchValidateResponse::Ok(ok) => {
                assert_eq!(ok.touches.len(), 2);
                assert_eq!(ok.touches[0].hunks, 2);
                assert_eq!(ok.touches[1].hunks, 1);
                assert_eq!(ok.hunks, 3);
            }
            PatchValidateResponse::Err(e) => panic!("expected ok, got errors {:?}", e.errors),
        }
    }

    #[test]
    fn malformed_diff_reports_malformed_error() {
        let td = TempDir::new("malformed");
        let root = canon_root(&td);
        let input = "--- a/x\n+++ b/x\n@@ this is not a hunk header @@\n";
        let resp = validate_patch(&root, input);
        match resp {
            PatchValidateResponse::Err(e) => {
                assert!(!e.ok);
                assert!(matches!(
                    e.errors[0].kind,
                    PatchValidationErrorKind::Malformed
                ));
            }
            PatchValidateResponse::Ok(_) => panic!("expected err"),
        }
    }

    #[test]
    fn no_files_input_reports_no_diff_block() {
        let td = TempDir::new("nofiles");
        let root = canon_root(&td);
        let resp = validate_patch(&root, "just some prose, no diff content here\n");
        match resp {
            PatchValidateResponse::Err(e) => {
                assert!(matches!(
                    e.errors[0].kind,
                    PatchValidationErrorKind::NoDiffBlock
                ));
            }
            PatchValidateResponse::Ok(_) => panic!("expected err"),
        }
    }

    #[test]
    fn empty_input_reports_no_diff_block() {
        let td = TempDir::new("empty");
        let root = canon_root(&td);
        let resp = validate_patch(&root, "");
        match resp {
            PatchValidateResponse::Err(e) => {
                assert!(matches!(
                    e.errors[0].kind,
                    PatchValidationErrorKind::NoDiffBlock
                ));
            }
            PatchValidateResponse::Ok(_) => panic!("expected err"),
        }
    }

    #[test]
    fn path_escape_via_parent_dir_is_rejected() {
        let td = TempDir::new("escape");
        let root = canon_root(&td);
        let input = "--- a/../etc/passwd\n\
            +++ b/../etc/passwd\n\
            @@ -1,1 +1,1 @@\n\
            -a\n\
            +A\n";
        let resp = validate_patch(&root, input);
        match resp {
            PatchValidateResponse::Err(e) => {
                assert!(matches!(
                    e.errors[0].kind,
                    PatchValidationErrorKind::PathEscape
                ));
                assert!(e.errors[0].message.contains(".."));
            }
            PatchValidateResponse::Ok(_) => panic!("expected err"),
        }
    }

    #[test]
    fn absolute_path_is_rejected() {
        let td = TempDir::new("absolute");
        let root = canon_root(&td);
        let input = "--- /etc/passwd\n\
            +++ /etc/passwd\n\
            @@ -1,1 +1,1 @@\n\
            -a\n\
            +A\n";
        let resp = validate_patch(&root, input);
        match resp {
            PatchValidateResponse::Err(e) => {
                assert!(matches!(
                    e.errors[0].kind,
                    PatchValidationErrorKind::AbsolutePath
                ));
            }
            PatchValidateResponse::Ok(_) => panic!("expected err"),
        }
    }

    #[test]
    fn create_diff_against_missing_file_is_valid() {
        let td = TempDir::new("create");
        let root = canon_root(&td);
        let input = "--- /dev/null\n\
            +++ b/src/new.rs\n\
            @@ -0,0 +1,1 @@\n\
            +hello\n";
        let resp = validate_patch(&root, input);
        match resp {
            PatchValidateResponse::Ok(ok) => {
                assert_eq!(ok.touches[0].path, "src/new.rs");
                assert_eq!(ok.touches[0].change_type, PatchChangeType::Create);
            }
            PatchValidateResponse::Err(e) => panic!("expected ok, got {:?}", e.errors),
        }
    }

    #[test]
    fn rename_diff_carries_renamed_from() {
        let td = TempDir::new("rename");
        let root = canon_root(&td);
        let input = "--- a/old.rs\n\
            +++ b/new.rs\n\
            @@ -1,1 +1,1 @@\n\
            -a\n\
            +A\n";
        let resp = validate_patch(&root, input);
        match resp {
            PatchValidateResponse::Ok(ok) => {
                assert_eq!(ok.touches[0].change_type, PatchChangeType::Rename);
                assert_eq!(ok.touches[0].path, "new.rs");
                assert_eq!(ok.touches[0].renamed_from.as_deref(), Some("old.rs"));
            }
            PatchValidateResponse::Err(e) => panic!("expected ok, got {:?}", e.errors),
        }
    }

    #[test]
    fn symlink_escape_is_rejected_via_canonicalize() {
        // The lexical check passes (no `..`, not absolute), but an
        // existing symlink in the project that points outside should
        // be caught by `ensure_inside`'s canonicalize step.
        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            let td_root = TempDir::new("sym-r");
            let td_outside = TempDir::new("sym-o");
            let root = canon_root(&td_root);
            let outside_file = td_outside.path().join("escape.txt");
            fs::write(&outside_file, b"x").unwrap();
            let link_inside = td_root.path().join("escape.txt");
            symlink(&outside_file, &link_inside).unwrap();
            let input = "--- a/escape.txt\n\
                +++ b/escape.txt\n\
                @@ -1,1 +1,1 @@\n\
                -a\n\
                +A\n";
            let resp = validate_patch(&root, input);
            match resp {
                PatchValidateResponse::Err(e) => {
                    assert!(matches!(
                        e.errors[0].kind,
                        PatchValidationErrorKind::PathEscape
                    ));
                }
                PatchValidateResponse::Ok(_) => panic!("expected err"),
            }
        }
    }

    #[test]
    fn fenced_diff_input_validates() {
        let td = TempDir::new("fenced");
        let root = canon_root(&td);
        let input = format!("```diff\n{}```", HAPPY_DIFF);
        let resp = validate_patch(&root, &input);
        match resp {
            PatchValidateResponse::Ok(ok) => {
                assert_eq!(ok.touches.len(), 1);
                assert_eq!(ok.touches[0].path, "src/foo.rs");
            }
            PatchValidateResponse::Err(e) => panic!("expected ok, got {:?}", e.errors),
        }
    }

    #[test]
    fn ok_response_serializes_with_camel_case_fields() {
        let resp = PatchValidateResponse::Ok(PatchValidateOk {
            ok: true,
            touches: vec![PatchTouch {
                path: "src/foo.rs".to_string(),
                hunks: 2,
                change_type: PatchChangeType::Modify,
                renamed_from: None,
            }],
            hunks: 2,
        });
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["ok"], serde_json::json!(true));
        assert_eq!(json["hunks"], serde_json::json!(2));
        let t = &json["touches"][0];
        assert_eq!(t["path"], serde_json::json!("src/foo.rs"));
        assert_eq!(t["hunks"], serde_json::json!(2));
        // `change_type` field renamed via outer struct rename_all.
        assert_eq!(t["changeType"], serde_json::json!("modify"));
        // `renamed_from` is skipped when None.
        assert!(t.get("renamedFrom").is_none());
    }

    #[test]
    fn rename_serializes_renamed_from_field() {
        let resp = PatchValidateResponse::Ok(PatchValidateOk {
            ok: true,
            touches: vec![PatchTouch {
                path: "new.rs".to_string(),
                hunks: 1,
                change_type: PatchChangeType::Rename,
                renamed_from: Some("old.rs".to_string()),
            }],
            hunks: 1,
        });
        let json = serde_json::to_value(&resp).unwrap();
        let t = &json["touches"][0];
        assert_eq!(t["renamedFrom"], serde_json::json!("old.rs"));
        assert_eq!(t["changeType"], serde_json::json!("rename"));
    }

    #[test]
    fn err_response_serializes_with_camel_case_fields() {
        let resp = PatchValidateResponse::Err(PatchValidateErr {
            ok: false,
            errors: vec![PatchValidationError {
                kind: PatchValidationErrorKind::PathEscape,
                message: "path escapes".to_string(),
                path: Some("../etc".to_string()),
                line: Some(1),
            }],
        });
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["ok"], serde_json::json!(false));
        let e = &json["errors"][0];
        assert_eq!(e["kind"], serde_json::json!("pathEscape"));
        assert_eq!(e["message"], serde_json::json!("path escapes"));
        assert_eq!(e["path"], serde_json::json!("../etc"));
        assert_eq!(e["line"], serde_json::json!(1));
    }

    #[test]
    fn err_response_skips_optional_fields_when_none() {
        let resp = PatchValidateResponse::Err(PatchValidateErr {
            ok: false,
            errors: vec![PatchValidationError {
                kind: PatchValidationErrorKind::NoDiffBlock,
                message: "no diff".to_string(),
                path: None,
                line: None,
            }],
        });
        let json = serde_json::to_value(&resp).unwrap();
        let e = &json["errors"][0];
        assert!(e.get("path").is_none());
        assert!(e.get("line").is_none());
    }
}
