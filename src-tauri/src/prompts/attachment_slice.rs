//! Line-range slicing for redacted project-file prompt attachments.

use super::assemble::LineRange;

/// Slice a 1-based inclusive range without silently clamping it.
/// Splitting only on `\n` preserves CRLF bytes, and the returned
/// slice ends in a newline so the attachment delimiter stays separate.
pub(super) fn slice_lines(content: &str, range: LineRange) -> Result<String, String> {
    debug_assert!(range.start >= 1, "start must be 1-based");
    debug_assert!(range.end >= range.start, "end must be >= start");

    let parts: Vec<&str> = content.split('\n').collect();
    let line_count = if parts.last().is_some_and(|s| s.is_empty()) && parts.len() > 1 {
        parts.len() - 1
    } else {
        parts.len()
    };

    let start = range.start as usize;
    let end = range.end as usize;
    if start > line_count {
        return Err(format!(
            "startLine {start} is past the file's last line ({line_count})"
        ));
    }
    if end > line_count {
        return Err(format!(
            "endLine {end} is past the file's last line ({line_count})"
        ));
    }
    let mut out = parts[(start - 1)..end].join("\n");
    if !out.ends_with('\n') {
        out.push('\n');
    }
    Ok(out)
}
