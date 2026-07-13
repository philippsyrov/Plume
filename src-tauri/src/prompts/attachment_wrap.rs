use super::LineRange;

pub(super) fn wrap_with_attachment(
    rel_path: &str,
    content: &str,
    applied_range: Option<LineRange>,
    user_instruction: &str,
) -> String {
    let mut out = String::with_capacity(content.len() + user_instruction.len() + 200);
    out.push_str("Attached file (read-only context): ");
    out.push_str(rel_path);
    if let Some(range) = applied_range {
        // Format "lines N–M" for a multi-line range, "line N" for
        // a single-line one. Using an en-dash here (instead of a
        // hyphen) keeps the label visually distinct from the
        // path; the model handles either fine.
        if range.start == range.end {
            out.push_str(&format!(" (line {})", range.start));
        } else {
            out.push_str(&format!(" (lines {}\u{2013}{})", range.start, range.end));
        }
    }
    out.push_str("\n\n----- FILE BEGIN -----\n");
    out.push_str(content);
    if !content.ends_with('\n') {
        out.push('\n');
    }
    out.push_str("----- FILE END -----\n\n");
    out.push_str(user_instruction);
    out
}
