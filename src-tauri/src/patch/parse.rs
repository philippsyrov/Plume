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
//! and does not start a new group on its own. Hunks are tracked
//! via `@@` headers. D31 also captures the hunk BODIES so
//! `patch.apply` can re-verify the pre-image against disk and
//! compute a post-image — the `patch.validate` path still only
//! looks at counts and shape, so it ignores `ParsedHunk.lines`.

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
    /// Hunk count. Always equals `hunks.len()`. Retained alongside
    /// `hunks` so the validator's wire shape can carry it without
    /// allocating from `hunks` just to count.
    pub hunk_count: u32,
    /// Body of every hunk in this file group, in file order. The
    /// validator does not look inside; `patch.apply` consumes this
    /// for pre-image verification and post-image computation.
    pub hunks: Vec<ParsedHunk>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeType {
    Modify,
    Create,
    Delete,
    Rename,
}

/// One hunk inside a file group. Carries the declared line ranges
/// from the `@@` header AND the actual body lines so `patch.apply`
/// can re-verify the pre-image against disk before writing.
///
/// Line numbers are 1-based as in the unified-diff wire format; a
/// hunk header `@@ -3,2 +3,3 @@` produces `old_start = 3`,
/// `old_count = 2`, `new_start = 3`, `new_count = 3`. When a side
/// omits the count (`@@ -5 +5 @@`), the count defaults to 1 per
/// the unified-diff spec.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedHunk {
    pub old_start: u32,
    pub old_count: u32,
    pub new_start: u32,
    pub new_count: u32,
    pub lines: Vec<HunkLine>,
}

/// One body line inside a hunk. The string carries the content
/// stripped of the leading `+`, `-`, or ` ` marker.
///
/// `\ No newline at end of file` markers are intentionally dropped
/// at parse time — D31 does not round-trip the trailing-newline-flip
/// case (see `docs/PATCH_APPLY_DESIGN.md § Open questions`). The
/// applier preserves the pre-image's trailing-newline state for
/// modify-typed files and defaults to trailing-newline for created
/// files; a future slice can layer marker-aware handling.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HunkLine {
    Context(String),
    Add(String),
    Delete(String),
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
                hunks: Vec::new(),
                pending_hunk: None,
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
            // Commit any pending hunk before starting a new one.
            if let Some(pending) = file.pending_hunk.take() {
                file.hunks.push(pending.finish());
            }
            let range = parse_hunk_header(line, lineno)?;
            file.pending_hunk = Some(HunkBuilder::new(range));
            continue;
        }

        // Body line — only consumed when we're inside a hunk for
        // the current file group. Outside hunks (after a hunk
        // closes but before the next header) we tolerate prose /
        // metadata without strict checking.
        if let Some(file) = partial.as_mut() {
            if let Some(builder) = file.pending_hunk.as_mut() {
                if line.starts_with('\\') {
                    // No-newline-at-eof marker. D31 ignores it; a
                    // follow-up slice may handle the flip-newline-state
                    // case.
                    continue;
                }
                if line.is_empty() {
                    // Empty body line — treat as empty context.
                    // Some emitters strip trailing whitespace from
                    // a single-space context line; we cope.
                    builder.lines.push(HunkLine::Context(String::new()));
                    continue;
                }
                if let Some(content) = line.strip_prefix('+') {
                    builder.lines.push(HunkLine::Add(content.to_string()));
                } else if let Some(content) = line.strip_prefix('-') {
                    builder.lines.push(HunkLine::Delete(content.to_string()));
                } else if let Some(content) = line.strip_prefix(' ') {
                    builder.lines.push(HunkLine::Context(content.to_string()));
                } else {
                    // Naked line (no leading space/+/-/\). The
                    // strict unified-diff grammar requires the
                    // marker, but model output and Rust raw-string
                    // line continuations both strip whitespace,
                    // and `git diff` itself produces unmarked
                    // empty lines for some patches. Treat as
                    // context — if the content disagrees with the
                    // pre-image at apply time, the pre-image
                    // check surfaces a `preImageMismatch` against
                    // this exact hunk.
                    builder.lines.push(HunkLine::Context(line.to_string()));
                }
            }
            // Outside a hunk: tolerate metadata (`index`, `similarity`,
            // `old mode`, `new mode`, prose). No state change.
        }
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
    hunks: Vec<ParsedHunk>,
    pending_hunk: Option<HunkBuilder>,
    start_line: u32,
}

struct HunkBuilder {
    range: HunkRange,
    lines: Vec<HunkLine>,
}

impl HunkBuilder {
    fn new(range: HunkRange) -> Self {
        Self {
            range,
            lines: Vec::new(),
        }
    }
    fn finish(self) -> ParsedHunk {
        ParsedHunk {
            old_start: self.range.old_start,
            old_count: self.range.old_count,
            new_start: self.range.new_start,
            new_count: self.range.new_count,
            lines: self.lines,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct HunkRange {
    old_start: u32,
    old_count: u32,
    new_start: u32,
    new_count: u32,
}

fn commit_file(
    files: &mut Vec<ParsedFile>,
    partial: Option<PartialFile>,
    rename_from: &mut Option<String>,
    rename_to: &mut Option<String>,
) -> Result<(), ParseError> {
    let Some(mut p) = partial else {
        // No file was being assembled. Drop any stray rename
        // markers — they belonged to a `diff --git` that did not
        // produce a `--- /+++ ` pair.
        *rename_from = None;
        *rename_to = None;
        return Ok(());
    };
    // Commit any in-progress hunk before classifying the file.
    if let Some(pending) = p.pending_hunk.take() {
        p.hunks.push(pending.finish());
    }
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
    if p.hunks.is_empty() {
        return Err(ParseError::NoHunks {
            path: path.clone(),
            line: p.start_line,
        });
    }

    let hunk_count = p.hunks.len() as u32;
    files.push(ParsedFile {
        path,
        renamed_from: renamed_from_path,
        change_type,
        hunk_count,
        hunks: p.hunks,
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

/// Parse `@@ -<a>[,<b>] +<c>[,<d>] @@ [optional context]` into a
/// `HunkRange`. Validates shape AND captures the line numbers — the
/// apply path needs the numbers; the validate path only needs the
/// reject-on-malformed behaviour.
///
/// Each side parses strictly as one digit group or two digit
/// groups separated by a single comma — `parse_hunk_side("1")`
/// and `parse_hunk_side("1,3")` are the only accepted shapes.
/// Empty digit groups (`"-"`, `"1,"`, `",1"`) and extra commas
/// (`"1,,2"`, `"1,2,3"`) reject as `Malformed`. The D16 fix that
/// added that strictness is preserved verbatim.
fn parse_hunk_header(line: &str, lineno: u32) -> Result<HunkRange, ParseError> {
    let malformed = |line: &str| ParseError::Malformed {
        line: lineno,
        message: format!("hunk header malformed: '{}'", line),
    };
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
        return Err(malformed(line));
    }
    let minus_digits = parts[0].strip_prefix('-').ok_or_else(|| malformed(line))?;
    let plus_digits = parts[1].strip_prefix('+').ok_or_else(|| malformed(line))?;
    let (old_start, old_count) = parse_hunk_side(minus_digits).ok_or_else(|| malformed(line))?;
    let (new_start, new_count) = parse_hunk_side(plus_digits).ok_or_else(|| malformed(line))?;
    Ok(HunkRange {
        old_start,
        old_count,
        new_start,
        new_count,
    })
}

/// Accept exactly one or two non-empty digit groups separated by
/// a single comma. Returns `(start, count)` on success; the count
/// defaults to 1 when only a single group is present
/// (`@@ -5 +5 @@`), per the unified-diff spec.
fn parse_hunk_side(s: &str) -> Option<(u32, u32)> {
    if s.is_empty() {
        return None;
    }
    let segments: Vec<&str> = s.split(',').collect();
    if segments.len() > 2 {
        return None;
    }
    for seg in &segments {
        if seg.is_empty() || !seg.chars().all(|c| c.is_ascii_digit()) {
            return None;
        }
    }
    let start = segments[0].parse::<u32>().ok()?;
    let count = if segments.len() == 2 {
        segments[1].parse::<u32>().ok()?
    } else {
        1
    };
    Some((start, count))
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

    /// Regression for the D16 P3 finding: empty digit groups in
    /// the hunk range used to pass because
    /// `.all(digit-or-comma)` on an empty `&str` returns `true`.
    /// These headers now reject as `Malformed`.
    #[test]
    fn rejects_hunk_header_with_empty_digit_groups() {
        let cases = [
            "--- a/x\n+++ b/x\n@@ - + @@\n",
            "--- a/x\n+++ b/x\n@@ -, +1 @@\n",
            "--- a/x\n+++ b/x\n@@ -1, +1 @@\n",
            "--- a/x\n+++ b/x\n@@ -1 +, @@\n",
            "--- a/x\n+++ b/x\n@@ -1 +,1 @@\n",
        ];
        for input in cases {
            let err = parse_diff(input).unwrap_err();
            assert!(
                matches!(err, ParseError::Malformed { .. }),
                "expected Malformed for {input:?}, got {err:?}"
            );
        }
    }

    /// Regression for the D16 P3 finding: a hunk range with more
    /// than one comma (e.g. `-1,,2` or `-1,2,3`) used to pass
    /// because every character was still in `digit | ','`. Now
    /// rejected.
    #[test]
    fn rejects_hunk_header_with_multiple_commas() {
        let cases = [
            "--- a/x\n+++ b/x\n@@ -1,,2 +1,1 @@\n",
            "--- a/x\n+++ b/x\n@@ -1,2,3 +1,1 @@\n",
            "--- a/x\n+++ b/x\n@@ -1,1 +1,,2 @@\n",
        ];
        for input in cases {
            let err = parse_diff(input).unwrap_err();
            assert!(
                matches!(err, ParseError::Malformed { .. }),
                "expected Malformed for {input:?}, got {err:?}"
            );
        }
    }

    /// Pins the still-accepted happy forms — `-1 +1` and
    /// `-1,3 +1,3` — so the stricter parser doesn't accidentally
    /// reject canonical headers.
    #[test]
    fn accepts_canonical_hunk_ranges() {
        let single = "--- a/x\n+++ b/x\n@@ -1 +1 @@\n-a\n+A\n";
        let paired = "--- a/x\n+++ b/x\n@@ -1,3 +1,3 @@\n a\n-b\n+B\n c\n";
        let files_single = parse_diff(single).unwrap();
        let files_paired = parse_diff(paired).unwrap();
        assert_eq!(files_single[0].hunk_count, 1);
        assert_eq!(files_paired[0].hunk_count, 1);
    }

    #[test]
    fn rejects_hunk_header_with_non_digit_range() {
        let input = "--- a/x\n+++ b/x\n@@ -a +1 @@\n";
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

    // ----- D31: hunk-body capture -----
    //
    // The validator only counted `@@` headers; D31 adds full hunk
    // bodies so `patch.apply` can re-verify pre-image and compute
    // post-image text. Tests below pin the new shape.

    #[test]
    fn captures_hunk_body_lines_with_classification() {
        let files = parse_diff(HAPPY).unwrap();
        let hunk = &files[0].hunks[0];
        assert_eq!(hunk.lines.len(), 4);
        assert_eq!(hunk.lines[0], HunkLine::Context("a".to_string()));
        assert_eq!(hunk.lines[1], HunkLine::Delete("b".to_string()));
        assert_eq!(hunk.lines[2], HunkLine::Add("B".to_string()));
        assert_eq!(hunk.lines[3], HunkLine::Context("c".to_string()));
    }

    #[test]
    fn captures_hunk_line_ranges_from_header() {
        let input = "--- a/x\n\
            +++ b/x\n\
            @@ -3,2 +3,3 @@\n\
             a\n\
            -b\n\
            +B\n\
            +B2\n";
        let files = parse_diff(input).unwrap();
        let hunk = &files[0].hunks[0];
        assert_eq!(hunk.old_start, 3);
        assert_eq!(hunk.old_count, 2);
        assert_eq!(hunk.new_start, 3);
        assert_eq!(hunk.new_count, 3);
    }

    #[test]
    fn single_digit_header_defaults_count_to_one() {
        let input = "--- a/x\n+++ b/x\n@@ -5 +5 @@\n-a\n+A\n";
        let files = parse_diff(input).unwrap();
        let hunk = &files[0].hunks[0];
        assert_eq!(hunk.old_start, 5);
        assert_eq!(hunk.old_count, 1);
        assert_eq!(hunk.new_start, 5);
        assert_eq!(hunk.new_count, 1);
    }

    #[test]
    fn captures_multiple_hunks_with_distinct_ranges() {
        let input = "--- a/x\n\
            +++ b/x\n\
            @@ -1,1 +1,1 @@\n\
            -a\n\
            +A\n\
            @@ -10,1 +10,1 @@\n\
            -b\n\
            +B\n";
        let files = parse_diff(input).unwrap();
        assert_eq!(files[0].hunks.len(), 2);
        assert_eq!(files[0].hunks[0].old_start, 1);
        assert_eq!(files[0].hunks[1].old_start, 10);
        assert_eq!(files[0].hunk_count, 2);
    }

    #[test]
    fn ignores_no_newline_at_eof_marker() {
        // The marker must not leak into `hunk.lines`. D31 documents
        // this as an intentional simplification — the applier
        // preserves the pre-image's trailing-newline state instead.
        let input = "--- a/x\n\
            +++ b/x\n\
            @@ -1,1 +1,1 @@\n\
            -a\n\
            \\ No newline at end of file\n\
            +A\n\
            \\ No newline at end of file\n";
        let files = parse_diff(input).unwrap();
        let hunk = &files[0].hunks[0];
        assert_eq!(hunk.lines.len(), 2);
        assert_eq!(hunk.lines[0], HunkLine::Delete("a".to_string()));
        assert_eq!(hunk.lines[1], HunkLine::Add("A".to_string()));
    }

    #[test]
    fn empty_context_line_is_preserved() {
        // A blank context line in the diff body should round-trip
        // as `Context("")`. Some emitters drop the leading space
        // entirely; both forms must parse.
        let input = "--- a/x\n+++ b/x\n@@ -1,3 +1,3 @@\n a\n\n c\n";
        let files = parse_diff(input).unwrap();
        let hunk = &files[0].hunks[0];
        assert_eq!(hunk.lines.len(), 3);
        assert_eq!(hunk.lines[1], HunkLine::Context(String::new()));
    }

    #[test]
    fn hunk_count_matches_hunks_len() {
        // Invariant: `hunk_count` stays in sync with `hunks.len()`.
        let input = "--- a/x\n\
            +++ b/x\n\
            @@ -1,1 +1,1 @@\n\
            -a\n\
            +A\n\
            @@ -10,1 +10,1 @@\n\
            -b\n\
            +B\n\
            @@ -20,1 +20,1 @@\n\
            -c\n\
            +C\n";
        let files = parse_diff(input).unwrap();
        assert_eq!(files[0].hunk_count as usize, files[0].hunks.len());
        assert_eq!(files[0].hunks.len(), 3);
    }
}
