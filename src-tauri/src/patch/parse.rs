//! Unified-diff parser. Internal to `patch`; no public IPC surface.
//!
//! The parser is line-oriented and forgiving where forgiveness is
//! cheap. It accepts:
//!   * A fenced ```diff/```patch block (model output verbatim).
//!   * An untagged fence whose first two lines are `--- ` / `+++ `.
//!   * A bare unified diff.
//!
//! Each file group is identified by a `--- ` / `+++ ` header pair;
//! the `diff --git` header line (when present) is informational
//! and does not start a new group on its own. Hunks are counted
//! by `@@` headers; their bodies are not validated against any
//! pre-image (that's `patch.apply`'s job).

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedFile {
    /// Path used as the file's identity. For deletes this is the
    /// old path; for everything else it's the new path. Header
    /// `a/` and `b/` prefixes are stripped; `/dev/null` is never
    /// present here (it canonicalises into a `change_type` flag).
    pub path: String,
    /// Set only when `change_type == Rename`. Carries the old
    /// (pre-rename) path. Stripped of `a/` / `b/` prefix.
    pub renamed_from: Option<String>,
    pub change_type: ChangeType,
    pub hunk_count: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeType {
    Modify,
    Create,
    Delete,
    Rename,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    /// Input had no `--- ` / `+++ ` pair anywhere. Either no
    /// fenced block, or the fenced block didn't contain a diff,
    /// or the input was empty.
    NoDiffBlock,
    /// A file group had `--- ` / `+++ ` headers but no `@@` hunk
    /// header below them. `path` is the post-strip new-side path.
    NoHunks { path: String, line: u32 },
    /// Both sides of a header pair were `/dev/null`. The diff
    /// doesn't reference a real file at all.
    DevNullBoth { line: u32 },
    /// Generic syntactic failure. `line` is the 1-based offset in
    /// the post-fence-extraction body where the parser gave up;
    /// `message` is a human-readable explanation.
    Malformed { line: u32, message: String },
}

pub fn parse_diff(input: &str) -> Result<Vec<ParsedFile>, ParseError> {
    let body = extract_fenced_block(input).unwrap_or(input);
    if !body.contains("--- ") {
        return Err(ParseError::NoDiffBlock);
    }

    let mut files: Vec<ParsedFile> = Vec::new();
    let mut partial: Option<PartialFile> = None;
    let mut rename_from: Option<String> = None;
    let mut rename_to: Option<String> = None;

    for (idx, line) in body.lines().enumerate() {
        let lineno = (idx + 1) as u32;

        // `diff --git a/old b/new` introduces a file group at the
        // git layer. We capture a pending rename here only if the
        // two paths differ; the actual change-type classification
        // happens at commit time using the `--- /+++ ` pair.
        if let Some(rest) = line.strip_prefix("diff --git ") {
            commit_file(&mut files, partial.take(), &mut rename_from, &mut rename_to)?;
            // Reset rename markers from any prior group.
            rename_from = None;
            rename_to = None;
            let _ = rest;
            continue;
        }

        if let Some(rest) = line.strip_prefix("rename from ") {
            rename_from = Some(rest.trim().to_string());
            continue;
        }
        if let Some(rest) = line.strip_prefix("rename to ") {
            rename_to = Some(rest.trim().to_string());
            continue;
        }

        if let Some(rest) = line.strip_prefix("--- ") {
            commit_file(&mut files, partial.take(), &mut rename_from, &mut rename_to)?;
            partial = Some(PartialFile {
                old_raw: rest.to_string(),
                new_raw: None,
                hunks: 0,
                start_line: lineno,
            });
            continue;
        }

        if let Some(rest) = line.strip_prefix("+++ ") {
            let Some(file) = partial.as_mut() else {
                return Err(ParseError::Malformed {
                    line: lineno,
                    message: "+++ header without preceding --- header".to_string(),
                });
            };
            if file.new_raw.is_some() {
                return Err(ParseError::Malformed {
                    line: lineno,
                    message: "two +++ headers in the same file group".to_string(),
                });
            }
            file.new_raw = Some(rest.to_string());
            continue;
        }

        if line.starts_with("@@") {
            let Some(file) = partial.as_mut() else {
                return Err(ParseError::Malformed {
                    line: lineno,
                    message: "hunk header before file headers".to_string(),
                });
            };
            if file.new_raw.is_none() {
                return Err(ParseError::Malformed {
                    line: lineno,
                    message: "hunk header before +++ header".to_string(),
                });
            }
            validate_hunk_header(line, lineno)?;
            file.hunks += 1;
            continue;
        }

        // Body lines (` `, `+`, `-`, `\`) and metadata
        // (`index`, `similarity`, `old mode`, `new mode`, prose
        // between groups) are tolerated without strict checking.
    }

    commit_file(&mut files, partial.take(), &mut rename_from, &mut rename_to)?;

    if files.is_empty() {
        return Err(ParseError::NoDiffBlock);
    }
    Ok(files)
}

struct PartialFile {
    old_raw: String,
    new_raw: Option<String>,
    hunks: u32,
    start_line: u32,
}

fn commit_file(
    files: &mut Vec<ParsedFile>,
    partial: Option<PartialFile>,
    rename_from: &mut Option<String>,
    rename_to: &mut Option<String>,
) -> Result<(), ParseError> {
    let Some(p) = partial else {
        // No file was being assembled. Drop any stray rename
        // markers — they belonged to a `diff --git` that did not
        // produce a `--- /+++ ` pair.
        *rename_from = None;
        *rename_to = None;
        return Ok(());
    };
    let Some(new_raw) = p.new_raw else {
        return Err(ParseError::Malformed {
            line: p.start_line,
            message: "--- header without matching +++ header before EOF".to_string(),
        });
    };
    let old = normalize_header_path(&p.old_raw);
    let new = normalize_header_path(&new_raw);

    if old == "/dev/null" && new == "/dev/null" {
        return Err(ParseError::DevNullBoth { line: p.start_line });
    }

    let (change_type, path, renamed_from_path) = if old == "/dev/null" {
        (ChangeType::Create, new.clone(), None)
    } else if new == "/dev/null" {
        (ChangeType::Delete, old.clone(), None)
    } else if rename_from.is_some() || rename_to.is_some() || old != new {
        // Either git's rename markers are set, or the header pair
        // names different paths — both signal a rename. Prefer
        // the explicit `rename from` value when present; fall
        // back to the old header.
        let from = rename_from.clone().unwrap_or_else(|| old.clone());
        (ChangeType::Rename, new.clone(), Some(from))
    } else {
        (ChangeType::Modify, new.clone(), None)
    };

    // No hunks: only acceptable when the diff itself carries no
    // body — but for D16's narrow scope, every claimed file must
    // touch at least one hunk. Pure rename-no-change diffs aren't
    // expected from a model's propose-diff response, and accepting
    // them would mean validation passes on a no-op.
    if p.hunks == 0 {
        return Err(ParseError::NoHunks {
            path: path.clone(),
            line: p.start_line,
        });
    }

    files.push(ParsedFile {
        path,
        renamed_from: renamed_from_path,
        change_type,
        hunk_count: p.hunks,
    });

    *rename_from = None;
    *rename_to = None;
    Ok(())
}

/// Pulls the body out of a fenced ```diff/```patch block (or an
/// untagged fence whose first two body lines look like diff
/// headers). Returns `None` if no fence is found — the caller
/// then treats the raw input as the diff body.
fn extract_fenced_block(input: &str) -> Option<&str> {
    // Tagged fence: ```diff\n...\n``` or ```patch\n...\n```.
    if let Some(body) = find_fenced(input, &["diff", "patch"]) {
        return Some(body);
    }
    // Untagged / unknown-tagged fence: only accept if the first
    // two non-empty lines look like unified-diff headers.
    if let Some(body) = find_fenced(input, &[""]) {
        let mut lines = body.lines().filter(|l| !l.trim().is_empty());
        let l1 = lines.next()?;
        let l2 = lines.next()?;
        if (l1.starts_with("--- ") && l2.starts_with("+++ ")) || l1.starts_with("diff --git ") {
            return Some(body);
        }
    }
    None
}

/// Locate the body of the first fenced code block in `input`. When
/// `tags` includes an empty string, any tag (or no tag) matches.
fn find_fenced<'a>(input: &'a str, tags: &[&str]) -> Option<&'a str> {
    let mut idx = 0;
    while let Some(start) = input[idx..].find("```") {
        let abs = idx + start + 3;
        let line_end = input[abs..]
            .find('\n')
            .map(|n| abs + n)
            .unwrap_or(input.len());
        let tag = input[abs..line_end].trim();
        let accept = tags.iter().any(|t| {
            if t.is_empty() {
                true
            } else {
                tag.eq_ignore_ascii_case(t)
            }
        });
        let body_start = (line_end + 1).min(input.len());
        if accept {
            if let Some(close) = input[body_start..].find("\n```") {
                let body_end = body_start + close;
                return Some(&input[body_start..body_end]);
            }
        }
        idx = body_start;
    }
    None
}

/// Strip the leading `a/` / `b/` from a unified-diff header path
/// and trim any trailing tab-separated metadata (timestamp, etc.).
/// `/dev/null` is preserved verbatim so the caller can detect it.
fn normalize_header_path(raw: &str) -> String {
    let cut = raw.split('\t').next().unwrap_or(raw).trim();
    if cut == "/dev/null" {
        return cut.to_string();
    }
    if let Some(stripped) = cut.strip_prefix("a/") {
        return stripped.to_string();
    }
    if let Some(stripped) = cut.strip_prefix("b/") {
        return stripped.to_string();
    }
    cut.to_string()
}

/// Cheap shape check on `@@ -<a>[,<b>] +<c>[,<d>] @@ [optional context]`.
/// Rejects clearly-malformed headers; accepts the common variants.
fn validate_hunk_header(line: &str, lineno: u32) -> Result<(), ParseError> {
    // `@@` followed by space, `-<num>[,<num>]`, space, `+<num>[,<num>]`,
    // space, `@@`, then optional ` <context>`.
    let rest = line
        .strip_prefix("@@")
        .ok_or_else(|| ParseError::Malformed {
            line: lineno,
            message: "hunk header missing opening @@".to_string(),
        })?;
    let rest = rest.trim_start();
    let close_idx = rest.find("@@").ok_or_else(|| ParseError::Malformed {
        line: lineno,
        message: "hunk header missing closing @@".to_string(),
    })?;
    let inside = rest[..close_idx].trim();
    let parts: Vec<&str> = inside.split_whitespace().collect();
    if parts.len() != 2 {
        return Err(ParseError::Malformed {
            line: lineno,
            message: format!("hunk header malformed: '{}'", line),
        });
    }
    if !parts[0].starts_with('-') || !parts[1].starts_with('+') {
        return Err(ParseError::Malformed {
            line: lineno,
            message: format!("hunk header malformed: '{}'", line),
        });
    }
    if !parts[0][1..]
        .chars()
        .all(|c| c.is_ascii_digit() || c == ',')
    {
        return Err(ParseError::Malformed {
            line: lineno,
            message: format!("hunk header malformed: '{}'", line),
        });
    }
    if !parts[1][1..]
        .chars()
        .all(|c| c.is_ascii_digit() || c == ',')
    {
        return Err(ParseError::Malformed {
            line: lineno,
            message: format!("hunk header malformed: '{}'", line),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const HAPPY: &str = "--- a/src/foo.rs\n\
        +++ b/src/foo.rs\n\
        @@ -1,3 +1,3 @@\n\
         a\n\
        -b\n\
        +B\n\
         c\n";

    #[test]
    fn parses_happy_single_file() {
        let files = parse_diff(HAPPY).unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, "src/foo.rs");
        assert_eq!(files[0].change_type, ChangeType::Modify);
        assert_eq!(files[0].hunk_count, 1);
        assert!(files[0].renamed_from.is_none());
    }

    #[test]
    fn counts_multiple_hunks_in_one_file() {
        let input = "--- a/x\n\
            +++ b/x\n\
            @@ -1,1 +1,1 @@\n\
            -a\n\
            +A\n\
            @@ -10,1 +10,1 @@\n\
            -b\n\
            +B\n";
        let files = parse_diff(input).unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].hunk_count, 2);
    }

    #[test]
    fn parses_multi_file_diff() {
        let input = "--- a/x\n\
            +++ b/x\n\
            @@ -1,1 +1,1 @@\n\
            -a\n\
            +A\n\
            --- a/y\n\
            +++ b/y\n\
            @@ -1,1 +1,1 @@\n\
            -b\n\
            +B\n";
        let files = parse_diff(input).unwrap();
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].path, "x");
        assert_eq!(files[1].path, "y");
    }

    #[test]
    fn detects_create_via_dev_null_on_old_side() {
        let input = "--- /dev/null\n\
            +++ b/src/new.rs\n\
            @@ -0,0 +1,1 @@\n\
            +hello\n";
        let files = parse_diff(input).unwrap();
        assert_eq!(files[0].change_type, ChangeType::Create);
        assert_eq!(files[0].path, "src/new.rs");
    }

    #[test]
    fn detects_delete_via_dev_null_on_new_side() {
        let input = "--- a/old.rs\n\
            +++ /dev/null\n\
            @@ -1,1 +0,0 @@\n\
            -bye\n";
        let files = parse_diff(input).unwrap();
        assert_eq!(files[0].change_type, ChangeType::Delete);
        assert_eq!(files[0].path, "old.rs");
    }

    #[test]
    fn rejects_dev_null_on_both_sides() {
        let input = "--- /dev/null\n\
            +++ /dev/null\n\
            @@ -0,0 +0,0 @@\n";
        let err = parse_diff(input).unwrap_err();
        assert!(matches!(err, ParseError::DevNullBoth { .. }), "got {err:?}");
    }

    #[test]
    fn detects_rename_via_differing_header_paths() {
        let input = "--- a/old.rs\n\
            +++ b/new.rs\n\
            @@ -1,1 +1,1 @@\n\
            -a\n\
            +A\n";
        let files = parse_diff(input).unwrap();
        assert_eq!(files[0].change_type, ChangeType::Rename);
        assert_eq!(files[0].path, "new.rs");
        assert_eq!(files[0].renamed_from.as_deref(), Some("old.rs"));
    }

    #[test]
    fn detects_rename_via_git_markers() {
        let input = "diff --git a/old b/new\n\
            similarity index 95%\n\
            rename from old\n\
            rename to new\n\
            --- a/old\n\
            +++ b/new\n\
            @@ -1,1 +1,1 @@\n\
            -a\n\
            +A\n";
        let files = parse_diff(input).unwrap();
        assert_eq!(files[0].change_type, ChangeType::Rename);
        assert_eq!(files[0].path, "new");
        assert_eq!(files[0].renamed_from.as_deref(), Some("old"));
    }

    #[test]
    fn rejects_no_diff_block_on_empty_input() {
        let err = parse_diff("").unwrap_err();
        assert!(matches!(err, ParseError::NoDiffBlock), "got {err:?}");
    }

    #[test]
    fn rejects_no_diff_block_on_prose_only() {
        let err = parse_diff("hello, no diff here").unwrap_err();
        assert!(matches!(err, ParseError::NoDiffBlock), "got {err:?}");
    }

    #[test]
    fn rejects_headers_without_hunks() {
        let input = "--- a/x\n+++ b/x\n";
        let err = parse_diff(input).unwrap_err();
        assert!(matches!(err, ParseError::NoHunks { .. }), "got {err:?}");
    }

    #[test]
    fn rejects_malformed_hunk_header() {
        let input = "--- a/x\n+++ b/x\n@@ this is not a hunk header @@\n";
        let err = parse_diff(input).unwrap_err();
        assert!(matches!(err, ParseError::Malformed { .. }), "got {err:?}");
    }

    #[test]
    fn rejects_plus_before_minus() {
        let input = "+++ b/x\n--- a/x\n@@ -1,1 +1,1 @@\n";
        let err = parse_diff(input).unwrap_err();
        assert!(matches!(err, ParseError::Malformed { .. }), "got {err:?}");
    }

    #[test]
    fn extracts_fenced_diff_block() {
        let input = "Sure, here you go:\n\n```diff\n--- a/x\n+++ b/x\n@@ -1,1 +1,1 @@\n-a\n+A\n```\n\nLet me know!";
        let files = parse_diff(input).unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, "x");
    }

    #[test]
    fn extracts_patch_fenced_block() {
        let input = "```patch\n--- a/y\n+++ b/y\n@@ -1,1 +1,1 @@\n-a\n+b\n```";
        let files = parse_diff(input).unwrap();
        assert_eq!(files[0].path, "y");
    }

    #[test]
    fn accepts_untagged_fence_when_body_starts_with_headers() {
        let input = "```\n--- a/z\n+++ b/z\n@@ -1,1 +1,1 @@\n-a\n+b\n```";
        let files = parse_diff(input).unwrap();
        assert_eq!(files[0].path, "z");
    }

    #[test]
    fn strips_a_b_prefix_with_no_subdir() {
        let input = "--- a/foo\n+++ b/foo\n@@ -1,1 +1,1 @@\n-x\n+y\n";
        let files = parse_diff(input).unwrap();
        assert_eq!(files[0].path, "foo");
    }

    #[test]
    fn accepts_headers_without_a_b_prefix() {
        let input = "--- src/x.rs\n+++ src/x.rs\n@@ -1,1 +1,1 @@\n-a\n+A\n";
        let files = parse_diff(input).unwrap();
        assert_eq!(files[0].path, "src/x.rs");
    }

    #[test]
    fn trims_tab_separated_timestamp_from_header() {
        let input = "--- a/x.rs\t2026-05-13 18:00:00\n+++ b/x.rs\t2026-05-13 18:00:01\n@@ -1,1 +1,1 @@\n-a\n+A\n";
        let files = parse_diff(input).unwrap();
        assert_eq!(files[0].path, "x.rs");
    }

    #[test]
    fn rejects_eof_after_minus_header() {
        let input = "--- a/x\n";
        let err = parse_diff(input).unwrap_err();
        assert!(matches!(err, ParseError::Malformed { .. }), "got {err:?}");
    }
}
