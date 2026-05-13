//! Ollama `/api/chat` adapter — synchronous and streaming.
//!
//! Real wire shape verified against
//! <https://github.com/ollama/ollama/blob/main/docs/api.md#generate-a-chat-completion>
//! and `api/types.go` (ChatRequest / ChatResponse / Message), plus
//! `server/routes.go` for streaming + error semantics.
//!
//! Non-streaming request body (`send_chat`):
//! ```json
//! { "model": "llama3:latest",
//!   "messages": [{"role": "user", "content": "hi"}],
//!   "stream": false }
//! ```
//!
//! Non-streaming response: single JSON object with `message`,
//! `done: true`, and metrics.
//!
//! Streaming request body (`stream_chat`): same as above but with
//! `"stream": true`. Response is `Content-Type:
//! application/x-ndjson` — one JSON object per line. Each line
//! carries a DELTA in `message.content` (not cumulative). The final
//! line has `done: true`, empty `content`, plus metrics.
//!
//! Error shape on a missing model: HTTP 404 with single-JSON body
//! `{"error": "model 'foo' not found, try pulling it first"}`. This
//! is the SAME for streaming and non-streaming — Ollama sends the
//! 404 + JSON before the NDJSON stream begins, so the adapter can
//! treat both modes the same way for the not-found path.
//!
//! Streaming cancellation is cooperative. `stream_chat` checks an
//! `AtomicBool` between line reads. The underlying TCP read uses a
//! short poll timeout so the cancel signal is acted on within
//! ~200 ms in the worst case. Documented limit: one more in-flight
//! line may still be buffered by the kernel before the loop notices.

use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::Deserialize;

#[cfg(test)]
use super::{ChatFinish, ChatResponse};
use super::{ChatMessage, ChatRole};
#[cfg(test)]
use crate::providers::http::http_request_with_status;

const CHAT_PATH: &str = "/api/chat";

/// Poll interval for the streaming read loop. The cancel flag is
/// re-checked at most every `STREAM_READ_POLL`, so this trades
/// responsiveness against CPU. 200 ms is fast enough for a human
/// clicking Stop and slow enough that the loop is idle the vast
/// majority of its life. Documented in
/// `docs/IPC_CONTRACT.md § chat`.
const STREAM_READ_POLL: Duration = Duration::from_millis(200);

/// Errors the Ollama chat adapter can raise. The command layer maps
/// these onto `IpcError`; tests assert on the variant, not on the
/// formatted message.
#[derive(Debug, thiserror::Error)]
pub enum ChatError {
    #[error("ollama at {host}:{port} did not answer: {source}")]
    Transport {
        host: String,
        port: u16,
        #[source]
        source: io::Error,
    },
    /// Model not found at the runtime. Ollama returns 404 with an
    /// `{"error": "..."}` body; the message is carried verbatim so
    /// the UI can show what the daemon said ("try pulling it
    /// first", typically).
    #[error("ollama reports model '{model}' not found: {message}")]
    ModelNotFound { model: String, message: String },
    /// Any other non-2xx status. Status code is kept so the UI can
    /// distinguish a 5xx (server fault) from a 4xx (bad request)
    /// even though both map to `ProviderDown` today.
    #[error("ollama returned HTTP {status}: {message}")]
    BadStatus { status: u16, message: String },
    #[error("ollama response was not valid JSON: {0}")]
    Parse(String),
}

/// Send a non-streaming chat completion to a localhost Ollama daemon
/// and parse the assistant message out of the response.
///
/// `model` is the Ollama tag string (`llama3:latest`,
/// `qwen2.5-coder:14b-q4`, …). `messages` is the full transcript;
/// Ollama is stateless across `/api/chat` calls, so the caller
/// concatenates history themselves.
///
/// `#[cfg(test)]`-gated since D7.1: the shipping IPC path goes
/// through `stream_chat`. This function and its support code are
/// kept as a reference implementation of the non-streaming protocol
/// and as the basis for several parser / error-mapping tests in
/// this module.
#[cfg(test)]
pub fn send_chat(
    host: &str,
    port: u16,
    model: &str,
    messages: &[ChatMessage],
    timeout: Duration,
) -> Result<ChatResponse, ChatError> {
    let request_body = build_request_body(model, messages);

    let (status, body) =
        http_request_with_status(host, port, "POST", CHAT_PATH, Some(&request_body), timeout)
            .map_err(|source| ChatError::Transport {
                host: host.to_string(),
                port,
                source,
            })?;

    if status == 404 {
        let message = extract_error_message(&body).unwrap_or_else(|| body.clone());
        return Err(ChatError::ModelNotFound {
            model: model.to_string(),
            message,
        });
    }
    if !(200..300).contains(&status) {
        let message = extract_error_message(&body).unwrap_or_else(|| body.clone());
        return Err(ChatError::BadStatus { status, message });
    }

    let parsed: OllamaChatResponse =
        serde_json::from_str(&body).map_err(|e| ChatError::Parse(e.to_string()))?;

    let finish = if parsed.done {
        ChatFinish::Stop
    } else {
        ChatFinish::Length
    };

    Ok(ChatResponse {
        message: ChatMessage {
            role: ChatRole::Assistant,
            content: parsed.message.content,
        },
        provider_id: "ollama".to_string(),
        model_id: parsed.model,
        // Caller fills duration_ms; the adapter doesn't time itself.
        duration_ms: 0,
        finish,
    })
}

/// Build the JSON request body. Kept out of `send_chat` so a test can
/// assert on the exact bytes without spinning up a TCP listener.
/// Test-only since D7.1 — the shipping path uses
/// `build_request_body_streaming`.
#[cfg(test)]
fn build_request_body(model: &str, messages: &[ChatMessage]) -> String {
    // Hand-build the JSON instead of using a derived `Serialize`
    // struct — that way the wire shape is auditable next to the
    // upstream docs in this file's header comment without a layer
    // of indirection.
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
    serde_json::json!({
        "model": model,
        "messages": messages_json,
        "stream": false,
    })
    .to_string()
}

fn role_str(role: ChatRole) -> &'static str {
    match role {
        ChatRole::System => "system",
        ChatRole::User => "user",
        ChatRole::Assistant => "assistant",
        ChatRole::Tool => "tool",
    }
}

/// Pull `{"error": "..."}` out of an Ollama error body when present.
/// Ollama's error responses are not strictly schema'd; if the body
/// isn't JSON or doesn't have an `error` field we return `None` and
/// the caller falls back to the raw body.
fn extract_error_message(body: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(body).ok()?;
    value
        .get("error")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

/// Summary of why `stream_chat` returned. The caller uses this to
/// pick the right `ChatFinish` for the terminal `chat.done` event.
#[derive(Debug, PartialEq, Eq)]
pub enum StreamOutcome {
    /// Saw a frame with `done: true`. Carries the model id the
    /// runtime reported it served — frontend displays this rather
    /// than the request id since Ollama can resolve `llama3` →
    /// `llama3:latest`. `stats` carries the generation telemetry
    /// the final frame reported, with each field absent when the
    /// runtime didn't include it (D9).
    Done {
        model_id: String,
        stats: OllamaFrameStats,
    },
    /// Cancel flag tripped before a `done: true` frame arrived.
    /// `model_id` is `None` if the cancel happened before any
    /// frame; otherwise the last-seen id. Stats are intentionally
    /// not carried here: Ollama only emits eval_count / duration
    /// in the final frame, so a cancelled stream has nothing
    /// authoritative to report.
    Cancelled { model_id: Option<String> },
    /// Socket closed cleanly before a `done: true` frame. Treated as
    /// a truncated reply (`ChatFinish::Length`). Same no-stats
    /// rationale as `Cancelled`.
    EofBeforeDone { model_id: Option<String> },
}

/// Raw counts + durations from Ollama's final NDJSON frame. The
/// command layer translates this onto `chat::ChatStats` (the
/// provider-neutral wire shape); leaving the raw fields here means
/// the parser doesn't have to know about the public wire type.
///
/// Durations are kept in nanoseconds the way Ollama wrote them —
/// the translator converts to milliseconds for the wire. Storing
/// as `Option<u64>` rather than `u64`-with-zero matters: zero is a
/// legitimate value (a zero-token reply), so "field not present"
/// has to be its own state.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct OllamaFrameStats {
    pub eval_count: Option<u64>,
    pub eval_duration_ns: Option<u64>,
    pub prompt_eval_count: Option<u64>,
    pub prompt_eval_duration_ns: Option<u64>,
}

/// Stream a chat completion from a localhost Ollama daemon. Each
/// NDJSON frame's delta is forwarded to `on_delta`; the function
/// returns once the runtime sends `done: true`, the cancel flag
/// trips, or the socket closes.
///
/// `on_delta` should be cheap — it runs on the streaming thread.
/// Emitting Tauri events out of it is fine because `AppHandle::emit`
/// is non-blocking.
///
/// Errors:
///   * `Transport` — TCP / write / read failure during the call.
///   * `ModelNotFound` — Ollama returned 404 before the NDJSON
///     stream began. Same path as `send_chat`.
///   * `BadStatus` — any other non-2xx, or an NDJSON frame that
///     carries an `error` field.
///   * `Parse` — a frame was not valid JSON.
// 8 parameters is past clippy's default complaint threshold but the
// alternative — bundling them into a `StreamChatArgs` struct just
// to dodge a lint — buries the call shape. The function is called
// from exactly one place (`commands::chat::run_stream`) and from
// tests, so reading the call sites is enough; suppress the lint.
#[allow(clippy::too_many_arguments)]
pub fn stream_chat<F>(
    host: &str,
    port: u16,
    model: &str,
    messages: &[ChatMessage],
    cancel: Arc<AtomicBool>,
    mut on_delta: F,
    connect_timeout: Duration,
    overall_deadline: Instant,
) -> Result<StreamOutcome, ChatError>
where
    F: FnMut(&str),
{
    let request_body = build_request_body_streaming(model, messages);

    // 1. Connect + send. These are short, so the connect-budget
    //    timeout is enough — we don't yet swap to the per-line poll.
    let addr: SocketAddr =
        format!("{host}:{port}")
            .parse()
            .map_err(|e: std::net::AddrParseError| ChatError::Transport {
                host: host.to_string(),
                port,
                source: io::Error::new(io::ErrorKind::InvalidInput, format!("bad addr: {e}")),
            })?;
    let stream = TcpStream::connect_timeout(&addr, connect_timeout).map_err(|source| {
        ChatError::Transport {
            host: host.to_string(),
            port,
            source,
        }
    })?;
    stream
        .set_write_timeout(Some(connect_timeout))
        .map_err(|source| ChatError::Transport {
            host: host.to_string(),
            port,
            source,
        })?;
    // Short read timeout so the loop can re-poll the cancel flag
    // at human-perceivable cadence.
    stream
        .set_read_timeout(Some(STREAM_READ_POLL))
        .map_err(|source| ChatError::Transport {
            host: host.to_string(),
            port,
            source,
        })?;

    let req = format!(
        "POST {CHAT_PATH} HTTP/1.1\r\n\
         Host: {host}:{port}\r\n\
         User-Agent: plume\r\n\
         Accept: application/x-ndjson\r\n\
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
        .map_err(|source| ChatError::Transport {
            host: host.to_string(),
            port,
            source,
        })?;
    stream_for_write
        .flush()
        .map_err(|source| ChatError::Transport {
            host: host.to_string(),
            port,
            source,
        })?;

    // 2. Read headers byte-by-byte on the raw stream. Doing this
    //    BEFORE wrapping in a BufReader is load-bearing: BufReader
    //    has no "unshift" API, so any bytes it accidentally pulls
    //    past the `\r\n\r\n` boundary would be lost to the NDJSON
    //    body loop below. Headers are tiny (<200 B for /api/chat) so
    //    one-byte reads are cheap.
    let mut stream_for_head = &stream;
    let (status, _headers) = read_response_head(&mut stream_for_head, &cancel, overall_deadline)
        .map_err(|source| ChatError::Transport {
            host: host.to_string(),
            port,
            source,
        })?;

    // 3. Wrap the stream for the body. From here on the read loop
    //    is line-buffered so multi-byte NDJSON frames are efficient.
    let mut reader = BufReader::new(stream);

    // 4. Non-2xx → drain the body and map to the same typed errors
    //    `send_chat` produces. 404 stays distinct so the UI can
    //    suggest "pull the model" instead of "server might be down".
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

    // 5. NDJSON loop. Each iteration: poll cancel, read a line,
    //    parse, forward delta, watch for `done`.
    let mut line = String::new();
    let mut last_model: Option<String> = None;
    loop {
        line.clear();
        match read_line_polled(&mut reader, &mut line, &cancel, overall_deadline) {
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
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                // Ollama on stream:true emits a single JSON object per
                // line. Treat in-stream `error` as a fatal frame so a
                // model crash mid-generation surfaces as BadStatus to
                // the command layer.
                let frame: OllamaStreamFrame =
                    serde_json::from_str(trimmed).map_err(|e| ChatError::Parse(e.to_string()))?;
                if let Some(err) = frame.error {
                    return Err(ChatError::BadStatus {
                        status: 200,
                        message: err,
                    });
                }
                if let Some(model_seen) = frame.model.clone() {
                    last_model = Some(model_seen);
                }
                if let Some(msg) = frame.message.as_ref() {
                    if !msg.content.is_empty() {
                        on_delta(&msg.content);
                    }
                }
                if frame.done {
                    // D9: pull the four telemetry fields out of the
                    // final frame. Each stays `None` when the
                    // runtime didn't include it; the command layer
                    // doesn't fabricate values.
                    let stats = OllamaFrameStats {
                        eval_count: frame.eval_count,
                        eval_duration_ns: frame.eval_duration,
                        prompt_eval_count: frame.prompt_eval_count,
                        prompt_eval_duration_ns: frame.prompt_eval_duration,
                    };
                    return Ok(StreamOutcome::Done {
                        model_id: last_model.unwrap_or_else(|| model.to_string()),
                        stats,
                    });
                }
            }
            Err(source) => {
                return Err(ChatError::Transport {
                    host: host.to_string(),
                    port,
                    source,
                });
            }
        }
    }
}

/// Build the request body for the streaming endpoint. Same shape as
/// `build_request_body` but with `stream: true` so Ollama returns
/// NDJSON instead of a single object.
fn build_request_body_streaming(model: &str, messages: &[ChatMessage]) -> String {
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
    serde_json::json!({
        "model": model,
        "messages": messages_json,
        "stream": true,
    })
    .to_string()
}

enum ReadOutcome {
    Line,
    Eof,
    Cancelled,
}

/// `read_line` wrapper that retries on `WouldBlock` / `TimedOut`
/// (which the short `STREAM_READ_POLL` causes constantly when the
/// model is "thinking" without producing tokens). Polls the cancel
/// flag between retries and enforces an overall deadline so a stuck
/// model doesn't hang the stream forever.
///
/// Partial-line semantics: `read_line` appends to `buf`. On
/// `WouldBlock` we don't clear it — any bytes already buffered stay
/// in `buf` so the next retry resumes the same line.
fn read_line_polled(
    reader: &mut BufReader<TcpStream>,
    buf: &mut String,
    cancel: &Arc<AtomicBool>,
    deadline: Instant,
) -> io::Result<ReadOutcome> {
    loop {
        if cancel.load(Ordering::SeqCst) {
            return Ok(ReadOutcome::Cancelled);
        }
        if Instant::now() > deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "chat stream deadline elapsed",
            ));
        }
        match reader.read_line(buf) {
            Ok(0) => return Ok(ReadOutcome::Eof),
            Ok(_) => return Ok(ReadOutcome::Line),
            Err(e) if is_timeout_kind(e.kind()) => continue,
            Err(e) => return Err(e),
        }
    }
}

fn is_timeout_kind(kind: io::ErrorKind) -> bool {
    matches!(kind, io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut)
}

/// Read HTTP response head from the raw TCP stream one byte at a
/// time, stopping exactly at the `\r\n\r\n` separator. Reading
/// byte-by-byte avoids ever over-reading into the body — once
/// `stream_chat` wraps the stream in a `BufReader`, every byte the
/// reader pulls from the kernel will be a body byte. The cost is
/// ~200 syscalls per chat call (one per header byte), which is
/// negligible on localhost.
///
/// Honors the same cancel + deadline contract as the body loop.
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

/// Pull whatever's left on the stream and return it as a UTF-8
/// string. Used only on error paths to surface Ollama's
/// `{"error": "..."}` body. The 64 KiB cap keeps a misbehaving
/// daemon from filling memory with a non-2xx body.
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

// --- streaming response framing -------------------------------------

#[derive(Debug, Deserialize)]
struct OllamaStreamFrame {
    /// Always echoed in normal frames; absent if the frame is an
    /// error-only payload. We tolerate either shape.
    #[serde(default)]
    model: Option<String>,
    /// The delta message. Absent in error-only frames; on the final
    /// frame `content` is the empty string.
    #[serde(default)]
    message: Option<OllamaStreamMessage>,
    #[serde(default)]
    done: bool,
    /// Set when a frame is an error report. Ollama surfaces this
    /// rarely (most failures come back as 4xx/5xx before the stream
    /// starts), but defensive parsing is cheap.
    #[serde(default)]
    error: Option<String>,

    // -- D9 telemetry fields, only present on the final `done:true`
    // frame. All optional so a future minor Ollama release that
    // drops one of them doesn't break the parse path. Stored as
    // `Option<u64>` because `0` is a legal value (empty reply) and
    // we want to distinguish "absent" from "zero".
    /// Tokens generated for the reply.
    #[serde(default)]
    eval_count: Option<u64>,
    /// Time spent generating the reply, in nanoseconds.
    #[serde(default)]
    eval_duration: Option<u64>,
    /// Tokens in the input prompt as evaluated by the model.
    #[serde(default)]
    prompt_eval_count: Option<u64>,
    /// Time spent evaluating the prompt, in nanoseconds.
    #[serde(default)]
    prompt_eval_duration: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct OllamaStreamMessage {
    #[allow(dead_code)]
    #[serde(default)]
    role: Option<String>,
    #[serde(default)]
    content: String,
}

// --- non-streaming response parsing (test-only since D7.1) ----------

#[cfg(test)]
#[derive(Debug, Deserialize)]
struct OllamaChatResponse {
    /// Echoed model id; can differ from the request (`llama3` →
    /// `llama3:latest`) so we propagate it verbatim.
    model: String,
    message: OllamaMessage,
    done: bool,
    // The rest of the wire payload (`created_at`, `total_duration`,
    // `eval_count`, etc.) is intentionally dropped. Adding fields
    // here later is additive; today's surface keeps no telemetry.
}

#[cfg(test)]
#[derive(Debug, Deserialize)]
struct OllamaMessage {
    // We trust Ollama to return role:"assistant" here. Plume always
    // overrides with ChatRole::Assistant when building the public
    // `ChatResponse`, so even if a future daemon returns "tool" or
    // "system" the surface remains honest.
    #[allow(dead_code)]
    role: String,
    content: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::http::read_full_request;
    // `Write` is already imported at the module top level, so test
    // bodies that call `sock.write_all(...)` resolve it through the
    // outer scope. No re-import needed.
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};
    use std::thread;

    /// Realistic /api/chat body trimmed to the surface Plume reads,
    /// plus extra keys (`created_at`, `total_duration`, …) we ignore
    /// on purpose to prove the parser tolerates upstream additions.
    const CHAT_FIXTURE: &str = r#"{
        "model": "llama3:latest",
        "created_at": "2024-04-19T10:00:00.123Z",
        "message": {
            "role": "assistant",
            "content": "Hello! How can I help you today?"
        },
        "done": true,
        "done_reason": "stop",
        "total_duration": 5191566416,
        "load_duration": 2154458,
        "prompt_eval_count": 26,
        "prompt_eval_duration": 383809000,
        "eval_count": 298,
        "eval_duration": 4799921000
    }"#;

    #[test]
    fn builds_request_body_with_stream_false() {
        // The wire shape is load-bearing: if `stream` flips back to
        // its default (true) the adapter would have to parse NDJSON
        // and D7 explicitly does not.
        let msgs = vec![
            ChatMessage {
                role: ChatRole::User,
                content: "hi".into(),
            },
            ChatMessage {
                role: ChatRole::Assistant,
                content: "hi back".into(),
            },
            ChatMessage {
                role: ChatRole::User,
                content: "more".into(),
            },
        ];
        let body = build_request_body("llama3:latest", &msgs);
        let parsed: serde_json::Value = serde_json::from_str(&body).expect("valid json");
        assert_eq!(parsed["model"], "llama3:latest");
        assert_eq!(parsed["stream"], false);
        let arr = parsed["messages"].as_array().expect("messages array");
        assert_eq!(arr.len(), 3);
        assert_eq!(arr[0]["role"], "user");
        assert_eq!(arr[1]["role"], "assistant");
        assert_eq!(arr[2]["content"], "more");
    }

    #[test]
    fn role_serializes_lowercase() {
        // Spot-check every variant — Ollama only accepts the lowercase
        // form, so a future enum rename here would silently break
        // chat with a "role must be one of ..." 400.
        assert_eq!(role_str(ChatRole::System), "system");
        assert_eq!(role_str(ChatRole::User), "user");
        assert_eq!(role_str(ChatRole::Assistant), "assistant");
        assert_eq!(role_str(ChatRole::Tool), "tool");
    }

    #[test]
    fn extracts_error_message_from_ollama_404_body() {
        let body = r#"{"error":"model 'foo' not found, try pulling it first"}"#;
        assert_eq!(
            extract_error_message(body).as_deref(),
            Some("model 'foo' not found, try pulling it first")
        );
    }

    #[test]
    fn extract_error_message_returns_none_for_non_json() {
        assert!(extract_error_message("plain text 500").is_none());
        assert!(extract_error_message("{not json}").is_none());
        assert!(extract_error_message(r#"{"detail":"not the error key"}"#).is_none());
    }

    #[test]
    fn send_chat_round_trip_against_stub() {
        // End-to-end: stub server returns a real-looking chat body;
        // adapter parses it and the wire request matches what
        // /api/chat expects. Captures the request bytes the way the
        // /api/show test does so the assertion isn't just on the
        // parser side.
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            CHAT_FIXTURE.len(),
            CHAT_FIXTURE,
        );
        let captured = Arc::new(Mutex::new(None::<Vec<u8>>));
        let captured_for_thread = captured.clone();
        thread::spawn(move || {
            if let Ok((mut sock, _)) = listener.accept() {
                let request = read_full_request(&mut sock);
                *captured_for_thread.lock().unwrap() = Some(request);
                let _ = sock.write_all(response.as_bytes());
            }
        });

        let msgs = vec![ChatMessage {
            role: ChatRole::User,
            content: "Hello?".into(),
        }];
        let resp = send_chat(
            "127.0.0.1",
            port,
            "llama3:latest",
            &msgs,
            Duration::from_millis(500),
        )
        .expect("send_chat");
        assert_eq!(resp.message.role, ChatRole::Assistant);
        assert_eq!(resp.message.content, "Hello! How can I help you today?");
        assert_eq!(resp.model_id, "llama3:latest");
        assert_eq!(resp.finish, ChatFinish::Stop);

        let request_bytes = captured
            .lock()
            .unwrap()
            .take()
            .expect("stub never received a request");
        let request = std::str::from_utf8(&request_bytes).expect("utf-8 request");
        assert!(
            request.starts_with("POST /api/chat HTTP/1.1\r\n"),
            "expected POST /api/chat, got start: {:?}",
            request.lines().next()
        );
        let body = request.split_once("\r\n\r\n").map(|(_, b)| b).unwrap_or("");
        assert!(
            body.contains("\"stream\":false"),
            "request body missing stream:false: {body:?}"
        );
        assert!(
            body.contains("\"model\":\"llama3:latest\""),
            "request body missing model: {body:?}"
        );
    }

    #[test]
    fn send_chat_maps_404_to_model_not_found() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let body = r#"{"error":"model 'ghost:tag' not found, try pulling it first"}"#;
        let response = format!(
            "HTTP/1.1 404 Not Found\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body,
        );
        thread::spawn(move || {
            if let Ok((mut sock, _)) = listener.accept() {
                use crate::providers::http::drain_request;
                drain_request(&mut sock);
                let _ = sock.write_all(response.as_bytes());
            }
        });

        let msgs = vec![ChatMessage {
            role: ChatRole::User,
            content: "hi".into(),
        }];
        let err = send_chat(
            "127.0.0.1",
            port,
            "ghost:tag",
            &msgs,
            Duration::from_millis(500),
        )
        .expect_err("should map 404 to ModelNotFound");
        match err {
            ChatError::ModelNotFound { model, message } => {
                assert_eq!(model, "ghost:tag");
                assert!(message.contains("not found"), "msg was: {message:?}");
            }
            other => panic!("expected ModelNotFound, got {other:?}"),
        }
    }

    #[test]
    fn send_chat_maps_5xx_to_bad_status() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let response =
            "HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
        thread::spawn(move || {
            if let Ok((mut sock, _)) = listener.accept() {
                use crate::providers::http::drain_request;
                drain_request(&mut sock);
                let _ = sock.write_all(response.as_bytes());
            }
        });

        let msgs = vec![ChatMessage {
            role: ChatRole::User,
            content: "hi".into(),
        }];
        let err = send_chat(
            "127.0.0.1",
            port,
            "anything",
            &msgs,
            Duration::from_millis(500),
        )
        .expect_err("should map 500 to BadStatus");
        match err {
            ChatError::BadStatus { status, .. } => assert_eq!(status, 500),
            other => panic!("expected BadStatus, got {other:?}"),
        }
    }

    #[test]
    fn send_chat_marks_done_false_as_length_finish() {
        // Ollama's non-streaming responses normally have done:true.
        // We treat done:false as a truncated reply so the UI can flag
        // it. Synthetic body — not common in the wild but documented.
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let body = r#"{"model":"x","message":{"role":"assistant","content":"..."},"done":false}"#;
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body,
        );
        thread::spawn(move || {
            if let Ok((mut sock, _)) = listener.accept() {
                use crate::providers::http::drain_request;
                drain_request(&mut sock);
                let _ = sock.write_all(response.as_bytes());
            }
        });

        let msgs = vec![ChatMessage {
            role: ChatRole::User,
            content: "hi".into(),
        }];
        let resp = send_chat("127.0.0.1", port, "x", &msgs, Duration::from_millis(500))
            .expect("send_chat");
        assert_eq!(resp.finish, ChatFinish::Length);
    }

    // ============ D7.1 streaming tests ============
    //
    // The streaming tests use a slower stub-server pattern: instead
    // of writing the entire response in one `write_all`, they emit
    // frames with deliberate gaps so we can assert that
    // `stream_chat` calls `on_delta` per-frame (not once at the
    // end) and that the cancel flag short-circuits mid-stream.

    use std::sync::atomic::AtomicBool;
    use std::time::Instant;

    fn streaming_response(frames: &[&str]) -> Vec<u8> {
        // Chunked-style NDJSON response. The HTTP head plus one
        // frame per line; the connection closes after the last
        // frame so the client sees an EOF if no `done:true` arrived.
        let body: String = frames.iter().map(|f| format!("{f}\n")).collect();
        let header =
            "HTTP/1.1 200 OK\r\nContent-Type: application/x-ndjson\r\nConnection: close\r\n\r\n";
        let mut out = Vec::new();
        out.extend_from_slice(header.as_bytes());
        out.extend_from_slice(body.as_bytes());
        out
    }

    #[test]
    fn stream_chat_round_trip_emits_per_frame_deltas() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let frames = vec![
            r#"{"model":"llama3:latest","created_at":"2024-04-19T10:00:00Z","message":{"role":"assistant","content":"Hel"},"done":false}"#,
            r#"{"model":"llama3:latest","created_at":"2024-04-19T10:00:01Z","message":{"role":"assistant","content":"lo"},"done":false}"#,
            r#"{"model":"llama3:latest","created_at":"2024-04-19T10:00:02Z","message":{"role":"assistant","content":"!"},"done":false}"#,
            // Final frame carries the D9 telemetry: 3 output tokens
            // (matches the three preceding content frames) in 600 ms,
            // a 12-token prompt evaluated in 100 ms. Sized so the
            // tok/s assertion is exact: 3 / 0.6 = 5.0.
            r#"{"model":"llama3:latest","created_at":"2024-04-19T10:00:03Z","message":{"role":"assistant","content":""},"done":true,"done_reason":"stop","total_duration":700000000,"prompt_eval_count":12,"prompt_eval_duration":100000000,"eval_count":3,"eval_duration":600000000}"#,
        ];
        let response = streaming_response(&frames);
        thread::spawn(move || {
            if let Ok((mut sock, _)) = listener.accept() {
                use crate::providers::http::drain_request;
                drain_request(&mut sock);
                let _ = sock.write_all(&response);
            }
        });

        let cancel = Arc::new(AtomicBool::new(false));
        let collected = Arc::new(Mutex::new(String::new()));
        let collected_for_cb = collected.clone();

        let outcome = stream_chat(
            "127.0.0.1",
            port,
            "llama3:latest",
            &[ChatMessage {
                role: ChatRole::User,
                content: "hi".into(),
            }],
            cancel,
            move |delta| collected_for_cb.lock().unwrap().push_str(delta),
            Duration::from_millis(500),
            Instant::now() + Duration::from_secs(5),
        )
        .expect("stream_chat");

        match outcome {
            StreamOutcome::Done { model_id, stats } => {
                assert_eq!(model_id, "llama3:latest");
                // D9: the four metric fields surface verbatim from
                // the final NDJSON frame. The parser doesn't yet
                // convert ns→ms; that happens in the command layer.
                assert_eq!(stats.eval_count, Some(3));
                assert_eq!(stats.eval_duration_ns, Some(600_000_000));
                assert_eq!(stats.prompt_eval_count, Some(12));
                assert_eq!(stats.prompt_eval_duration_ns, Some(100_000_000));
            }
            other => panic!("expected Done, got {other:?}"),
        }
        assert_eq!(collected.lock().unwrap().as_str(), "Hello!");
    }

    #[test]
    fn stream_chat_done_without_telemetry_returns_all_none_stats() {
        // Defensive parse: a daemon (or test stub) that produces
        // `done:true` without the optional metrics fields should
        // still succeed and surface `None` for each metric. This
        // pins the `#[serde(default)]` behavior; without it a
        // minor Ollama release that dropped a field would 500 the
        // stream parse.
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let frames =
            vec![r#"{"model":"m","message":{"role":"assistant","content":""},"done":true}"#];
        let response = streaming_response(&frames);
        thread::spawn(move || {
            if let Ok((mut sock, _)) = listener.accept() {
                use crate::providers::http::drain_request;
                drain_request(&mut sock);
                let _ = sock.write_all(&response);
            }
        });

        let cancel = Arc::new(AtomicBool::new(false));
        let outcome = stream_chat(
            "127.0.0.1",
            port,
            "m",
            &[ChatMessage {
                role: ChatRole::User,
                content: "hi".into(),
            }],
            cancel,
            |_| {},
            Duration::from_millis(500),
            Instant::now() + Duration::from_secs(5),
        )
        .expect("stream_chat");

        match outcome {
            StreamOutcome::Done { stats, .. } => {
                assert_eq!(stats.eval_count, None);
                assert_eq!(stats.eval_duration_ns, None);
                assert_eq!(stats.prompt_eval_count, None);
                assert_eq!(stats.prompt_eval_duration_ns, None);
            }
            other => panic!("expected Done, got {other:?}"),
        }
    }

    #[test]
    fn stream_chat_request_body_has_stream_true() {
        // Wire-level check: the streaming variant must set
        // `stream: true`. A regression to `false` would make Ollama
        // return a single non-NDJSON object and the line loop would
        // never advance.
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let frames =
            vec![r#"{"model":"m","message":{"role":"assistant","content":""},"done":true}"#];
        let response = streaming_response(&frames);
        let captured = Arc::new(Mutex::new(None::<Vec<u8>>));
        let captured_for_thread = captured.clone();
        thread::spawn(move || {
            if let Ok((mut sock, _)) = listener.accept() {
                let request = crate::providers::http::read_full_request(&mut sock);
                *captured_for_thread.lock().unwrap() = Some(request);
                let _ = sock.write_all(&response);
            }
        });

        let cancel = Arc::new(AtomicBool::new(false));
        let _ = stream_chat(
            "127.0.0.1",
            port,
            "m",
            &[ChatMessage {
                role: ChatRole::User,
                content: "x".into(),
            }],
            cancel,
            |_| {},
            Duration::from_millis(500),
            Instant::now() + Duration::from_secs(5),
        )
        .expect("stream_chat");

        let request_bytes = captured
            .lock()
            .unwrap()
            .take()
            .expect("stub never received a request");
        let request = std::str::from_utf8(&request_bytes).expect("utf-8 request");
        let body = request.split_once("\r\n\r\n").map(|(_, b)| b).unwrap_or("");
        assert!(
            body.contains("\"stream\":true"),
            "streaming request must set stream:true; body was: {body:?}"
        );
    }

    #[test]
    fn stream_chat_maps_404_to_model_not_found() {
        // 404 path is the same as send_chat's: single JSON body
        // delivered before any NDJSON, so the body loop never runs.
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let body = r#"{"error":"model 'ghost' not found"}"#;
        let response = format!(
            "HTTP/1.1 404 Not Found\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body,
        );
        thread::spawn(move || {
            if let Ok((mut sock, _)) = listener.accept() {
                use crate::providers::http::drain_request;
                drain_request(&mut sock);
                let _ = sock.write_all(response.as_bytes());
            }
        });

        let cancel = Arc::new(AtomicBool::new(false));
        let err = stream_chat(
            "127.0.0.1",
            port,
            "ghost",
            &[ChatMessage {
                role: ChatRole::User,
                content: "x".into(),
            }],
            cancel,
            |_| {},
            Duration::from_millis(500),
            Instant::now() + Duration::from_secs(5),
        )
        .expect_err("404 should error");
        match err {
            ChatError::ModelNotFound { model, message } => {
                assert_eq!(model, "ghost");
                assert!(message.contains("not found"));
            }
            other => panic!("expected ModelNotFound, got {other:?}"),
        }
    }

    #[test]
    fn stream_chat_returns_cancelled_when_flag_trips_mid_stream() {
        // Stub server emits two frames then sleeps before emitting
        // `done`. The test flips the cancel flag while the server
        // is idle; the stream should return Cancelled rather than
        // wait for the (never-arriving) final frame.
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let cancel = Arc::new(AtomicBool::new(false));
        let cancel_for_test = cancel.clone();
        thread::spawn(move || {
            if let Ok((mut sock, _)) = listener.accept() {
                use crate::providers::http::drain_request;
                drain_request(&mut sock);
                let _ = sock.write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Type: application/x-ndjson\r\nConnection: close\r\n\r\n",
                );
                let _ = sock.write_all(
                    b"{\"model\":\"m\",\"message\":{\"role\":\"assistant\",\"content\":\"He\"},\"done\":false}\n",
                );
                let _ = sock.write_all(
                    b"{\"model\":\"m\",\"message\":{\"role\":\"assistant\",\"content\":\"llo\"},\"done\":false}\n",
                );
                // Hold the socket open without writing more; the
                // client's cancel-flag check should fire before we
                // ever send another frame.
                thread::sleep(Duration::from_secs(2));
            }
        });

        // Race the cancel: wait a bit so the first two frames land
        // in the buffer, then flip the flag.
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(400));
            cancel_for_test.store(true, Ordering::SeqCst);
        });

        let collected = Arc::new(Mutex::new(String::new()));
        let collected_for_cb = collected.clone();
        let outcome = stream_chat(
            "127.0.0.1",
            port,
            "m",
            &[ChatMessage {
                role: ChatRole::User,
                content: "hi".into(),
            }],
            cancel,
            move |delta| collected_for_cb.lock().unwrap().push_str(delta),
            Duration::from_millis(500),
            Instant::now() + Duration::from_secs(5),
        )
        .expect("stream_chat returns cancelled, not errored");

        match outcome {
            StreamOutcome::Cancelled { model_id } => assert_eq!(model_id.as_deref(), Some("m")),
            other => panic!("expected Cancelled, got {other:?}"),
        }
        // Both pre-cancel frames should have been forwarded.
        let collected = collected.lock().unwrap();
        assert_eq!(collected.as_str(), "Hello");
    }

    #[test]
    fn stream_chat_treats_in_stream_error_frame_as_bad_status() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let frames = vec![r#"{"error":"model crashed mid-generation"}"#];
        let response = streaming_response(&frames);
        thread::spawn(move || {
            if let Ok((mut sock, _)) = listener.accept() {
                use crate::providers::http::drain_request;
                drain_request(&mut sock);
                let _ = sock.write_all(&response);
            }
        });

        let cancel = Arc::new(AtomicBool::new(false));
        let err = stream_chat(
            "127.0.0.1",
            port,
            "m",
            &[ChatMessage {
                role: ChatRole::User,
                content: "hi".into(),
            }],
            cancel,
            |_| {},
            Duration::from_millis(500),
            Instant::now() + Duration::from_secs(5),
        )
        .expect_err("in-stream error frame should error");
        match err {
            ChatError::BadStatus { status, message } => {
                assert_eq!(status, 200);
                assert!(message.contains("crashed"));
            }
            other => panic!("expected BadStatus, got {other:?}"),
        }
    }

    #[test]
    fn stream_chat_eof_without_done_returns_eof_before_done() {
        // Server closes the connection cleanly after a few frames
        // but never sends `done: true`. Reflects a real-world
        // truncation; we treat it as `EofBeforeDone` so the
        // command layer maps it to `ChatFinish::Length`.
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let frames = vec![
            r#"{"model":"m","message":{"role":"assistant","content":"par"},"done":false}"#,
            r#"{"model":"m","message":{"role":"assistant","content":"tial"},"done":false}"#,
        ];
        let response = streaming_response(&frames);
        thread::spawn(move || {
            if let Ok((mut sock, _)) = listener.accept() {
                use crate::providers::http::drain_request;
                drain_request(&mut sock);
                let _ = sock.write_all(&response);
                // Server drops; Connection: close header tells the
                // client to expect EOF.
            }
        });

        let cancel = Arc::new(AtomicBool::new(false));
        let collected = Arc::new(Mutex::new(String::new()));
        let collected_for_cb = collected.clone();
        let outcome = stream_chat(
            "127.0.0.1",
            port,
            "m",
            &[ChatMessage {
                role: ChatRole::User,
                content: "hi".into(),
            }],
            cancel,
            move |delta| collected_for_cb.lock().unwrap().push_str(delta),
            Duration::from_millis(500),
            Instant::now() + Duration::from_secs(5),
        )
        .expect("stream_chat should return Ok(EofBeforeDone), not error");
        match outcome {
            StreamOutcome::EofBeforeDone { model_id } => {
                assert_eq!(model_id.as_deref(), Some("m"))
            }
            other => panic!("expected EofBeforeDone, got {other:?}"),
        }
        assert_eq!(collected.lock().unwrap().as_str(), "partial");
    }
}
