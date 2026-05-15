//! Tests for `patch::parse`. Split into a sibling file via
//! `#[path]` so the production module stays under the
//! decomposition cap. Same pattern as `apply_tests.rs` /
//! `revert_tests.rs`.

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
