//! Tests for the OpenAI SSE parser. Pure unit tests — no HTTP, no
//! mocks. Each test consumes one or more wire lines through a fresh
//! `SseParser` and asserts the classified output.
//!
//! The realistic-transcript test at the bottom feeds an end-to-end
//! stream the way an upstream MLX-LM / vLLM / llama-server server
//! actually emits one, to make sure the parser composes correctly
//! and not just per-line.

use super::*;

fn parse_one(line: &str) -> Vec<SseEvent> {
    let mut p = SseParser::new();
    p.parse_line(line).expect("expected parse success")
}

// --- ignored lines -------------------------------------------------------

#[test]
fn empty_line_emits_nothing() {
    assert!(parse_one("").is_empty());
}

#[test]
fn comment_line_emits_nothing() {
    assert!(parse_one(":").is_empty());
    assert!(parse_one(": keepalive").is_empty());
    assert!(parse_one(":ping").is_empty());
}

#[test]
fn other_sse_fields_are_ignored() {
    // chat-completions streams don't use these, but we should still
    // tolerate them without erroring.
    assert!(parse_one("event: message").is_empty());
    assert!(parse_one("id: 12345").is_empty());
    assert!(parse_one("retry: 5000").is_empty());
    // Lines without a `data:` prefix or any colon at all.
    assert!(parse_one("garbage line with no prefix").is_empty());
}

#[test]
fn crlf_line_endings_are_tolerated() {
    // The caller is expected to strip the LF; we strip the optional
    // trailing CR to handle CRLF servers. Raw strings can't embed a
    // real CR, so we build the line via `format!` to ensure the `\r`
    // is the byte 0x0D, not an escaped backslash-r.
    assert!(parse_one(": keepalive\r").is_empty());
    let line = format!("data: {}\r", r#"{"choices":[{"delta":{"content":"hi"}}]}"#);
    let evs = parse_one(&line);
    assert_eq!(evs.len(), 1);
    assert!(matches!(
        &evs[0],
        SseEvent::Delta { content: Some(s), .. } if s == "hi"
    ));
}

// --- DONE sentinel -------------------------------------------------------

#[test]
fn done_marker_emits_done_event() {
    let evs = parse_one("data: [DONE]");
    assert_eq!(evs, vec![SseEvent::Done]);
}

#[test]
fn done_marker_without_space_after_colon_works() {
    // SSE spec allows the optional single space after `data:` to be
    // omitted — `data:[DONE]` is still a valid field.
    let evs = parse_one("data:[DONE]");
    assert_eq!(evs, vec![SseEvent::Done]);
}

// --- normal content frames ----------------------------------------------

#[test]
fn content_delta_is_extracted() {
    let evs =
        parse_one(r#"data: {"choices":[{"delta":{"content":"Hello"},"finish_reason":null}]}"#);
    assert_eq!(evs.len(), 1);
    assert_eq!(
        evs[0],
        SseEvent::Delta {
            content: Some("Hello".to_string()),
            finish_reason: None,
        }
    );
}

#[test]
fn role_only_first_frame_emits_content_none() {
    // The opening frame on every OpenAI stream looks like this.
    let evs =
        parse_one(r#"data: {"choices":[{"delta":{"role":"assistant"},"finish_reason":null}]}"#);
    assert_eq!(evs.len(), 1);
    assert_eq!(
        evs[0],
        SseEvent::Delta {
            content: None,
            finish_reason: None,
        }
    );
}

#[test]
fn stop_frame_carries_finish_reason() {
    let evs = parse_one(r#"data: {"choices":[{"delta":{},"finish_reason":"stop"}]}"#);
    assert_eq!(evs.len(), 1);
    assert_eq!(
        evs[0],
        SseEvent::Delta {
            content: None,
            finish_reason: Some("stop".to_string()),
        }
    );
}

#[test]
fn length_finish_reason_is_preserved_verbatim() {
    let evs =
        parse_one(r#"data: {"choices":[{"delta":{"content":"…"},"finish_reason":"length"}]}"#);
    assert_eq!(evs.len(), 1);
    let SseEvent::Delta {
        content,
        finish_reason,
    } = &evs[0]
    else {
        panic!("expected delta, got {:?}", evs[0]);
    };
    assert_eq!(content.as_deref(), Some("…"));
    assert_eq!(finish_reason.as_deref(), Some("length"));
}

#[test]
fn data_field_without_space_after_colon_parses() {
    // Some servers don't insert the space. The SSE spec permits both.
    let evs = parse_one(r#"data:{"choices":[{"delta":{"content":"x"}}]}"#);
    assert_eq!(evs.len(), 1);
    assert!(matches!(
        &evs[0],
        SseEvent::Delta { content: Some(s), .. } if s == "x"
    ));
}

// --- usage frames --------------------------------------------------------

#[test]
fn standalone_usage_chunk_is_extracted() {
    // `include_usage = true` shape on servers that emit usage as a
    // separate trailing chunk.
    let evs = parse_one(
        r#"data: {"choices":[],"usage":{"prompt_tokens":12,"completion_tokens":7,"total_tokens":19}}"#,
    );
    assert_eq!(evs.len(), 1);
    assert_eq!(
        evs[0],
        SseEvent::Usage(SseUsage {
            prompt_tokens: Some(12),
            completion_tokens: Some(7),
            total_tokens: Some(19),
        })
    );
}

#[test]
fn usage_inlined_with_stop_chunk_emits_both_events() {
    // vLLM / llama-server / MLX-LM inline shape: a single chunk
    // carries both the stop-finish delta and the usage block.
    let evs = parse_one(
        r#"data: {"choices":[{"delta":{},"finish_reason":"stop"}],"usage":{"prompt_tokens":10,"completion_tokens":5,"total_tokens":15}}"#,
    );
    assert_eq!(evs.len(), 2);
    assert!(matches!(
        &evs[0],
        SseEvent::Delta { content: None, finish_reason: Some(r) } if r == "stop"
    ));
    assert!(matches!(
        &evs[1],
        SseEvent::Usage(u) if u.prompt_tokens == Some(10)
            && u.completion_tokens == Some(5)
            && u.total_tokens == Some(15)
    ));
}

#[test]
fn usage_with_partial_fields_works() {
    // Server omits total_tokens — we surface the rest verbatim.
    let evs =
        parse_one(r#"data: {"choices":[],"usage":{"prompt_tokens":3,"completion_tokens":2}}"#);
    assert_eq!(evs.len(), 1);
    assert_eq!(
        evs[0],
        SseEvent::Usage(SseUsage {
            prompt_tokens: Some(3),
            completion_tokens: Some(2),
            total_tokens: None,
        })
    );
}

// --- error paths ---------------------------------------------------------

#[test]
fn invalid_json_returns_structured_error() {
    let mut p = SseParser::new();
    let err = p.parse_line("data: { not actually json").unwrap_err();
    match err {
        SseParseError::InvalidJson {
            line_no, payload, ..
        } => {
            assert_eq!(line_no, 1);
            assert_eq!(payload, "{ not actually json");
        }
        other => panic!("expected InvalidJson, got {other:?}"),
    }
}

#[test]
fn unknown_chunk_returns_structured_error() {
    // Valid JSON but no `choices` and no `usage` — nothing the
    // caller can do with this, so we surface it rather than
    // silently dropping the frame.
    let mut p = SseParser::new();
    let err = p
        .parse_line(r#"data: {"id":"chatcmpl-1","object":"chat.completion.chunk"}"#)
        .unwrap_err();
    match err {
        SseParseError::UnknownChunk { line_no, payload } => {
            assert_eq!(line_no, 1);
            assert!(payload.contains("chatcmpl-1"));
        }
        other => panic!("expected UnknownChunk, got {other:?}"),
    }
}

#[test]
fn error_line_numbers_match_the_full_stream_position() {
    // The parser counts every line it sees, including comments and
    // empty boundaries, so a logged failure addresses the same
    // line a transcript reader would count.
    let mut p = SseParser::new();
    assert!(p.parse_line(": keepalive").unwrap().is_empty()); // line 1
    assert!(p.parse_line("").unwrap().is_empty()); // line 2
    assert!(p.parse_line("event: message").unwrap().is_empty()); // line 3
    let err = p.parse_line("data: garbage").unwrap_err(); // line 4
    match err {
        SseParseError::InvalidJson { line_no, .. } => assert_eq!(line_no, 4),
        other => panic!("expected InvalidJson, got {other:?}"),
    }
    assert_eq!(p.line_no(), 4);
}

// --- realistic full-stream walkthrough ----------------------------------

#[test]
fn realistic_stream_with_keepalives_role_deltas_stop_usage_done() {
    // Mirrors what an upstream chat-completions stream actually looks
    // like with `include_usage = true` and a couple of keepalive
    // comments interleaved. The assertions read top-to-bottom so a
    // failure points at the exact frame that drifted.
    let lines = [
        ": keepalive",
        "",
        r#"data: {"choices":[{"index":0,"delta":{"role":"assistant"},"finish_reason":null}]}"#,
        "",
        r#"data: {"choices":[{"index":0,"delta":{"content":"Hello"},"finish_reason":null}]}"#,
        "",
        ":",
        r#"data: {"choices":[{"index":0,"delta":{"content":" world"},"finish_reason":null}]}"#,
        "",
        r#"data: {"choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}"#,
        "",
        r#"data: {"choices":[],"usage":{"prompt_tokens":8,"completion_tokens":2,"total_tokens":10}}"#,
        "",
        "data: [DONE]",
    ];

    let mut p = SseParser::new();
    let mut events: Vec<SseEvent> = Vec::new();
    for line in lines {
        let mut got = p.parse_line(line).expect("parse should succeed");
        events.append(&mut got);
    }

    assert_eq!(
        events,
        vec![
            SseEvent::Delta {
                content: None,
                finish_reason: None,
            },
            SseEvent::Delta {
                content: Some("Hello".to_string()),
                finish_reason: None,
            },
            SseEvent::Delta {
                content: Some(" world".to_string()),
                finish_reason: None,
            },
            SseEvent::Delta {
                content: None,
                finish_reason: Some("stop".to_string()),
            },
            SseEvent::Usage(SseUsage {
                prompt_tokens: Some(8),
                completion_tokens: Some(2),
                total_tokens: Some(10),
            }),
            SseEvent::Done,
        ]
    );
}

#[test]
fn parser_is_reusable_default_construction() {
    // The Default impl mirrors `new`; just exercise it so a refactor
    // that removes one and not the other gets caught.
    let mut p = SseParser::default();
    let evs = p.parse_line("data: [DONE]").unwrap();
    assert_eq!(evs, vec![SseEvent::Done]);
    assert_eq!(p.line_no(), 1);
}
