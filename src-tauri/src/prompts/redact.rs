//! Content-pattern redactor for prompt-read attachments.
//!
//! Matches the patterns documented in `docs/SAFETY.md § Secret
//! handling` against UTF-8 text and replaces each match with a
//! `[REDACTED:<kind>]` marker before the content reaches the model.
//!
//! Hand-rolled — no `regex` crate — so this module adds zero new
//! dependencies. The patterns are deliberately narrow (high
//! precision, low recall): we'd rather miss a borderline case than
//! mangle innocuous identifiers. False negatives are tracked as a
//! roadmap concern in `docs/SAFETY.md`.
//!
//! Patterns shipped in D8:
//!
//! | Kind         | Shape                                                                  |
//! | ------------ | ---------------------------------------------------------------------- |
//! | `aws-key`    | `AKIA` followed by exactly 16 chars `[A-Z0-9]`                         |
//! | `github-pat` | `ghp_` followed by ≥ 36 chars `[A-Za-z0-9]`, or `github_pat_` + ≥ 20   |
//! | `api-key`    | `sk-` followed by ≥ 20 chars `[A-Za-z0-9_\-]` (OpenAI / Anthropic)     |
//! | `jwt`        | three base64url segments separated by `.`, first starts with `eyJ`     |
//! | `bearer`     | case-insensitive `Bearer ` (or `Bearer\t`) + token chars               |
//!
//! Connection strings with embedded passwords are intentionally
//! deferred — they need a URL parser to avoid mangling `https://`
//! and similar. Documented in `docs/SAFETY.md § Secret handling`.

use serde::Serialize;

/// One redaction applied to the content. The frontend never sees
/// these directly — they ride along with `RedactedContent` so the
/// assembler can attach a count to the model-facing prompt ("3
/// values redacted") and tests can assert on what got caught.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RedactionSpan {
    /// Stable identifier for the matched pattern. Kept lower-kebab so
    /// it stays human-readable inside the marker text.
    pub kind: &'static str,
    /// Byte offset into the ORIGINAL pre-redaction string. UTF-8 safe
    /// because every pattern matched here is ASCII.
    pub start: usize,
    /// Byte length of the matched span in the original string.
    pub len: usize,
}

/// Redact every documented secret pattern from `raw`. Returns the
/// redacted text and the list of spans that were replaced.
///
/// The output text is not the original with characters x'd out —
/// each match is replaced with `[REDACTED:<kind>]` so the model can
/// see "something was here" without knowing the original length.
/// Length-preserving redaction would tell a savvy reader exactly how
/// many characters the secret had, which is not useful and slightly
/// leaky.
pub fn redact(raw: &str) -> (String, Vec<RedactionSpan>) {
    // Collect all matches across all patterns first, then sort by
    // start offset and stitch the output. That way overlapping
    // matches resolve deterministically (longest-first wins at the
    // same start) without a complex regex-engine alternation.
    let mut matches: Vec<RedactionSpan> = Vec::new();
    collect_aws(raw, &mut matches);
    collect_github(raw, &mut matches);
    collect_api_key(raw, &mut matches);
    collect_jwt(raw, &mut matches);
    collect_bearer(raw, &mut matches);

    if matches.is_empty() {
        return (raw.to_string(), matches);
    }

    // Sort by start ascending, then by length descending so a longer
    // match at the same start wins. After sorting, prune overlaps.
    matches.sort_by(|a, b| a.start.cmp(&b.start).then(b.len.cmp(&a.len)));
    let mut kept: Vec<RedactionSpan> = Vec::with_capacity(matches.len());
    for m in matches {
        if let Some(last) = kept.last() {
            if m.start < last.start + last.len {
                continue; // overlaps a longer match already kept
            }
        }
        kept.push(m);
    }

    let mut out = String::with_capacity(raw.len());
    let bytes = raw.as_bytes();
    let mut cursor = 0usize;
    for span in &kept {
        if span.start > cursor {
            // Safe: every pattern matches ASCII, so the gaps between
            // them stay valid UTF-8 boundaries.
            out.push_str(
                std::str::from_utf8(&bytes[cursor..span.start])
                    .expect("redactor preserves utf-8 boundaries"),
            );
        }
        out.push_str(&format!("[REDACTED:{}]", span.kind));
        cursor = span.start + span.len;
    }
    if cursor < bytes.len() {
        out.push_str(
            std::str::from_utf8(&bytes[cursor..]).expect("redactor preserves utf-8 boundaries"),
        );
    }
    (out, kept)
}

/// `AKIA` + 16 chars `[A-Z0-9]`. AWS access key IDs are exactly 20
/// chars; we require the 4-char prefix and 16-char body.
fn collect_aws(raw: &str, out: &mut Vec<RedactionSpan>) {
    let bytes = raw.as_bytes();
    let needle = b"AKIA";
    let mut i = 0;
    while i + 20 <= bytes.len() {
        if &bytes[i..i + 4] == needle {
            let body = &bytes[i + 4..i + 20];
            if body
                .iter()
                .all(|&b| b.is_ascii_uppercase() || b.is_ascii_digit())
            {
                // Don't match if the next char is also alphanumeric — that
                // would be a longer identifier, not an AWS key.
                let next_is_alnum = bytes
                    .get(i + 20)
                    .map(|b| b.is_ascii_alphanumeric())
                    .unwrap_or(false);
                if !next_is_alnum {
                    out.push(RedactionSpan {
                        kind: "aws-key",
                        start: i,
                        len: 20,
                    });
                    i += 20;
                    continue;
                }
            }
        }
        i += 1;
    }
}

/// `ghp_` + ≥ 36 chars `[A-Za-z0-9]`, or `github_pat_` + ≥ 20 chars
/// `[A-Za-z0-9_]`.
fn collect_github(raw: &str, out: &mut Vec<RedactionSpan>) {
    for_each_prefix(raw, b"ghp_", 36, false, |start, len| {
        out.push(RedactionSpan {
            kind: "github-pat",
            start,
            len,
        });
    });
    for_each_prefix(raw, b"github_pat_", 20, true, |start, len| {
        out.push(RedactionSpan {
            kind: "github-pat",
            start,
            len,
        });
    });
}

/// `sk-` + ≥ 20 chars `[A-Za-z0-9_\-]`. Covers OpenAI (`sk-…`) and
/// Anthropic (`sk-ant-…`) shapes. The 20-char floor avoids matching
/// stylistic identifiers like `sk-button` or `sk-12`.
fn collect_api_key(raw: &str, out: &mut Vec<RedactionSpan>) {
    let bytes = raw.as_bytes();
    let needle = b"sk-";
    let mut i = 0;
    while i + needle.len() + 20 <= bytes.len() {
        if &bytes[i..i + needle.len()] == needle {
            // Body must not start at a position preceded by an
            // identifier char — otherwise we'd match `risk-…`.
            if i > 0 && bytes[i - 1].is_ascii_alphanumeric() {
                i += 1;
                continue;
            }
            let mut end = i + needle.len();
            while end < bytes.len() && is_api_key_char(bytes[end]) {
                end += 1;
            }
            let body_len = end - (i + needle.len());
            if body_len >= 20 {
                out.push(RedactionSpan {
                    kind: "api-key",
                    start: i,
                    len: end - i,
                });
                i = end;
                continue;
            }
        }
        i += 1;
    }
}

fn is_api_key_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'-' || b == b'_'
}

/// Three base64url segments separated by `.`. First segment starts
/// with `eyJ` (the base64url of `{"`), the others are any base64url
/// chars (≥ 1 each).
fn collect_jwt(raw: &str, out: &mut Vec<RedactionSpan>) {
    let bytes = raw.as_bytes();
    let prefix = b"eyJ";
    let mut i = 0;
    while i + prefix.len() < bytes.len() {
        if &bytes[i..i + prefix.len()] == prefix {
            // Guard against this being the tail of a longer word.
            if i > 0 && (bytes[i - 1].is_ascii_alphanumeric() || bytes[i - 1] == b'_') {
                i += 1;
                continue;
            }
            // First segment.
            let mut p = i + prefix.len();
            while p < bytes.len() && is_b64url_char(bytes[p]) {
                p += 1;
            }
            if p >= bytes.len() || bytes[p] != b'.' || p == i + prefix.len() {
                i += 1;
                continue;
            }
            let dot1 = p;
            // Second segment.
            p += 1;
            // Second segment also normally begins `eyJ`; we don't
            // require that — some JWTs have a non-{ payload.
            let seg2_start = p;
            while p < bytes.len() && is_b64url_char(bytes[p]) {
                p += 1;
            }
            if p == seg2_start || p >= bytes.len() || bytes[p] != b'.' {
                i = dot1 + 1;
                continue;
            }
            // Third segment.
            p += 1;
            let seg3_start = p;
            while p < bytes.len() && is_b64url_char(bytes[p]) {
                p += 1;
            }
            if p == seg3_start {
                i = dot1 + 1;
                continue;
            }
            out.push(RedactionSpan {
                kind: "jwt",
                start: i,
                len: p - i,
            });
            i = p;
            continue;
        }
        i += 1;
    }
}

fn is_b64url_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'-' || b == b'_'
}

/// `Bearer<space-or-tab>` + token chars. Token chars are RFC 6750's
/// `b64token` superset (alpha, digit, `-`, `.`, `_`, `~`, `+`, `/`,
/// optional trailing `=`). Match case-insensitively because real
/// fixtures contain `bearer`, `Bearer`, and `BEARER`.
fn collect_bearer(raw: &str, out: &mut Vec<RedactionSpan>) {
    let bytes = raw.as_bytes();
    let mut i = 0;
    while i + 7 <= bytes.len() {
        // Match `Bearer` ignoring case + a single space/tab separator.
        let head = &bytes[i..i + 6];
        let is_bearer = head[0].eq_ignore_ascii_case(&b'B')
            && head[1].eq_ignore_ascii_case(&b'e')
            && head[2].eq_ignore_ascii_case(&b'a')
            && head[3].eq_ignore_ascii_case(&b'r')
            && head[4].eq_ignore_ascii_case(&b'e')
            && head[5].eq_ignore_ascii_case(&b'r');
        if !is_bearer || (bytes[i + 6] != b' ' && bytes[i + 6] != b'\t') {
            i += 1;
            continue;
        }
        // Boundary guard: don't match `disbearer foo` etc.
        if i > 0 && (bytes[i - 1].is_ascii_alphanumeric() || bytes[i - 1] == b'_') {
            i += 1;
            continue;
        }
        let mut p = i + 7;
        let token_start = p;
        while p < bytes.len() && is_bearer_token_char(bytes[p]) {
            p += 1;
        }
        // Trailing padding `=` chars are allowed.
        while p < bytes.len() && bytes[p] == b'=' {
            p += 1;
        }
        if p >= token_start + 8 {
            // ≥ 8 token chars is a useful threshold — short headers
            // like `Bearer abc` are almost certainly placeholders.
            // The comparison is inclusive so an exactly-8-char token
            // still trips the redactor; the doc and the code agree.
            out.push(RedactionSpan {
                kind: "bearer",
                start: i,
                len: p - i,
            });
            i = p;
            continue;
        }
        i += 1;
    }
}

fn is_bearer_token_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~' | b'+' | b'/')
}

/// Walk every position where `raw` starts with `prefix`, scan the
/// trailing identifier-char run, and emit a match if it meets
/// `min_body`. Used by both the `ghp_` and `github_pat_` variants of
/// the GitHub PAT shape.
fn for_each_prefix<F>(
    raw: &str,
    prefix: &[u8],
    min_body: usize,
    allow_underscore: bool,
    mut emit: F,
) where
    F: FnMut(usize, usize),
{
    let bytes = raw.as_bytes();
    let mut i = 0;
    while i + prefix.len() + min_body <= bytes.len() {
        if &bytes[i..i + prefix.len()] == prefix {
            // Don't fold a longer identifier like `xyzghp_…`.
            if i > 0 && (bytes[i - 1].is_ascii_alphanumeric() || bytes[i - 1] == b'_') {
                i += 1;
                continue;
            }
            let mut end = i + prefix.len();
            while end < bytes.len()
                && (bytes[end].is_ascii_alphanumeric() || (allow_underscore && bytes[end] == b'_'))
            {
                end += 1;
            }
            let body_len = end - (i + prefix.len());
            if body_len >= min_body {
                emit(i, end - i);
                i = end;
                continue;
            }
        }
        i += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn redact_kinds(raw: &str) -> Vec<&'static str> {
        let (_, spans) = redact(raw);
        spans.iter().map(|s| s.kind).collect()
    }

    #[test]
    fn unchanged_when_no_secrets() {
        let raw = "fn main() {\n  println!(\"hello\");\n}\n";
        let (out, spans) = redact(raw);
        assert_eq!(out, raw);
        assert!(spans.is_empty());
    }

    #[test]
    fn redacts_aws_access_key_id() {
        let raw = "AKIAIOSFODNN7EXAMPLE rest";
        let (out, spans) = redact(raw);
        assert_eq!(out, "[REDACTED:aws-key] rest");
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].kind, "aws-key");
        assert_eq!(spans[0].start, 0);
        assert_eq!(spans[0].len, 20);
    }

    #[test]
    fn does_not_match_short_or_lowercase_aws_lookalike() {
        // 19 chars after AKIA — not enough.
        let (out, spans) = redact("AKIAIOSFODNN7EXAMPL more");
        assert_eq!(out, "AKIAIOSFODNN7EXAMPL more");
        assert!(spans.is_empty());
        // lowercase characters in the body — AWS keys are all caps.
        let (out, _) = redact("AKIAiosfodnn7example more");
        assert_eq!(out, "AKIAiosfodnn7example more");
    }

    #[test]
    fn redacts_github_classic_pat() {
        let raw = "token: ghp_AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA done";
        let (_, spans) = redact(raw);
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].kind, "github-pat");
    }

    #[test]
    fn redacts_github_fine_grained_pat() {
        let raw = "GITHUB_TOKEN=github_pat_11ABCDEF000_abcdefghijk1234567 fin";
        let kinds = redact_kinds(raw);
        assert_eq!(kinds, vec!["github-pat"]);
    }

    #[test]
    fn redacts_openai_style_key() {
        // Fixed-fake fixture: this is exactly the kind of literal the
        // redactor exists to catch. The `gitleaks:allow` marker tells
        // the pre-commit secret scanner that it's a deliberate test
        // input, not an accidentally-committed key.
        let raw = "OPENAI_API_KEY=sk-1234567890abcdef1234567890abcdef"; // gitleaks:allow
        let kinds = redact_kinds(raw);
        assert_eq!(kinds, vec!["api-key"]);
    }

    #[test]
    fn redacts_anthropic_style_key() {
        // `sk-ant-…` matches the same generic `sk-` rule.
        let raw = "ANTHROPIC_API_KEY=sk-ant-api03-AAAAAAAAAAAAAAAAAAAAAAA";
        let kinds = redact_kinds(raw);
        assert_eq!(kinds, vec!["api-key"]);
    }

    #[test]
    fn does_not_match_sk_prefix_inside_word() {
        // `risk-12345…` looks like `sk-` after `ri` but the boundary
        // check rejects it.
        let raw = "risk-1234567890abcdef1234567890abcdef tail";
        let kinds = redact_kinds(raw);
        assert!(kinds.is_empty());
    }

    #[test]
    fn redacts_jwt_triplet() {
        // Minimal three-segment JWT — actual content irrelevant.
        let raw =
            "Authorization: eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiJ4In0.aZbY1234_signaturepart and more";
        let kinds = redact_kinds(raw);
        // Bearer prefix is missing here so only the jwt matcher fires.
        assert_eq!(kinds, vec!["jwt"]);
    }

    #[test]
    fn redacts_bearer_header() {
        let raw = "Authorization: Bearer abc.def-ghi_jkl12345 more";
        let kinds = redact_kinds(raw);
        assert_eq!(kinds, vec!["bearer"]);
    }

    #[test]
    fn bearer_match_is_case_insensitive() {
        let raw = "auth: bearer aaaaaaaa-bbbb-cccc-dddd-eeeeeeee end";
        let kinds = redact_kinds(raw);
        assert_eq!(kinds, vec!["bearer"]);
    }

    #[test]
    fn bearer_redacts_at_minimum_threshold() {
        // Exactly 8 token chars is the documented floor — the
        // comment in `collect_bearer` says "≥ 8", and a P3 review
        // caught code that used `>` so 8 fell through. This test
        // pins the boundary so the doc/code stay aligned.
        let raw = "Authorization: Bearer abcd1234 trailing";
        let kinds = redact_kinds(raw);
        assert_eq!(kinds, vec!["bearer"]);
    }

    #[test]
    fn bearer_does_not_redact_below_threshold() {
        // 7 token chars — below the floor; the matcher should
        // back off and the placeholder stays untouched.
        let raw = "Authorization: Bearer abc1234 trailing";
        let kinds = redact_kinds(raw);
        assert!(kinds.is_empty(), "got unexpected redactions: {kinds:?}");
    }

    #[test]
    fn overlapping_bearer_and_jwt_kept_once() {
        // Bearer wins because it starts earlier; the JWT inside
        // would overlap.
        let raw = "Bearer eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiJ4In0.aZbY1234_signaturepart end";
        let kinds = redact_kinds(raw);
        // Bearer match at position 0 already covers the entire
        // header line; jwt suppressed by the overlap pruning.
        assert_eq!(kinds, vec!["bearer"]);
    }

    #[test]
    fn output_replaces_match_with_marker() {
        let raw = "before AKIAIOSFODNN7EXAMPLE after";
        let (out, _) = redact(raw);
        assert!(out.starts_with("before "));
        assert!(out.contains("[REDACTED:aws-key]"));
        assert!(out.ends_with(" after"));
    }

    #[test]
    fn spans_are_in_original_offsets() {
        // Verify a span's (start, len) refers to the ORIGINAL string,
        // not the redacted output. Tests / audit code use this.
        let raw = "id: AKIAIOSFODNN7EXAMPLE; key: ghp_AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
        let (_, spans) = redact(raw);
        assert_eq!(spans.len(), 2);
        let key0 = &raw[spans[0].start..spans[0].start + spans[0].len];
        assert_eq!(key0, "AKIAIOSFODNN7EXAMPLE");
        let key1 = &raw[spans[1].start..spans[1].start + spans[1].len];
        assert!(key1.starts_with("ghp_"));
    }
}
