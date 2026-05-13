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

use std::io;

mod blocking;
mod http;
mod streaming;

#[cfg(test)]
#[allow(unused_imports)]
pub use blocking::send_chat;
pub use streaming::stream_chat;

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
