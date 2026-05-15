//! D35 split: hunk-application helpers extracted from `apply.rs`.
//!
//! Two pure functions:
//!
//! * `apply_hunks_to` — walks the pre-image, splicing each hunk's
//!   `+`/` ` lines in place of its `-`/` ` lines, returns the
//!   post-image text. Pre-image and context mismatches surface as
//!   `preImageMismatch`-shaped failure details naming the hunk.
//! * `create_from_hunks` — builds a newly-created file from a
//!   create-diff's hunks. A `-` line inside a create-diff is
//!   malformed.
//!
//! Both functions are pure and synchronous — no filesystem, no
//! state. They live in their own file because `apply.rs` crossed
//! the decomposition cap; this is `D35`'s amber-cleanup
//! companion to the D33 checkpoint extraction. No behavior change.

use crate::patch::apply::PatchFailureDetail;
use crate::patch::parse::{HunkLine, ParsedHunk};

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
pub(crate) fn apply_hunks_to(
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
pub(crate) fn create_from_hunks(
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
