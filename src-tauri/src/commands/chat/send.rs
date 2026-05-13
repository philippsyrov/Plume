//! `chat.send` Tauri command handler + streaming task + telemetry
//! translation.
//!
//! `chat.send` accepts a client-minted `streamId`, validates the
//! payload, spawns the streaming task on the blocking pool, and
//! returns the same id back. The assistant reply arrives over Tauri
//! events (`chat.token` per delta, terminal `chat.done`). This file
//! also owns the Ollama-stats → wire-stats translation that rides
//! on the final `chat.done` payload.

use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Instant;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};

use crate::chat::ollama::{self, ChatError, OllamaFrameStats, StreamOutcome};
use crate::chat::stream::ChatStreamRegistry;
use crate::chat::{ChatDoneEvent, ChatFinish, ChatMessage, ChatStats, ChatTokenEvent};
use crate::commands::project::AppState;
use crate::error::{IpcError, IpcRequest};
use crate::prompts::{assemble, ChatMode};

use super::validate::validate_payload;
use super::{
    attachment_to_request, check_attachment_requires_trust, optional_trusted_open,
    AttachmentPayload, CHAT_DONE_EVENT, CHAT_OVERALL_BUDGET, CHAT_TOKEN_EVENT, CONNECT_TIMEOUT,
    OLLAMA_HOST, OLLAMA_PORT,
};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatSendPayload {
    /// Client-minted opaque stream id. Lets the frontend subscribe
    /// to `chat.token` / `chat.done` events BEFORE calling
    /// `chat.send`, closing the listener-registration race that
    /// would otherwise drop early tokens. Backend rejects empty,
    /// overlong, or already-in-flight ids with `BadArgument`.
    pub stream_id: String,
    pub provider_id: String,
    pub model_id: String,
    pub messages: Vec<ChatMessage>,
    /// D8 (optional): a single read-only project-file attachment to
    /// fold into the last user message before the stream starts.
    /// When `None` the handler runs the D7.1 text-only path exactly.
    #[serde(default)]
    pub attachment: Option<AttachmentPayload>,
    /// D15 (optional): the response-shape mode for this send.
    /// Defaults to `Chat` (the D7.1 free-form path) when the
    /// field is absent or the value is `"chat"`. `"proposeDiff"`
    /// pins the model to produce a unified-diff preview; the
    /// frontend renders the diff with per-line coloring and shows
    /// a *disabled* Apply button — Plume does NOT apply patches
    /// in D15. New modes are additive; unknown variants reject
    /// with `BadArgument` at the serde layer.
    #[serde(default)]
    pub mode: ChatMode,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatSendStartedResponse {
    /// Echoes the client-minted stream id. Returned for convenience
    /// so the caller doesn't have to thread its own value back into
    /// state — the IPC return signals "you're cleared to await the
    /// terminal `chat.done`".
    pub stream_id: String,
    /// Echoed for routing convenience.
    pub provider_id: String,
    /// Echoed for routing convenience.
    pub model_id: String,
    /// D11: `true` when the project's root `AGENTS.md` was
    /// successfully read and folded in as a system message for
    /// this send. The frontend uses this to confirm its "Project
    /// instructions included" indicator. `false` covers all the
    /// honest reasons we couldn't include them — no trusted
    /// project open, `AGENTS.md` missing / oversize / binary /
    /// unreadable.
    pub instructions_included: bool,
}

#[tauri::command]
pub async fn chat_send(
    req: IpcRequest<ChatSendPayload>,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<ChatSendStartedResponse, IpcError> {
    req.check_version()?;
    let payload = req.payload;

    validate_payload(&payload)?;

    if payload.provider_id != "ollama" {
        // LM Studio and llama.cpp will share an OpenAI-compatible
        // adapter when their chat path lands; today an attempt to
        // chat against them is honest about not being wired up.
        return Err(IpcError::BadArgument(format!(
            "provider '{}' has no chat adapter yet — only 'ollama' is wired",
            payload.provider_id
        )));
    }

    // D8 + D10 + D11: every chat send goes through `prompts::
    // assemble` now. The assembler:
    //   * probes the (trusted) project root for `AGENTS.md` and
    //     prepends it as a system message when present (D11);
    //   * folds the optional file attachment into the last user
    //     message, slicing to a line range if requested (D8+D10);
    //   * returns the final wire transcript plus a summary of
    //     what landed.
    //
    // Attachment errors (`Blocked` for secret-pattern filenames,
    // `NotFound`, `PathEscape`, `BadArgument` for shape, …)
    // surface synchronously here so the frontend never spins up a
    // streaming UI for a request that already failed.
    // Instructions errors do NOT surface — a broken `AGENTS.md`
    // skips silently and `instructions_included` reports `false`.
    let trusted_open = optional_trusted_open(&state);

    // Attachment requires a trusted project the same way `fs.read`
    // does. Reject before reaching the assembler so the
    // `NeedsApproval` message is honest about *why* the send was
    // rejected.
    check_attachment_requires_trust(payload.attachment.is_some(), trusted_open.is_some())?;

    let attachment_request = payload.attachment.as_ref().map(attachment_to_request);
    let project_root = trusted_open.as_ref().map(|p| p.root.as_path());
    let assembled = assemble(
        project_root,
        &payload.messages,
        attachment_request,
        payload.mode,
    )?;
    if let Some(summary) = assembled.attachment.as_ref() {
        let range_label = match summary.line_range {
            Some(r) => format!("{}-{}", r.start, r.end),
            None => "whole-file".to_string(),
        };
        tracing::debug!(
            rel_path = %summary.rel_path,
            original_bytes = summary.original_bytes,
            redactions = summary.redaction_count,
            line_range = %range_label,
            "chat.send attached file"
        );
    }
    if let Some(summary) = assembled.instructions.as_ref() {
        tracing::debug!(
            source = %summary.source,
            original_bytes = summary.original_bytes,
            redactions = summary.redaction_count,
            "chat.send included project instructions"
        );
    }
    let instructions_included = assembled.instructions.is_some();
    let assembled_messages = assembled.messages;

    // Reserve the client-minted id. Failing here means another
    // stream is already live with this id; the frontend should
    // never do that, but a bad caller (or a buggy auto-retry that
    // doesn't realize the previous send is still streaming) gets
    // a typed rejection instead of a silent overwrite.
    let cancel: Arc<AtomicBool> = state
        .chat_streams
        .register(payload.stream_id.clone())
        .ok_or_else(|| {
            IpcError::BadArgument(format!(
                "chat.send: streamId '{}' is already in flight",
                payload.stream_id
            ))
        })?;

    // Clone everything the background task needs. AppHandle is
    // cheap to clone and Send + 'static.
    let app_for_task = app.clone();
    let registry_handle = state.chat_streams.clone();
    let stream_id_for_task = payload.stream_id.clone();
    let provider_id_for_task = payload.provider_id.clone();
    let model_id_for_task = payload.model_id.clone();
    let messages_for_task = assembled_messages;

    tauri::async_runtime::spawn_blocking(move || {
        run_stream(
            app_for_task,
            registry_handle,
            stream_id_for_task,
            provider_id_for_task,
            model_id_for_task,
            messages_for_task,
            cancel,
        );
    });

    Ok(ChatSendStartedResponse {
        stream_id: payload.stream_id,
        provider_id: payload.provider_id,
        model_id: payload.model_id,
        instructions_included,
    })
}

/// Drive the streaming loop, emitting `chat.token` events per delta
/// and exactly one terminal `chat.done` event. Always cleans up the
/// registry entry on exit so the stream id is reusable / no longer
/// targetable by `chat.cancel`.
///
/// Runs on the blocking thread pool because the underlying TCP
/// reader is sync.
fn run_stream(
    app: AppHandle,
    registry: std::sync::Arc<ChatStreamRegistry>,
    stream_id: String,
    provider_id: String,
    model_id: String,
    messages: Vec<ChatMessage>,
    cancel: Arc<AtomicBool>,
) {
    let started = Instant::now();
    let deadline = started + CHAT_OVERALL_BUDGET;

    // seq is monotonic for the whole stream. Token events take
    // 0..n, the terminal `chat.done` takes n.
    let seq_counter = std::sync::atomic::AtomicU64::new(0);
    let emit_token = |delta: &str| {
        let seq = seq_counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let payload = ChatTokenEvent {
            id: stream_id.clone(),
            seq,
            delta: delta.to_string(),
        };
        if let Err(e) = app.emit(CHAT_TOKEN_EVENT, payload) {
            tracing::warn!(
                stream = %stream_id, error = %e,
                "failed to emit chat.token event"
            );
        }
    };

    let outcome = ollama::stream_chat(
        OLLAMA_HOST,
        OLLAMA_PORT,
        &model_id,
        &messages,
        cancel,
        emit_token,
        CONNECT_TIMEOUT,
        deadline,
    );

    let duration_ms = started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64;
    let seq = seq_counter.load(std::sync::atomic::Ordering::Relaxed);
    let done = match outcome {
        Ok(StreamOutcome::Done {
            model_id: served,
            stats,
        }) => ChatDoneEvent {
            id: stream_id.clone(),
            seq,
            finish: ChatFinish::Stop,
            model_id: Some(served),
            duration_ms,
            error: None,
            stats: Some(translate_stats(&stats)),
        },
        Ok(StreamOutcome::Cancelled { model_id: served }) => ChatDoneEvent {
            id: stream_id.clone(),
            seq,
            finish: ChatFinish::Cancelled,
            model_id: served.or(Some(model_id.clone())),
            duration_ms,
            error: None,
            // D9: no authoritative metrics on cancel — Ollama only
            // emits eval_count / duration in the final frame, and
            // cancellation closes the socket before that lands.
            stats: None,
        },
        Ok(StreamOutcome::EofBeforeDone { model_id: served }) => ChatDoneEvent {
            id: stream_id.clone(),
            seq,
            finish: ChatFinish::Length,
            model_id: served.or(Some(model_id.clone())),
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
                id: stream_id.clone(),
                seq,
                finish: ChatFinish::Error,
                model_id: Some(model_id.clone()),
                duration_ms,
                error: Some(format_chat_error(&err)),
                stats: None,
            }
        }
    };

    if let Err(e) = app.emit(CHAT_DONE_EVENT, done) {
        tracing::warn!(
            stream = %stream_id, error = %e,
            "failed to emit chat.done event"
        );
    }
    registry.finish(&stream_id);
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
fn translate_stats(stats: &OllamaFrameStats) -> ChatStats {
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
fn ns_to_ms(ns: u64) -> u64 {
    ns / 1_000_000
}

/// `tokens / seconds` from the same two integers Ollama emits.
/// Returns `None` when either is absent or the duration is zero —
/// the caller (and the smoke check) interprets that as "throughput
/// not measurable", which is more truthful than reporting infinity.
fn compute_tokens_per_second(tokens: Option<u64>, duration_ns: Option<u64>) -> Option<f32> {
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
fn format_chat_error(err: &ChatError) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_chat_error_carries_through_messages() {
        let e = ChatError::ModelNotFound {
            model: "ghost".into(),
            message: "not pulled".into(),
        };
        let s = format_chat_error(&e);
        assert!(s.contains("ghost"));
        assert!(s.contains("not pulled"));
    }

    // ---- D9 generation telemetry ----

    #[test]
    fn translate_stats_passes_counts_and_converts_durations_to_ms() {
        // 18 output tokens generated in exactly 1 s → 18 tok/s.
        // 12 prompt tokens evaluated in 100 ms → prompt_ms == 100.
        let raw = OllamaFrameStats {
            eval_count: Some(18),
            eval_duration_ns: Some(1_000_000_000),
            prompt_eval_count: Some(12),
            prompt_eval_duration_ns: Some(100_000_000),
        };
        let stats = translate_stats(&raw);
        assert_eq!(stats.output_tokens, Some(18));
        assert_eq!(stats.eval_ms, Some(1_000));
        assert_eq!(stats.prompt_tokens, Some(12));
        assert_eq!(stats.prompt_ms, Some(100));
        assert_eq!(stats.tokens_per_second, Some(18.0));
    }

    #[test]
    fn translate_stats_returns_none_fields_when_inputs_absent() {
        // A frame with no telemetry fields produces a stats value
        // where every output is None — the UI hides the footer in
        // that case.
        let stats = translate_stats(&OllamaFrameStats::default());
        assert_eq!(stats.output_tokens, None);
        assert_eq!(stats.eval_ms, None);
        assert_eq!(stats.tokens_per_second, None);
        assert_eq!(stats.prompt_tokens, None);
        assert_eq!(stats.prompt_ms, None);
    }

    #[test]
    fn tokens_per_second_is_none_when_eval_duration_is_zero() {
        // Division by zero would produce inf; we prefer honest
        // "throughput not measurable" by returning None.
        assert_eq!(
            compute_tokens_per_second(Some(10), Some(0)),
            None,
            "zero eval_duration must not produce infinity"
        );
    }

    #[test]
    fn tokens_per_second_is_none_when_either_input_is_none() {
        assert_eq!(compute_tokens_per_second(None, Some(1_000_000)), None);
        assert_eq!(compute_tokens_per_second(Some(5), None), None);
    }

    #[test]
    fn ns_to_ms_floors_sub_millisecond_durations() {
        // 999 µs rounds down to 0 ms; the UI surfaces that as
        // "0 ms" rather than fabricating a 1 ms reading.
        assert_eq!(ns_to_ms(999_000), 0);
        assert_eq!(ns_to_ms(1_000_000), 1);
        assert_eq!(ns_to_ms(1_500_000), 1);
    }

    // ---- D15: chat.send mode wire shape ----
    //
    // Pin both directions of the new `mode` field on the wire so
    // a future refactor that drops `#[serde(default)]` (= D7.1
    // payloads break) or `rename_all = "camelCase"` on `ChatMode`
    // (= `proposeDiff` stops parsing) fires a test instead of
    // a Codex smoke flag. The `ChatMode` enum itself is unit-
    // variant so `rename_all` does cascade — D8's struct-variant
    // trap doesn't apply here, but the explicit tests keep the
    // contract auditable.

    #[test]
    fn chat_send_payload_defaults_mode_to_chat_when_omitted() {
        // The wire compatibility win of D15: an existing D7.1
        // frontend that sends no `mode` field still deserialises
        // to a payload where `mode == ChatMode::Chat`. Without
        // the `#[serde(default)]` on the field this would reject
        // and break every older send.
        let json = r#"{
            "streamId": "s",
            "providerId": "ollama",
            "modelId": "llama3",
            "messages": [{"role":"user","content":"hi"}]
        }"#;
        let p: ChatSendPayload =
            serde_json::from_str(json).expect("omitted mode must default to chat");
        assert!(matches!(p.mode, ChatMode::Chat));
    }

    #[test]
    fn chat_send_payload_accepts_explicit_chat_mode() {
        let json = r#"{
            "streamId": "s",
            "providerId": "ollama",
            "modelId": "llama3",
            "messages": [{"role":"user","content":"hi"}],
            "mode": "chat"
        }"#;
        let p: ChatSendPayload = serde_json::from_str(json).expect("must parse");
        assert!(matches!(p.mode, ChatMode::Chat));
    }

    #[test]
    fn chat_send_payload_accepts_propose_diff_mode_in_camel_case() {
        // The exact wire shape `chat.send` sees when the user
        // flips the chat panel to "Propose diff" mode.
        let json = r#"{
            "streamId": "s",
            "providerId": "ollama",
            "modelId": "llama3",
            "messages": [{"role":"user","content":"rename foo"}],
            "mode": "proposeDiff"
        }"#;
        let p: ChatSendPayload =
            serde_json::from_str(json).expect("camelCase proposeDiff must parse");
        assert!(matches!(p.mode, ChatMode::ProposeDiff));
    }

    #[test]
    fn chat_send_payload_rejects_unknown_mode_variant() {
        // Serde rejects on unknown variant before the handler
        // runs, which surfaces as `IpcError::BadArgument` at the
        // Tauri envelope level. A future mode (`'scopedEdit'`,
        // `'agentLoop'`) is opt-in: until the backend knows
        // about it, the frontend gets a clean rejection rather
        // than a silent "mode dropped" send.
        let json = r#"{
            "streamId": "s",
            "providerId": "ollama",
            "modelId": "llama3",
            "messages": [{"role":"user","content":"hi"}],
            "mode": "scopedEdit"
        }"#;
        let err =
            serde_json::from_str::<ChatSendPayload>(json).expect_err("unknown mode must reject");
        let msg = err.to_string();
        assert!(
            msg.contains("variant") || msg.contains("scopedEdit"),
            "expected unknown-variant error, got: {msg}"
        );
    }
}
