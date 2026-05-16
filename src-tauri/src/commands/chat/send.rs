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

use crate::chat::mlx_lm as mlx_chat;
use crate::chat::ollama::{self, ChatError, OllamaFrameStats, StreamOutcome};
use crate::chat::stream::ChatStreamRegistry;
use crate::chat::{ChatDoneEvent, ChatFinish, ChatMessage, ChatStats, ChatTokenEvent};
use crate::commands::project::AppState;
use crate::error::{IpcError, IpcRequest};
use crate::prompts::{assemble, ChatMode};
use crate::providers::mlx_lm::{self as mlx_supervisor, ServerHandleId};

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
    /// D45 (optional): server handle id from
    /// `providers.startServer`. Required when `providerId ==
    /// "mlx-lm"` — the backend uses it to look up the port the
    /// Plume-managed MLX server bound to. Ignored for any other
    /// provider so an over-eager frontend can pass it harmlessly.
    /// `None` for Ollama; today's UI omits the field there.
    #[serde(default)]
    pub handle_id: Option<String>,
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
    /// D42: summary of the project-memory fold-in, when any
    /// entries rode along on this send. `None` when no trusted
    /// project is open, the store is empty, the store was
    /// unreadable, or every entry was dropped under the byte cap
    /// (the cap is enforced in `prompts::assemble`). The frontend
    /// renders a "Memory · N entries · K bytes" badge based on
    /// `Some(...)`.
    pub memory: Option<ChatSendMemorySummary>,
}

/// D42: wire shape for the project-memory summary echoed on
/// `chat.send`. Field names mirror the `chat.context` preview
/// shape (`MemoryContextPreview`) so the frontend can reuse one
/// renderer for both call sites.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatSendMemorySummary {
    pub entry_count: u64,
    pub bytes: u64,
    pub byte_cap: u64,
    pub truncated: bool,
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

    // D45: route by provider id. Ollama is the legacy path; MLX-LM
    // arrives via the D40 supervisor and needs a port lookup to
    // resolve `handleId`. LM Studio and llama.cpp will share the
    // MLX/OpenAI-compatible adapter when their chat path lands.
    let route = resolve_route(&payload)?;

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
    if let Some(summary) = assembled.memory.as_ref() {
        tracing::debug!(
            entry_count = summary.entry_count,
            used_bytes = summary.used_bytes,
            byte_cap = summary.byte_cap,
            truncated = summary.truncated,
            "chat.send included project memory"
        );
    }
    let instructions_included = assembled.instructions.is_some();
    let memory = assembled.memory.as_ref().map(|s| ChatSendMemorySummary {
        // `usize` → `u64` is widening on every supported target;
        // cast is safe.
        entry_count: s.entry_count as u64,
        bytes: s.used_bytes as u64,
        byte_cap: s.byte_cap as u64,
        truncated: s.truncated,
    });
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
    let route_for_task = route;

    tauri::async_runtime::spawn_blocking(move || {
        run_stream(
            app_for_task,
            registry_handle,
            stream_id_for_task,
            provider_id_for_task,
            model_id_for_task,
            messages_for_task,
            cancel,
            route_for_task,
        );
    });

    Ok(ChatSendStartedResponse {
        stream_id: payload.stream_id,
        provider_id: payload.provider_id,
        model_id: payload.model_id,
        instructions_included,
        memory,
    })
}

/// D45: which adapter to route this send through. Resolved at the
/// command-handler boundary so `run_stream` has a single match instead
/// of re-parsing the provider id mid-stream. `Mlx { port }` carries
/// the bound port from the D40 supervisor's registry — the lookup
/// happens at handler time so a stale `handleId` rejects synchronously
/// with `NotFound`, not as a mid-stream transport error.
#[derive(Debug, Clone, Copy)]
enum ChatRoute {
    Ollama,
    Mlx { port: u16 },
}

/// Resolve the provider id (and optional `handleId`) onto a
/// `ChatRoute`. Three honest outcomes:
///
///   * `"ollama"` — legacy path, no `handleId` required.
///   * `"mlx-lm"` — D40-supervised path. Requires a non-empty
///     `handleId` and a live entry in
///     `providers::mlx_lm::lookup_port`. A stale or missing handle
///     returns `IpcError::NotFound` so the frontend can prompt the
///     user to start (or restart) the server.
///   * anything else — `BadArgument`. LM Studio and llama.cpp share
///     the OpenAI-compatible adapter once their chat path lands;
///     today the rejection is honest about not being wired up.
fn resolve_route(payload: &ChatSendPayload) -> Result<ChatRoute, IpcError> {
    match payload.provider_id.as_str() {
        "ollama" => Ok(ChatRoute::Ollama),
        "mlx-lm" => {
            let raw = payload.handle_id.as_deref().unwrap_or("").trim();
            if raw.is_empty() {
                return Err(IpcError::BadArgument(
                    "chat.send: provider 'mlx-lm' requires handleId — call providers.startServer first".into(),
                ));
            }
            let id = ServerHandleId(raw.to_string());
            match mlx_supervisor::lookup_port(&id) {
                Some(port) => Ok(ChatRoute::Mlx { port }),
                None => Err(IpcError::NotFound(format!(
                    "chat.send: no live MLX server with handleId '{raw}'; call providers.startServer and pass the returned id"
                ))),
            }
        }
        other => Err(IpcError::BadArgument(format!(
            "provider '{other}' has no chat adapter yet — only 'ollama' and 'mlx-lm' are wired"
        ))),
    }
}

/// Drive the streaming loop, emitting `chat.token` events per delta
/// and exactly one terminal `chat.done` event. Always cleans up the
/// registry entry on exit so the stream id is reusable / no longer
/// targetable by `chat.cancel`.
///
/// Runs on the blocking thread pool because the underlying TCP
/// reader is sync.
#[allow(clippy::too_many_arguments)]
fn run_stream(
    app: AppHandle,
    registry: std::sync::Arc<ChatStreamRegistry>,
    stream_id: String,
    provider_id: String,
    model_id: String,
    messages: Vec<ChatMessage>,
    cancel: Arc<AtomicBool>,
    route: ChatRoute,
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

    // Each adapter returns its own outcome / error shape; map both
    // into the common `chat.done` event here so the rest of the
    // function doesn't branch.
    let done = match route {
        ChatRoute::Ollama => {
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
            ollama_outcome_to_done(
                outcome,
                &stream_id,
                &provider_id,
                &model_id,
                &seq_counter,
                started,
            )
        }
        ChatRoute::Mlx { port } => {
            let outcome = mlx_chat::stream_chat(
                port,
                &model_id,
                &messages,
                cancel,
                emit_token,
                CONNECT_TIMEOUT,
                deadline,
            );
            mlx_outcome_to_done(
                outcome,
                &stream_id,
                &provider_id,
                &model_id,
                &seq_counter,
                started,
            )
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

fn ollama_outcome_to_done(
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
fn mlx_outcome_to_done(
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

fn format_mlx_chat_error(err: &mlx_chat::ChatError) -> String {
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

    // ---- D45: routing dispatch ----

    fn payload_for_route(provider: &str, handle_id: Option<&str>) -> ChatSendPayload {
        ChatSendPayload {
            stream_id: "s".into(),
            provider_id: provider.into(),
            model_id: "m".into(),
            messages: vec![ChatMessage {
                role: crate::chat::ChatRole::User,
                content: "hi".into(),
            }],
            handle_id: handle_id.map(str::to_string),
            attachment: None,
            mode: ChatMode::Chat,
        }
    }

    #[test]
    fn resolve_route_picks_ollama_for_ollama_provider() {
        let route = resolve_route(&payload_for_route("ollama", None)).expect("ollama route ok");
        assert!(matches!(route, ChatRoute::Ollama));
    }

    #[test]
    fn resolve_route_for_ollama_ignores_handle_id_even_when_present() {
        // An over-eager frontend that always sends `handleId` should
        // not break the Ollama path. The id is silently ignored
        // there.
        let route = resolve_route(&payload_for_route("ollama", Some("srv_0000000000000001")))
            .expect("ollama with stray handleId");
        assert!(matches!(route, ChatRoute::Ollama));
    }

    #[test]
    fn resolve_route_rejects_mlx_lm_without_handle_id() {
        let err = resolve_route(&payload_for_route("mlx-lm", None))
            .expect_err("mlx-lm without handleId must reject");
        match err {
            IpcError::BadArgument(s) => {
                assert!(s.contains("handleId"), "msg was: {s}");
                assert!(s.contains("providers.startServer"), "msg was: {s}");
            }
            other => panic!("expected BadArgument, got {other:?}"),
        }
    }

    #[test]
    fn resolve_route_rejects_mlx_lm_with_blank_handle_id() {
        let err = resolve_route(&payload_for_route("mlx-lm", Some("   ")))
            .expect_err("blank handleId must reject");
        match err {
            IpcError::BadArgument(s) => assert!(s.contains("handleId")),
            other => panic!("expected BadArgument, got {other:?}"),
        }
    }

    #[test]
    fn resolve_route_rejects_unknown_handle_id_with_not_found() {
        // A handle id that's well-formed but not in the supervisor
        // registry surfaces as NotFound. The frontend uses the same
        // error to drive "start the server again" — a typed
        // distinction from BadArgument.
        let err = resolve_route(&payload_for_route("mlx-lm", Some("srv_ffffffffffffffff")))
            .expect_err("unknown handle must reject");
        match err {
            IpcError::NotFound(s) => {
                assert!(s.contains("MLX server"), "msg was: {s}");
                assert!(s.contains("srv_ffffffffffffffff"), "msg was: {s}");
            }
            other => panic!("expected NotFound, got {other:?}"),
        }
    }

    #[test]
    fn resolve_route_rejects_unknown_provider() {
        let err = resolve_route(&payload_for_route("nope", None))
            .expect_err("unknown provider must reject");
        match err {
            IpcError::BadArgument(s) => {
                assert!(s.contains("nope"), "msg was: {s}");
                assert!(s.contains("mlx-lm"), "msg was: {s}");
            }
            other => panic!("expected BadArgument, got {other:?}"),
        }
    }

    #[test]
    fn chat_send_payload_defaults_handle_id_to_none() {
        // Backward compat: an older Ollama payload that doesn't
        // include `handleId` must still deserialize.
        let json = r#"{
            "streamId": "s",
            "providerId": "ollama",
            "modelId": "llama3",
            "messages": [{"role":"user","content":"hi"}]
        }"#;
        let p: ChatSendPayload = serde_json::from_str(json).expect("must parse");
        assert!(p.handle_id.is_none());
    }

    #[test]
    fn chat_send_payload_accepts_handle_id_in_camel_case() {
        let json = r#"{
            "streamId": "s",
            "providerId": "mlx-lm",
            "modelId": "gemma-2b",
            "handleId": "srv_0000000000000001",
            "messages": [{"role":"user","content":"hi"}]
        }"#;
        let p: ChatSendPayload = serde_json::from_str(json).expect("must parse");
        assert_eq!(p.handle_id.as_deref(), Some("srv_0000000000000001"));
    }

    #[test]
    fn mlx_outcome_to_done_carries_through_completion_and_prompt_tokens() {
        // D45 stats translation: only the OpenAI-shape fields land.
        // eval_ms / prompt_ms / tokens_per_second stay None because
        // MLX-LM's usage chunk doesn't carry per-phase durations.
        let outcome: Result<mlx_chat::StreamOutcome, mlx_chat::ChatError> =
            Ok(mlx_chat::StreamOutcome::Done {
                model_id: "gemma-2b".into(),
                stats: mlx_chat::MlxFrameStats {
                    prompt_tokens: Some(42),
                    completion_tokens: Some(7),
                },
            });
        let seq = std::sync::atomic::AtomicU64::new(3);
        let started = Instant::now();
        let done = mlx_outcome_to_done(outcome, "s", "mlx-lm", "gemma-2b", &seq, started);
        assert!(matches!(done.finish, ChatFinish::Stop));
        assert_eq!(done.model_id.as_deref(), Some("gemma-2b"));
        let stats = done.stats.expect("stats present on Stop");
        assert_eq!(stats.prompt_tokens, Some(42));
        assert_eq!(stats.output_tokens, Some(7));
        assert!(stats.eval_ms.is_none());
        assert!(stats.prompt_ms.is_none());
        assert!(stats.tokens_per_second.is_none());
    }

    #[test]
    fn mlx_outcome_to_done_maps_eof_to_length_finish() {
        let outcome = Ok(mlx_chat::StreamOutcome::EofBeforeDone { model_id: None });
        let seq = std::sync::atomic::AtomicU64::new(1);
        let done = mlx_outcome_to_done(outcome, "s", "mlx-lm", "gemma-2b", &seq, Instant::now());
        assert!(matches!(done.finish, ChatFinish::Length));
        // Falls back to the request's model id when the adapter
        // didn't observe one.
        assert_eq!(done.model_id.as_deref(), Some("gemma-2b"));
        assert!(done.stats.is_none());
    }

    #[test]
    fn mlx_outcome_to_done_maps_cancelled_to_cancelled_finish() {
        let outcome = Ok(mlx_chat::StreamOutcome::Cancelled {
            model_id: Some("served-id".into()),
        });
        let seq = std::sync::atomic::AtomicU64::new(0);
        let done = mlx_outcome_to_done(outcome, "s", "mlx-lm", "gemma-2b", &seq, Instant::now());
        assert!(matches!(done.finish, ChatFinish::Cancelled));
        assert_eq!(done.model_id.as_deref(), Some("served-id"));
        assert!(done.stats.is_none());
    }

    #[test]
    fn mlx_outcome_to_done_maps_transport_error_with_useful_message() {
        let err = mlx_chat::ChatError::Transport {
            port: 9999,
            source: std::io::Error::new(std::io::ErrorKind::ConnectionRefused, "refused"),
        };
        let seq = std::sync::atomic::AtomicU64::new(0);
        let done = mlx_outcome_to_done(Err(err), "s", "mlx-lm", "gemma-2b", &seq, Instant::now());
        assert!(matches!(done.finish, ChatFinish::Error));
        let msg = done.error.expect("error message");
        assert!(msg.contains("mlx-lm"), "msg was: {msg}");
        assert!(msg.contains("9999"), "msg was: {msg}");
    }

    #[test]
    fn format_mlx_chat_error_carries_through_messages() {
        let e = mlx_chat::ChatError::ModelNotFound {
            model: "ghost".into(),
            message: "not loaded".into(),
        };
        let s = format_mlx_chat_error(&e);
        assert!(s.contains("ghost"));
        assert!(s.contains("not loaded"));
    }
}
