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

    // No hunks: D16's original rule rejected every file group
    // that didn't carry at least one hunk, because a no-op modify
    // is meaningless. D33 relaxes that for pure renames: a model
    // may legitimately emit a rename with no body change (the
    // file's bytes are unchanged at the new path), and rejecting
    // those would force the model to fabricate a context hunk.
    // For every OTHER change type the strict rule still holds —
    // a hunkless create/modify/delete is still a parser malformity.
    if p.hunks.is_empty() && change_type != ChangeType::Rename {
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
#[path = "parse_tests.rs"]
mod tests;
