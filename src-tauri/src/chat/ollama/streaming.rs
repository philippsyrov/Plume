//! Streaming Ollama `/api/chat` adapter. Handles connection setup,
//! NDJSON framing, cooperative cancel, and the final-frame telemetry
//! the command layer needs.

use std::io::{self, BufReader, Write};
use std::net::{SocketAddr, TcpStream};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::Deserialize;

use super::super::stream_read::{read_line_bounded, ReadOutcome};
use super::super::ChatMessage;
use super::http::{drain_body_to_string, extract_error_message, read_response_head, CHAT_PATH};
use super::request::build_request_body_streaming_with_images;
use super::{ChatError, OllamaFrameStats, StreamOutcome};

/// Poll interval for the streaming read loop. The cancel flag is
/// re-checked at most every `STREAM_READ_POLL`, so this trades
/// responsiveness against CPU. 200 ms is fast enough for a human
/// clicking Stop and slow enough that the loop is idle the vast
/// majority of its life. Documented in
/// `docs/IPC_CONTRACT.md § chat`.
const STREAM_READ_POLL: Duration = Duration::from_millis(200);

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
    on_delta: F,
    connect_timeout: Duration,
    overall_deadline: Instant,
) -> Result<StreamOutcome, ChatError>
where
    F: FnMut(&str),
{
    stream_chat_with_images(
        host,
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

#[allow(clippy::too_many_arguments)]
pub fn stream_chat_with_images<F>(
    host: &str,
    port: u16,
    model: &str,
    messages: &[ChatMessage],
    images: &[Vec<u8>],
    cancel: Arc<AtomicBool>,
    mut on_delta: F,
    connect_timeout: Duration,
    overall_deadline: Instant,
) -> Result<StreamOutcome, ChatError>
where
    F: FnMut(&str),
{
    let request_body = build_request_body_streaming_with_images(model, messages, images);

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

// The bounded line reader and the timeout-kind classifier live in
// `chat::stream_read` (Thermos audit L1) — shared with the MLX SSE
// adapter, which frames its stream the same way. Re-exported so
// `super::http` keeps its original import path.
pub(super) use super::super::stream_read::is_timeout_kind;

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

#[cfg(test)]
#[path = "streaming_tests.rs"]
mod tests;
