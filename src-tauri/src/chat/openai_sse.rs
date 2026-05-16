//! Provider-neutral parser for OpenAI-style `text/event-stream` chat
//! completion streams. Pure, synchronous, line-driven: callers feed
//! one wire line per call, the parser classifies it. No HTTP, no
//! async, no Tauri events — that wiring lives in the runtime slice
//! (D40+) that consumes this helper.
//!
//! The module-level `#[allow(dead_code)]` below is deliberate: D39
//! ships only the helper + tests; no production caller exists yet,
//! so clippy's `dead_code` lint would fire on every public item.
//! The first consumer (the chat-runtime slice that drives MLX-LM
//! SSE responses) will remove the gate by referencing the API.
#![allow(dead_code)]
//!
//! Used by the upcoming MLX-LM runtime and any other OpenAI-
//! compatible chat path that streams `/v1/chat/completions` over SSE
//! (LM Studio, llama-server, vLLM, MLX-LM). Ollama uses NDJSON, not
//! SSE, and stays in `chat/ollama/streaming.rs` unchanged.
//!
//! ## SSE wire shape (W3C EventSource subset)
//!
//! Lines arrive LF- or CRLF-delimited; callers strip the LF, and
//! `parse_line` strips an optional trailing `\r` to tolerate CRLF
//! clients. The grammar this helper understands:
//!
//! - lines starting with `:` are comments / keepalives — ignored
//! - lines starting with `data:` carry a JSON payload OR the literal
//!   `[DONE]` terminator
//! - empty lines are event boundaries — ignored here because every
//!   OpenAI chat-completion frame is one `data: …` line followed by
//!   an empty line, so we don't need to buffer multi-line `data:`
//! - other SSE fields (`event:`, `id:`, `retry:`) are accepted but
//!   ignored — chat-completions streams don't use them
//!
//! ## OpenAI chat-completions chunk shape
//!
//! A streamed chunk looks like
//!
//! ```json
//! {
//!   "id": "chatcmpl-…",
//!   "object": "chat.completion.chunk",
//!   "choices": [
//!     {
//!       "index": 0,
//!       "delta": { "content": "Hello" },
//!       "finish_reason": null
//!     }
//!   ],
//!   "usage": null
//! }
//! ```
//!
//! Frame zero on most servers carries `delta: {"role":"assistant"}`
//! with no `content`; the parser surfaces that as a
//! `Delta { content: None, .. }` so the caller can ignore. The
//! terminal content frame carries `finish_reason: "stop"` (or
//! `"length"`).
//!
//! If the server was launched with `stream_options.include_usage =
//! true` (vLLM, llama-server, MLX-LM with the right flag), an EXTRA
//! trailing chunk with `choices: []` and a populated `usage` arrives
//! after the stop chunk and before `data: [DONE]`. Some servers
//! INLINE `usage` on the same chunk as `finish_reason: "stop"`. To
//! tolerate both shapes, `parse_line` returns `Vec<SseEvent>`: an
//! inlined frame emits both `Delta` and `Usage`, a separate trailing
//! usage chunk emits only `Usage`, and a normal content chunk emits
//! only `Delta`. Allocation cost is one to two small enum values per
//! frame, which is negligible against the JSON parse already done.

use serde::Deserialize;

/// Classified SSE event from a single chunk line. One wire line can
/// produce zero, one, or two events; see module docs for why two.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SseEvent {
    /// One delta from `choices[0]`. `content` is `None` when the
    /// chunk carried only `delta: {"role": …}` or an empty delta;
    /// `finish_reason` is `Some` on the terminal content chunk.
    Delta {
        content: Option<String>,
        finish_reason: Option<String>,
    },
    /// Generation telemetry. Emitted either inlined with the stop
    /// chunk or as a separate trailing chunk depending on the
    /// runtime. The caller should treat the LAST one it sees as
    /// authoritative.
    Usage(SseUsage),
    /// `data: [DONE]` sentinel. Caller can stop reading after this.
    Done,
}

/// OpenAI-shape usage telemetry. All fields are `Option<u64>` so a
/// server that reports only a subset (or omits usage entirely on
/// `include_usage = false`) doesn't have to lie about zero.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SseUsage {
    pub prompt_tokens: Option<u64>,
    pub completion_tokens: Option<u64>,
    pub total_tokens: Option<u64>,
}

/// Why a frame couldn't be parsed. The line number is 1-based and
/// counts every line the parser has seen on this stream — comments
/// and empty boundaries included — so a failure address matches
/// what an operator would count in a logged transcript.
#[derive(Debug)]
pub enum SseParseError {
    /// `data: …` line whose payload wasn't valid JSON.
    InvalidJson {
        line_no: usize,
        payload: String,
        source: serde_json::Error,
    },
    /// `data: …` payload was valid JSON but had no recognizable
    /// chunk shape — no `choices`, no `usage`. We surface this so the
    /// caller can decide whether to terminate or keep reading.
    UnknownChunk { line_no: usize, payload: String },
}

impl std::fmt::Display for SseParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SseParseError::InvalidJson {
                line_no, source, ..
            } => write!(f, "invalid JSON on line {line_no}: {source}"),
            SseParseError::UnknownChunk { line_no, .. } => {
                write!(f, "unknown chunk shape on line {line_no}")
            }
        }
    }
}

impl std::error::Error for SseParseError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            SseParseError::InvalidJson { source, .. } => Some(source),
            SseParseError::UnknownChunk { .. } => None,
        }
    }
}

/// Line-driven OpenAI SSE parser. Construct one per stream; feed
/// each wire line with `parse_line`.
pub struct SseParser {
    line_no: usize,
}

impl SseParser {
    pub fn new() -> Self {
        Self { line_no: 0 }
    }

    /// 1-based index of the most recently consumed line. Useful when
    /// formatting downstream errors that reference a stream
    /// transcript.
    pub fn line_no(&self) -> usize {
        self.line_no
    }

    /// Consume one wire line and classify it. The caller strips the
    /// trailing LF; an optional `\r` (CRLF clients) is stripped here.
    ///
    /// Returns:
    /// - empty `Vec` for comments, empty boundaries, and ignored
    ///   SSE fields (`event:`, `id:`, `retry:`)
    /// - one `SseEvent` for a normal content chunk, a role-only
    ///   chunk, a usage-only chunk, or `[DONE]`
    /// - two `SseEvent`s when a stop chunk inlines `usage`
    pub fn parse_line(&mut self, line: &str) -> Result<Vec<SseEvent>, SseParseError> {
        self.line_no += 1;

        // Tolerate CRLF clients.
        let trimmed = line.strip_suffix('\r').unwrap_or(line);

        // Empty line — event boundary, ignored.
        if trimmed.is_empty() {
            return Ok(Vec::new());
        }

        // SSE comment / keepalive (`:` or `: ping`).
        if trimmed.starts_with(':') {
            return Ok(Vec::new());
        }

        // Only `data:` fields carry payloads we care about. Other
        // SSE fields (`event:`, `id:`, `retry:`) are accepted-but-
        // ignored. Lines without a colon at all (malformed SSE) also
        // fall through here as ignored.
        let payload = match trimmed.strip_prefix("data:") {
            Some(rest) => rest.strip_prefix(' ').unwrap_or(rest),
            None => return Ok(Vec::new()),
        };

        // OpenAI stream terminator.
        if payload == "[DONE]" {
            return Ok(vec![SseEvent::Done]);
        }

        // Parse the JSON chunk.
        let chunk: ChatChunk =
            serde_json::from_str(payload).map_err(|source| SseParseError::InvalidJson {
                line_no: self.line_no,
                payload: payload.to_string(),
                source,
            })?;

        let mut events = Vec::with_capacity(2);

        // `choices[0]` if present.
        if let Some(choice) = chunk.choices.as_ref().and_then(|c| c.first()) {
            let content = choice.delta.as_ref().and_then(|d| d.content.clone());
            let finish_reason = choice.finish_reason.clone();
            events.push(SseEvent::Delta {
                content,
                finish_reason,
            });
        }

        // `usage` — inlined alongside a delta on some servers,
        // standalone on others.
        if let Some(usage) = chunk.usage {
            events.push(SseEvent::Usage(SseUsage {
                prompt_tokens: usage.prompt_tokens,
                completion_tokens: usage.completion_tokens,
                total_tokens: usage.total_tokens,
            }));
        }

        if events.is_empty() {
            return Err(SseParseError::UnknownChunk {
                line_no: self.line_no,
                payload: payload.to_string(),
            });
        }

        Ok(events)
    }
}

impl Default for SseParser {
    fn default() -> Self {
        Self::new()
    }
}

// --- private wire types --------------------------------------------------
//
// Kept minimal: we deliberately do NOT model every OpenAI streaming
// field. `id`, `object`, `created`, `model`, `system_fingerprint` and
// the various choice extras (`logprobs`, `index`, etc) are skipped on
// purpose — surfacing them would be premature and force every
// upstream variant onto our wire. If a future caller needs one of
// these, add a typed accessor here rather than re-typing the chunk
// shape at the call site.

#[derive(Deserialize)]
struct ChatChunk {
    #[serde(default)]
    choices: Option<Vec<ChatChoice>>,
    #[serde(default)]
    usage: Option<ChatUsage>,
}

#[derive(Deserialize)]
struct ChatChoice {
    #[serde(default)]
    delta: Option<ChatDelta>,
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(Deserialize)]
struct ChatDelta {
    #[serde(default)]
    content: Option<String>,
}

#[derive(Deserialize)]
struct ChatUsage {
    #[serde(default)]
    prompt_tokens: Option<u64>,
    #[serde(default)]
    completion_tokens: Option<u64>,
    #[serde(default)]
    total_tokens: Option<u64>,
}

#[cfg(test)]
#[path = "openai_sse_tests.rs"]
mod tests;
