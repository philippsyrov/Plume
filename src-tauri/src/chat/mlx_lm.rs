//! D45: streaming chat adapter for Plume-managed MLX-LM servers.
//!
//! The shape mirrors `chat::ollama::stream_chat`: connect, send the
//! request, read response head, then drive a streaming-read loop that
//! emits per-delta tokens via a callback and returns a `StreamOutcome`
//! when the runtime signals end-of-stream. Two things differ:
//!
//!   * **Wire format.** MLX-LM speaks OpenAI `/v1/chat/completions`,
//!     not Ollama `/api/chat`. The streaming response is SSE
//!     (`text/event-stream`) instead of NDJSON. Each frame is one
//!     `data: <json>` line followed by an empty line; the stream
//!     terminates with `data: [DONE]`. D39's pure
//!     `chat::openai_sse::SseParser` classifies each wire line into
//!     `SseEvent::Delta | Usage | Done`; this adapter drives the
//!     parser from the HTTP read loop.
//!
//!   * **Telemetry.** OpenAI usage chunks only expose `prompt_tokens`
//!     and `completion_tokens`. They do NOT report per-phase
//!     durations (no `prompt_eval_duration`, no `eval_duration`), so
//!     the resulting `ChatStats` populates `prompt_tokens` and
//!     `output_tokens` but leaves `eval_ms`, `prompt_ms`, and
//!     `tokens_per_second` as `None`. The frontend's footer renderer
//!     already handles partial stats (hides the missing parts);
//!     fabricating a wall-clock fallback would be dishonest about
//!     what the runtime actually measured.
//!
//! HTTP framing notes:
//!
//!   * The request advertises `HTTP/1.1`, `Connection: close`, and
//!     `Accept: text/event-stream`. mlx-lm's Python `BaseHTTPRequestHandler`
//!     responds close-delimited (no `Transfer-Encoding: chunked`) so
//!     the read loop just consumes bytes until EOF. If a future
//!     mlx-lm version starts sending chunked responses, lines that
//!     look like hex chunk-size headers will land in the SSE parser
//!     and surface as `UnknownChunk` errors — that's the signal to
//!     add a chunked-transfer decoder layer here.
//!
//!   * The read loop polls a cancel `AtomicBool` and an overall
//!     deadline between line reads, identical to the Ollama
//!     streaming loop, so `chat.cancel` works without changes at the
//!     command layer.
//!
//! This module does NOT spawn or supervise the MLX-LM process —
//! that's `providers::mlx_lm::process` (D40). It assumes the caller
//! already looked up the bound port from a `ServerHandleId` and
//! passes it in.

use std::io::{self, BufReader, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::openai_sse::{SseEvent, SseParser};
use super::stream_read::{is_timeout_kind, read_line_bounded, ReadOutcome};
use super::{ChatMessage, ChatRole};

/// Same 200 ms poll window the Ollama adapter uses. Trades CPU vs
/// cancel-response latency the same way.
const STREAM_READ_POLL: Duration = Duration::from_millis(200);

/// The fixed Qwen catalog model uses ChatML and otherwise emits this control
/// marker as visible text before the server's stop frame.
pub const QWEN_CHAT_STOP_SEQUENCE: &str = "<|im_end|>";

/// MLX-LM-side outcome of `stream_chat`. Matches the shape of
/// `ollama::StreamOutcome` so the command layer can dispatch on
/// either adapter's result with the same `match`. The two differ
/// only in their concrete stats payload; we use `MlxFrameStats`
/// here and the Ollama adapter uses `OllamaFrameStats`.
#[derive(Debug, PartialEq, Eq)]
pub enum StreamOutcome {
    /// SSE stream terminated with `data: [DONE]`. Carries the model
    /// id the runtime reported (echoes the request id when the
    /// runtime didn't surface one — mlx-lm's chunk shape DOES
    /// include `"model": "..."` but it's the path passed to
    /// `--model`, which the caller already knows).
    Done {
        model_id: String,
        stats: MlxFrameStats,
    },
    /// Cancel flag tripped between line reads.
    Cancelled { model_id: Option<String> },
    /// Connection closed without `[DONE]`. Surfaced separately from
    /// `Done` so the command layer maps it to `ChatFinish::Length`
    /// rather than a clean stop.
    EofBeforeDone { model_id: Option<String> },
}

#[derive(Debug, PartialEq, Eq)]
#[allow(dead_code)] // The research run wiring lands after its bounded model port.
pub struct CollectedTurn {
    pub text: String,
    pub outcome: StreamOutcome,
}

/// OpenAI-shape usage payload, kept honest about what we did and
/// didn't observe. Field names mirror the SSE wire (`prompt_tokens`,
/// `completion_tokens`) but at the chat-layer boundary we use the
/// same neutral spelling Ollama uses (`output_tokens` is the
/// completion count).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct MlxFrameStats {
    pub prompt_tokens: Option<u64>,
    pub completion_tokens: Option<u64>,
}

/// Errors that can come out of `stream_chat`. Same envelope as
/// `chat::ollama::ChatError` so the command layer can map both with
/// a common formatter.
#[derive(Debug, thiserror::Error)]
pub enum ChatError {
    #[error("mlx-lm at 127.0.0.1:{port} did not answer: {source}")]
    Transport {
        port: u16,
        #[source]
        source: io::Error,
    },
    /// 404 from `/v1/chat/completions` — the most likely cause is
    /// the server having loaded a different model than the caller
    /// thought.
    #[error("mlx-lm reports model '{model}' not found: {message}")]
    ModelNotFound { model: String, message: String },
    /// Any other non-2xx status.
    #[error("mlx-lm returned HTTP {status}: {message}")]
    BadStatus { status: u16, message: String },
    /// SSE parser couldn't make sense of a frame.
    #[error("mlx-lm SSE response did not parse: {0}")]
    Parse(String),
}

/// Stream a chat completion from a localhost MLX-LM server. Mirrors
/// `chat::ollama::stream_chat`'s signature for parity at the command
/// layer; the only difference is the absence of a `host` parameter
/// (MLX is always 127.0.0.1 since the supervisor binds to localhost
/// explicitly).
#[allow(clippy::too_many_arguments)]
pub fn stream_chat<F>(
    port: u16,
    model: &str,
    messages: &[ChatMessage],
    cancel: Arc<AtomicBool>,
    on_delta: F,
    connect_timeout: Duration,
    overall_deadline: Instant,
) -> Result<StreamOutcome, ChatError>
where
    F: FnMut(&str),
{
    stream_chat_with_stop_sequences(
        port,
        model,
        messages,
        &[],
        cancel,
        on_delta,
        connect_timeout,
        overall_deadline,
    )
}

/// Stream with model-specific stop strings selected by the trusted caller.
/// Generic MLX models keep the empty default above; the fixed Qwen catalog
/// route supplies its reviewed ChatML terminator.
#[allow(clippy::too_many_arguments)]
pub fn stream_chat_with_stop_sequences<F>(
    port: u16,
    model: &str,
    messages: &[ChatMessage],
    stop_sequences: &[&str],
    cancel: Arc<AtomicBool>,
    mut on_delta: F,
    connect_timeout: Duration,
    overall_deadline: Instant,
) -> Result<StreamOutcome, ChatError>
where
    F: FnMut(&str),
{
    let request_body = build_request_body(model, messages, stop_sequences);

    let addr: SocketAddr = ([127, 0, 0, 1], port).into();
    let stream = TcpStream::connect_timeout(&addr, connect_timeout)
        .map_err(|source| ChatError::Transport { port, source })?;
    stream
        .set_write_timeout(Some(connect_timeout))
        .map_err(|source| ChatError::Transport { port, source })?;
    stream
        .set_read_timeout(Some(STREAM_READ_POLL))
        .map_err(|source| ChatError::Transport { port, source })?;

    let req = format!(
        "POST /v1/chat/completions HTTP/1.1\r\n\
         Host: 127.0.0.1:{port}\r\n\
         User-Agent: plume\r\n\
         Accept: text/event-stream\r\n\
         Content-Type: application/json\r\n\
         Content-Length: {}\r\n\
         Connection: close\r\n\
         \r\n\
         {request_body}",
        request_body.len(),
    );
    let mut stream_for_write = &stream;
    stream_for_write
        .write_all(req.as_bytes())
        .map_err(|source| ChatError::Transport { port, source })?;
    stream_for_write
        .flush()
        .map_err(|source| ChatError::Transport { port, source })?;

    // Read headers byte-by-byte on the raw stream so a BufReader's
    // read-ahead doesn't swallow any SSE body bytes past the
    // `\r\n\r\n` boundary.
    let mut stream_for_head = &stream;
    let (status, _headers) = read_response_head(&mut stream_for_head, &cancel, overall_deadline)
        .map_err(|source| ChatError::Transport { port, source })?;

    let mut reader = BufReader::new(stream);

    // Non-2xx: drain the body for a useful error message, map 404
    // to ModelNotFound (the most common cause is the server having
    // loaded a different model than the caller assumed).
    if !(200..300).contains(&status) {
        let body = drain_body_to_string(&mut reader, &cancel, overall_deadline).unwrap_or_default();
        let message = extract_error_message(&body).unwrap_or_else(|| body.clone());
        if status == 404 {
            return Err(ChatError::ModelNotFound {
                model: model.to_string(),
                message,
            });
        }
        return Err(ChatError::BadStatus { status, message });
    }

    // SSE body loop. Read one line at a time, feed each to the
    // pure D39 parser, react to the classified events.
    let mut parser = SseParser::new();
    let mut line = String::new();
    let mut last_stats = MlxFrameStats::default();
    let mut last_model: Option<String> = None;
    loop {
        line.clear();
        match read_line_bounded(&mut reader, &mut line, &cancel, overall_deadline) {
            Ok(ReadOutcome::Cancelled) => {
                return Ok(StreamOutcome::Cancelled {
                    model_id: last_model,
                });
            }
            Ok(ReadOutcome::Eof) => {
                return Ok(StreamOutcome::EofBeforeDone {
                    model_id: last_model,
                });
            }
            Ok(ReadOutcome::Line) => {
                // `read_line` returns the LF; strip it before
                // handing to the SSE parser (the parser also strips
                // a trailing `\r` to tolerate CRLF clients).
                let trimmed = line.strip_suffix('\n').unwrap_or(&line);
                let events = parser
                    .parse_line(trimmed)
                    .map_err(|e| ChatError::Parse(e.to_string()))?;
                for event in events {
                    match event {
                        SseEvent::Delta {
                            content,
                            finish_reason: _,
                        } => {
                            if let Some(c) = content {
                                if !c.is_empty() {
                                    on_delta(&c);
                                }
                            }
                            // Track the model id from the request so
                            // the chat/done event has something to
                            // echo back. We don't pull the chunk's
                            // own `model` field — the D39 parser
                            // deliberately doesn't expose it (model
                            // resolution isn't the parser's job),
                            // and the request id is honest about
                            // what the caller asked for.
                            if last_model.is_none() {
                                last_model = Some(model.to_string());
                            }
                        }
                        SseEvent::Usage(usage) => {
                            // Last usage wins; mlx-lm emits at most
                            // one per stream (inlined OR trailing,
                            // not both).
                            last_stats = MlxFrameStats {
                                prompt_tokens: usage.prompt_tokens,
                                completion_tokens: usage.completion_tokens,
                            };
                        }
                        SseEvent::Done => {
                            return Ok(StreamOutcome::Done {
                                model_id: last_model.unwrap_or_else(|| model.to_string()),
                                stats: last_stats,
                            });
                        }
                    }
                }
            }
            Err(source) => return Err(ChatError::Transport { port, source }),
        }
    }
}

/// Collect one bounded model response through the exact same socket,
/// cancellation, deadline, SSE, and stop-sequence path used by chat.
#[allow(clippy::too_many_arguments)]
#[allow(dead_code)] // The research run wiring lands after its bounded model port.
pub(crate) fn collect_chat_with_stop_sequences(
    port: u16,
    model: &str,
    messages: &[ChatMessage],
    stop_sequences: &[&str],
    cancel: Arc<AtomicBool>,
    connect_timeout: Duration,
    overall_deadline: Instant,
) -> Result<CollectedTurn, ChatError> {
    let mut text = String::new();
    let outcome = stream_chat_with_stop_sequences(
        port,
        model,
        messages,
        stop_sequences,
        cancel,
        |delta| text.push_str(delta),
        connect_timeout,
        overall_deadline,
    )?;
    Ok(CollectedTurn { text, outcome })
}

/// Explicit output cap sent on every MLX chat request (D129C).
/// Before this, Plume sent no `max_tokens` and silently inherited
/// mlx-lm's version-dependent server default — a hidden, drifting
/// output cap. An explicit generous cap makes the app's effective
/// behavior self-contained, and it lets benchmark records declare an
/// output-token cap that is actually on the wire instead of a guess.
pub const MAX_OUTPUT_TOKENS: u32 = 4096;

fn build_request_body(model: &str, messages: &[ChatMessage], stop_sequences: &[&str]) -> String {
    let messages_json = serde_json::Value::Array(
        messages
            .iter()
            .map(|m| {
                serde_json::json!({
                    "role": role_str(m.role),
                    "content": m.content,
                })
            })
            .collect(),
    );
    // `stream_options.include_usage = true` asks the server to emit
    // a usage chunk before `[DONE]`. mlx-lm honors this flag; if a
    // future build doesn't, the parser still tolerates a missing
    // usage event (the resulting MlxFrameStats is just the default
    // None/None).
    let mut body = serde_json::json!({
        "model": model,
        "messages": messages_json,
        "stream": true,
        "stream_options": { "include_usage": true },
        "max_tokens": MAX_OUTPUT_TOKENS,
    });
    if !stop_sequences.is_empty() {
        body["stop"] = serde_json::json!(stop_sequences);
    }
    body.to_string()
}

fn role_str(role: ChatRole) -> &'static str {
    match role {
        ChatRole::System => "system",
        ChatRole::User => "user",
        ChatRole::Assistant => "assistant",
        ChatRole::Tool => "tool",
    }
}

// --- HTTP framing helpers ---------------------------------------------
//
// These mirror `chat::ollama::http::*` but are kept private to this
// module. The Ollama versions are `pub(super)` and a refactor to a
// shared `chat::http_utils` is the right move when the third adapter
// (LM Studio's OpenAI-compat chat) lands and the duplication actually
// hurts. Two copies of ~50 lines isn't worth the refactor today.
// (The body-line reader IS shared now — `chat::stream_read` — because
// its bounded version carries real safety logic, not just framing.)

fn read_response_head(
    stream: &mut &TcpStream,
    cancel: &Arc<AtomicBool>,
    deadline: Instant,
) -> io::Result<(u16, String)> {
    let mut buf: Vec<u8> = Vec::with_capacity(256);
    let mut one = [0u8; 1];
    loop {
        if cancel.load(Ordering::SeqCst) {
            return Err(io::Error::other("cancelled during header read"));
        }
        if Instant::now() > deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "chat stream deadline elapsed during header read",
            ));
        }
        match stream.read(&mut one) {
            Ok(0) => {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "connection closed before HTTP headers",
                ));
            }
            Ok(_) => {
                buf.push(one[0]);
                if buf.len() >= 4 && &buf[buf.len() - 4..] == b"\r\n\r\n" {
                    let header_text = String::from_utf8_lossy(&buf).to_string();
                    let status = parse_status_line(&header_text).ok_or_else(|| {
                        io::Error::new(
                            io::ErrorKind::InvalidData,
                            format!(
                                "could not parse status line: {:?}",
                                header_text.lines().next().unwrap_or("")
                            ),
                        )
                    })?;
                    return Ok((status, header_text));
                }
                if buf.len() > 64 * 1024 {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "response headers exceeded 64 KiB",
                    ));
                }
            }
            Err(e) if is_timeout_kind(e.kind()) => continue,
            Err(e) => return Err(e),
        }
    }
}

fn parse_status_line(header_text: &str) -> Option<u16> {
    let line = header_text.lines().next()?;
    let mut parts = line.split_whitespace();
    let _version = parts.next();
    parts.next()?.parse::<u16>().ok()
}

fn drain_body_to_string(
    reader: &mut BufReader<TcpStream>,
    cancel: &Arc<AtomicBool>,
    deadline: Instant,
) -> io::Result<String> {
    let mut buf = Vec::with_capacity(512);
    let mut tmp = [0u8; 512];
    loop {
        if cancel.load(Ordering::SeqCst) {
            break;
        }
        if Instant::now() > deadline {
            break;
        }
        match reader.read(&mut tmp) {
            Ok(0) => break,
            Ok(n) => {
                buf.extend_from_slice(&tmp[..n]);
                if buf.len() > 64 * 1024 {
                    break;
                }
            }
            Err(e) if is_timeout_kind(e.kind()) => continue,
            Err(e) => return Err(e),
        }
    }
    Ok(String::from_utf8_lossy(&buf).to_string())
}

/// Pull `{"error": {"message": "..."}}` or `{"error": "..."}` from
/// an OpenAI-shape error body. Returns `None` if the body isn't
/// JSON or has neither shape; caller falls back to the raw body.
fn extract_error_message(body: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(body).ok()?;
    let err = value.get("error")?;
    // OpenAI 'error' can be either a string or `{"message": "..."}`.
    if let Some(s) = err.as_str() {
        return Some(s.to_string());
    }
    err.get("message")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

#[cfg(test)]
#[path = "mlx_lm_tests.rs"]
mod tests;
