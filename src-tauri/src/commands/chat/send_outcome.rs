//! Outcome → `chat.done` translation for `chat.send` streams.
//! Extracted from `send.rs` (D120): the per-adapter outcome-to-event
//! mappers, the Ollama stats math, and both user-facing error
//! formatters. `run_stream` dispatches into the two `*_outcome_to_done`
//! entry points; everything else here is their support.

use std::time::Instant;

use crate::chat::mlx_lm as mlx_chat;
use crate::chat::ollama::{ChatError, OllamaFrameStats, StreamOutcome};
use crate::chat::{ChatDoneEvent, ChatFinish, ChatStats};

pub(super) fn ollama_outcome_to_done(
    outcome: Result<StreamOutcome, ChatError>,
    stream_id: &str,
    provider_id: &str,
    model_id: &str,
    seq_counter: &std::sync::atomic::AtomicU64,
    started: Instant,
) -> ChatDoneEvent {
    let duration_ms = started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64;
    let seq = seq_counter.load(std::sync::atomic::Ordering::Relaxed);
    match outcome {
        Ok(StreamOutcome::Done {
            model_id: served,
            stats,
        }) => ChatDoneEvent {
            id: stream_id.to_string(),
            seq,
            finish: ChatFinish::Stop,
            model_id: Some(served),
            duration_ms,
            error: None,
            stats: Some(translate_stats(&stats)),
        },
        Ok(StreamOutcome::Cancelled { model_id: served }) => ChatDoneEvent {
            id: stream_id.to_string(),
            seq,
            finish: ChatFinish::Cancelled,
            model_id: served.or_else(|| Some(model_id.to_string())),
            duration_ms,
            error: None,
            // D9: no authoritative metrics on cancel — Ollama only
            // emits eval_count / duration in the final frame, and
            // cancellation closes the socket before that lands.
            stats: None,
        },
        Ok(StreamOutcome::EofBeforeDone { model_id: served }) => ChatDoneEvent {
            id: stream_id.to_string(),
            seq,
            finish: ChatFinish::Length,
            model_id: served.or_else(|| Some(model_id.to_string())),
            duration_ms,
            error: None,
            stats: None,
        },
        Err(err) => {
            tracing::debug!(
                provider = %provider_id, model = %model_id, error = %err,
                "chat stream errored"
            );
            ChatDoneEvent {
                id: stream_id.to_string(),
                seq,
                finish: ChatFinish::Error,
                model_id: Some(model_id.to_string()),
                duration_ms,
                error: Some(format_chat_error(&err)),
                stats: None,
            }
        }
    }
}

/// D45: map an `mlx_chat::stream_chat` result onto the same
/// `ChatDoneEvent` shape. MLX-LM only reports `prompt_tokens` and
/// `completion_tokens` in its OpenAI-shape usage chunk — there are
/// no per-phase durations on the wire — so `eval_ms`, `prompt_ms`,
/// and `tokens_per_second` stay `None`. The frontend already
/// hides missing fields in the chat footer.
pub(super) fn mlx_outcome_to_done(
    outcome: Result<mlx_chat::StreamOutcome, mlx_chat::ChatError>,
    stream_id: &str,
    provider_id: &str,
    model_id: &str,
    seq_counter: &std::sync::atomic::AtomicU64,
    started: Instant,
) -> ChatDoneEvent {
    let duration_ms = started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64;
    let seq = seq_counter.load(std::sync::atomic::Ordering::Relaxed);
    match outcome {
        Ok(mlx_chat::StreamOutcome::Done {
            model_id: served,
            stats,
        }) => ChatDoneEvent {
            id: stream_id.to_string(),
            seq,
            finish: ChatFinish::Stop,
            model_id: Some(served),
            duration_ms,
            error: None,
            stats: Some(ChatStats {
                output_tokens: stats.completion_tokens,
                eval_ms: None,
                tokens_per_second: None,
                prompt_tokens: stats.prompt_tokens,
                prompt_ms: None,
            }),
        },
        Ok(mlx_chat::StreamOutcome::Cancelled { model_id: served }) => ChatDoneEvent {
            id: stream_id.to_string(),
            seq,
            finish: ChatFinish::Cancelled,
            model_id: served.or_else(|| Some(model_id.to_string())),
            duration_ms,
            error: None,
            stats: None,
        },
        Ok(mlx_chat::StreamOutcome::EofBeforeDone { model_id: served }) => ChatDoneEvent {
            id: stream_id.to_string(),
            seq,
            finish: ChatFinish::Length,
            model_id: served.or_else(|| Some(model_id.to_string())),
            duration_ms,
            error: None,
            stats: None,
        },
        Err(err) => {
            tracing::debug!(
                provider = %provider_id, model = %model_id, error = %err,
                "chat stream errored"
            );
            ChatDoneEvent {
                id: stream_id.to_string(),
                seq,
                finish: ChatFinish::Error,
                model_id: Some(model_id.to_string()),
                duration_ms,
                error: Some(format_mlx_chat_error(&err)),
                stats: None,
            }
        }
    }
}

pub(super) fn format_mlx_chat_error(err: &mlx_chat::ChatError) -> String {
    match err {
        mlx_chat::ChatError::Transport { port, source } => {
            format!("could not reach mlx-lm at 127.0.0.1:{port} ({source})")
        }
        mlx_chat::ChatError::ModelNotFound { model, message } => {
            format!("model '{model}' not found at mlx-lm: {message}")
        }
        mlx_chat::ChatError::BadStatus { status, message } => {
            format!("mlx-lm returned HTTP {status}: {message}")
        }
        mlx_chat::ChatError::Parse(msg) => format!("mlx-lm SSE did not parse: {msg}"),
    }
}

/// Convert the Ollama-shaped raw counts + nanosecond durations into
/// the provider-neutral `ChatStats` shape that rides on
/// `chat.done`. Durations land in milliseconds because that's the
/// granularity the UI renders and the smoke harness asserts on.
///
/// `tokens_per_second` is computed here (not in the frontend) for
/// two reasons:
///   * the formula is the same regardless of provider; centralising
///     it keeps a future LM Studio adapter consistent;
///   * it avoids the frontend doing `f32` math on every render and
///     having to handle the zero-duration edge case in TS.
///
/// Tests verify the conversion is faithful (1 s of generation, 18
/// tokens → 18.0 tok/s; zero eval_duration → `None`).
pub(super) fn translate_stats(stats: &OllamaFrameStats) -> ChatStats {
    let eval_ms = stats.eval_duration_ns.map(ns_to_ms);
    let prompt_ms = stats.prompt_eval_duration_ns.map(ns_to_ms);
    let tokens_per_second = compute_tokens_per_second(stats.eval_count, stats.eval_duration_ns);
    ChatStats {
        output_tokens: stats.eval_count,
        eval_ms,
        tokens_per_second,
        prompt_tokens: stats.prompt_eval_count,
        prompt_ms,
    }
}

/// Saturating nanosecond → millisecond conversion. We pick
/// saturate-on-overflow because a 64-bit nanosecond count tops out
/// around 585 years of generation; if we ever see one of those
/// numbers it's a bug, and clamping it stays inside the wire's
/// `u64` rather than panicking. Sub-millisecond evaluations round
/// down to zero, which the UI then surfaces as "0 ms" — honest
/// about the read.
pub(super) fn ns_to_ms(ns: u64) -> u64 {
    ns / 1_000_000
}

/// `tokens / seconds` from the same two integers Ollama emits.
/// Returns `None` when either is absent or the duration is zero —
/// the caller (and the smoke check) interprets that as "throughput
/// not measurable", which is more truthful than reporting infinity.
pub(super) fn compute_tokens_per_second(
    tokens: Option<u64>,
    duration_ns: Option<u64>,
) -> Option<f32> {
    let tokens = tokens?;
    let duration_ns = duration_ns?;
    if duration_ns == 0 {
        return None;
    }
    let seconds = (duration_ns as f64) / 1_000_000_000.0;
    Some((tokens as f64 / seconds) as f32)
}

/// Surface a user-facing message for `ChatError`. The streaming
/// adapter's error types are also reachable through the legacy
/// `send_chat` path in tests; we keep this mapping in one place.
pub(super) fn format_chat_error(err: &ChatError) -> String {
    match err {
        ChatError::Transport { host, port, source } => {
            format!("could not reach ollama at {host}:{port} ({source})")
        }
        ChatError::ModelNotFound { model, message } => {
            format!("model '{model}' not found at ollama: {message}")
        }
        ChatError::BadStatus { status, message } => {
            format!("ollama returned HTTP {status}: {message}")
        }
        ChatError::Parse(msg) => format!("ollama response did not parse: {msg}"),
    }
}
