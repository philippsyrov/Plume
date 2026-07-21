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
//!     `Accept: text/event-stream`. MLX-LM responds close-delimited;
//!     MLX-VLM's FastAPI server responds with HTTP chunked transfer
//!     encoding. The body reader removes chunk framing before the
//!     shared bounded-line reader feeds bytes to the SSE parser.
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

use base64::Engine as _;

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

/// Keep receipt-backed model paths and raw runtime bodies out of user-visible
/// fixed-catalog errors. The raw `ChatError` remains available to debug logs.
pub(crate) fn format_fixed_catalog_chat_error(error: &ChatError, catalog_id: &str) -> String {
    match error {
        ChatError::Transport { port, .. } => {
            format!("Could not reach the MLX runtime for catalog model '{catalog_id}' on 127.0.0.1:{port}.")
        }
        ChatError::ModelNotFound { .. } => format!(
            "Catalog model '{catalog_id}' was not found by the MLX runtime. Stop and start the model again."
        ),
        ChatError::BadStatus { status, .. } => format!(
            "The MLX runtime returned HTTP {status} for catalog model '{catalog_id}'."
        ),
        ChatError::Parse(_) => format!(
            "The MLX runtime returned an invalid response for catalog model '{catalog_id}'."
        ),
    }
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
    on_delta: F,
    connect_timeout: Duration,
    overall_deadline: Instant,
) -> Result<StreamOutcome, ChatError>
where
    F: FnMut(&str),
{
    stream_chat_with_stop_sequences_and_images(
        port,
        model,
        messages,
        stop_sequences,
        &[],
        false,
        cancel,
        on_delta,
        connect_timeout,
        overall_deadline,
    )
}

/// Stream an OpenAI-compatible MLX-VLM turn with bounded PNG inputs attached
/// to the final user message. Text-only callers keep using the wrapper above.
#[allow(clippy::too_many_arguments)]
pub fn stream_chat_with_stop_sequences_and_images<F>(
    port: u16,
    model: &str,
    messages: &[ChatMessage],
    stop_sequences: &[&str],
    images: &[Vec<u8>],
    enforce_role_alternation: bool,
    cancel: Arc<AtomicBool>,
    mut on_delta: F,
    connect_timeout: Duration,
    overall_deadline: Instant,
) -> Result<StreamOutcome, ChatError>
where
    F: FnMut(&str),
{
    let request_body = build_request_body_with_images(
        model,
        messages,
        stop_sequences,
        images,
        enforce_role_alternation,
    );

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
    let (status, headers) = read_response_head(&mut stream_for_head, &cancel, overall_deadline)
        .map_err(|source| ChatError::Transport { port, source })?;

    let raw_reader = BufReader::new(stream);
    let mut reader: Box<dyn io::BufRead> = if has_chunked_transfer_encoding(&headers) {
        Box::new(BufReader::new(ChunkedBodyReader::new(raw_reader)))
    } else {
        Box::new(raw_reader)
    };

    // Non-2xx: drain the body for a useful error message, map 404
    // to ModelNotFound (the most common cause is the server having
    // loaded a different model than the caller assumed).
    if !(200..300).contains(&status) {
        let body =
            drain_body_to_string(reader.as_mut(), &cancel, overall_deadline).unwrap_or_default();
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
        match read_line_bounded(reader.as_mut(), &mut line, &cancel, overall_deadline) {
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

#[cfg(test)]
fn build_request_body(model: &str, messages: &[ChatMessage], stop_sequences: &[&str]) -> String {
    build_request_body_with_images(model, messages, stop_sequences, &[], false)
}

fn build_request_body_with_images(
    model: &str,
    messages: &[ChatMessage],
    stop_sequences: &[&str],
    images: &[Vec<u8>],
    enforce_role_alternation: bool,
) -> String {
    let normalized_messages;
    let messages = if enforce_role_alternation {
        normalized_messages = coalesce_adjacent_roles(messages);
        normalized_messages.as_slice()
    } else {
        messages
    };
    let final_user = messages
        .iter()
        .rposition(|message| message.role == ChatRole::User);
    let messages_json = serde_json::Value::Array(
        messages
            .iter()
            .enumerate()
            .map(|(index, m)| {
                let content = if Some(index) == final_user && !images.is_empty() {
                    let mut parts = vec![serde_json::json!({
                        "type": "text",
                        "text": m.content,
                    })];
                    parts.extend(images.iter().map(|image| {
                        let encoded = base64::engine::general_purpose::STANDARD.encode(image);
                        serde_json::json!({
                            "type": "image_url",
                            "image_url": { "url": format!("data:image/png;base64,{encoded}") },
                        })
                    }));
                    serde_json::Value::Array(parts)
                } else {
                    serde_json::Value::String(m.content.clone())
                };
                serde_json::json!({
                    "role": role_str(m.role),
                    "content": content,
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

/// Qwen2-VL's MLX-VLM chat template rejects adjacent messages with the same role.
/// Plume transcripts can legitimately contain them when a non-chat command
/// records a user request without a model reply, so fold only those adjacent
/// entries at the runtime adapter boundary and keep every byte of their text.
fn coalesce_adjacent_roles(messages: &[ChatMessage]) -> Vec<ChatMessage> {
    let mut normalized: Vec<ChatMessage> = Vec::with_capacity(messages.len());
    for message in messages {
        if let Some(previous) = normalized.last_mut() {
            if previous.role == message.role {
                previous.content.push_str("\n\n");
                previous.content.push_str(&message.content);
                continue;
            }
        }
        normalized.push(message.clone());
    }
    normalized
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

const MAX_CHUNK_FRAMING_LINE_BYTES: usize = 8 * 1024;

enum ChunkState {
    Size,
    Data(usize),
    DataCr,
    DataLf,
    Trailers,
    Done,
}

/// Remove RFC 9112 chunk framing while leaving SSE bytes unchanged. The
/// decoder returns socket timeout errors immediately, so the outer bounded
/// line reader retains its 200 ms cancel/deadline polling behavior.
struct ChunkedBodyReader<R> {
    inner: R,
    state: ChunkState,
    framing_line: Vec<u8>,
}

impl<R: io::BufRead> ChunkedBodyReader<R> {
    fn new(inner: R) -> Self {
        Self {
            inner,
            state: ChunkState::Size,
            framing_line: Vec::new(),
        }
    }

    fn read_framing_line(&mut self) -> io::Result<()> {
        let mut byte = [0u8; 1];
        loop {
            match self.inner.read(&mut byte)? {
                0 => {
                    return Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "chunked body ended inside framing",
                    ));
                }
                _ => {
                    self.framing_line.push(byte[0]);
                    if self.framing_line.len() > MAX_CHUNK_FRAMING_LINE_BYTES {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            "chunked body framing line exceeded 8 KiB",
                        ));
                    }
                    if byte[0] == b'\n' {
                        return Ok(());
                    }
                }
            }
        }
    }

    fn parse_chunk_size(&self) -> io::Result<usize> {
        let line = self
            .framing_line
            .strip_suffix(b"\r\n")
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid chunk-size line"))?;
        let size = line.split(|byte| *byte == b';').next().unwrap_or_default();
        let size = std::str::from_utf8(size)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid chunk size"))?;
        usize::from_str_radix(size, 16)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid chunk size"))
    }

    fn read_expected_byte(&mut self, expected: u8) -> io::Result<()> {
        let mut byte = [0u8; 1];
        match self.inner.read(&mut byte)? {
            0 => Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "chunked body ended before chunk terminator",
            )),
            _ if byte[0] == expected => Ok(()),
            _ => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid chunk terminator",
            )),
        }
    }
}

impl<R: io::BufRead> Read for ChunkedBodyReader<R> {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        if output.is_empty() {
            return Ok(0);
        }
        loop {
            match self.state {
                ChunkState::Size => {
                    self.read_framing_line()?;
                    let size = self.parse_chunk_size()?;
                    self.framing_line.clear();
                    self.state = if size == 0 {
                        ChunkState::Trailers
                    } else {
                        ChunkState::Data(size)
                    };
                }
                ChunkState::Data(remaining) => {
                    let available = remaining.min(output.len());
                    let read = self.inner.read(&mut output[..available])?;
                    if read == 0 {
                        return Err(io::Error::new(
                            io::ErrorKind::UnexpectedEof,
                            "chunked body ended inside chunk data",
                        ));
                    }
                    self.state = if read == remaining {
                        ChunkState::DataCr
                    } else {
                        ChunkState::Data(remaining - read)
                    };
                    return Ok(read);
                }
                ChunkState::DataCr => {
                    self.read_expected_byte(b'\r')?;
                    self.state = ChunkState::DataLf;
                }
                ChunkState::DataLf => {
                    self.read_expected_byte(b'\n')?;
                    self.state = ChunkState::Size;
                }
                ChunkState::Trailers => {
                    self.read_framing_line()?;
                    let trailers_done = self.framing_line == b"\r\n";
                    self.framing_line.clear();
                    if trailers_done {
                        self.state = ChunkState::Done;
                    }
                }
                ChunkState::Done => return Ok(0),
            }
        }
    }
}

fn has_chunked_transfer_encoding(headers: &str) -> bool {
    headers.lines().any(|line| {
        line.split_once(':').is_some_and(|(name, value)| {
            name.eq_ignore_ascii_case("transfer-encoding")
                && value
                    .split(',')
                    .any(|encoding| encoding.trim().eq_ignore_ascii_case("chunked"))
        })
    })
}

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

fn drain_body_to_string<R: Read + ?Sized>(
    reader: &mut R,
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
