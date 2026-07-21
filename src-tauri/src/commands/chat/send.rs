//! `chat.send` Tauri command handler + streaming task.
//!
//! `chat.send` accepts a client-minted `streamId`, validates the
//! payload, spawns the streaming task on the blocking pool, and
//! returns the same id back. The assistant reply arrives over Tauri
//! events (`chat/token` per delta, terminal `chat/done`). The
//! outcome → `chat/done` translation (including the Ollama-stats →
//! wire-stats math) lives in the `send_outcome.rs` sibling (D120);
//! provider routing lives in `send_route.rs` (D118).

use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Instant;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::chat::apple_foundation as apple_chat;
use crate::chat::mlx_lm as mlx_chat;
use crate::chat::ollama;
use crate::chat::stream::ChatStreamRegistry;
use crate::chat::{ChatMessage, ChatTokenEvent};
use crate::commands::project::AppState;
use crate::error::{IpcError, IpcRequest};
use crate::prompts::{
    assemble_with_context_and_stores, ChatMode, ContextSourceManifestItem, ContextSourceRef,
    ExplicitContextStores,
};
use crate::providers::apple_foundation::{platform_supports_apple_models, NativeHelperPort};
use crate::providers::catalog::{QWEN2_VL_CATALOG_ID, QWEN_CATALOG_ID};

use super::validate::validate_payload;
use super::vision::require_screenshot_support;
use super::{
    attachment_to_request, check_attachment_requires_trust, optional_trusted_open,
    validate_context_owner, AttachmentPayload, ChatContextOwner, ChatMemoryContextEntry,
    ChatTopicContextFile, CHAT_DONE_EVENT, CHAT_OVERALL_BUDGET, CHAT_TOKEN_EVENT, CONNECT_TIMEOUT,
    OLLAMA_HOST, OLLAMA_PORT,
};

// D118: provider routing lives in a sibling file. Bare `use` (not
// `pub use`) keeps `ChatRoute` / `resolve_route` at their original
// module-private visibility — the test child sees them through
// `use super::*;` exactly as before.
#[path = "send_route.rs"]
mod route;
#[cfg(test)]
use route::validate_apple_route;
use route::{resolve_route, ChatRoute};

// D120: outcome → chat.done translation (stats math + error
// formatting) lives in a sibling file. Only the three entry points
// run_stream dispatches through are re-imported here; the helpers
// they wrap are reached directly by send_tests.rs, keeping this
// import list honest in non-test builds.
#[path = "send_outcome.rs"]
mod outcome;
use outcome::{apple_outcome_to_done, mlx_outcome_to_done, ollama_outcome_to_done};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatSendPayload {
    /// Client-minted opaque stream id. Lets the frontend subscribe
    /// to `chat/token` / `chat/done` events BEFORE calling
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
    /// Ordered explicit references. Content is resolved in Rust at send time.
    #[serde(default)]
    pub context_sources: Vec<ContextSourceRef>,
    /// Exact persisted chat that owns explicit Browser evidence.
    #[serde(default)]
    pub context_owner: Option<ChatContextOwner>,
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
    /// Defaults to true. No-project chat passes false so a previously
    /// trusted project cannot contribute AGENTS.md, memory, topics, or
    /// attachment context to a plain local chat.
    #[serde(default = "default_include_project_context")]
    pub include_project_context: bool,
}

fn default_include_project_context() -> bool {
    true
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatSendStartedResponse {
    /// Echoes the client-minted stream id. Returned for convenience
    /// so the caller doesn't have to thread its own value back into
    /// state — the IPC return signals "you're cleared to await the
    /// terminal `chat/done`".
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
    /// D72: summary of the curated topic-file fold-in (INDEX/USER/
    /// SOUL), when any rode along on this send. `None` on the same
    /// honest skips as `memory`.
    pub topics: Option<ChatSendTopicsSummary>,
    /// Exact ordered explicit sources that reached the bounded prompt.
    pub context_sources: Vec<ContextSourceManifestItem>,
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
    pub entries: Vec<ChatMemoryContextEntry>,
}

/// D72: wire shape for the curated topic-file summary echoed on
/// `chat.send`. Field names mirror `ChatContextTopicsPreview`.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatSendTopicsSummary {
    pub file_count: u64,
    pub bytes: u64,
    pub byte_cap: u64,
    pub truncated: bool,
    pub files: Vec<ChatTopicContextFile>,
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
    let assembled = prepare_chat_send_context(&payload, &state)?;
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
    if let Some(summary) = assembled.topics.as_ref() {
        tracing::debug!(
            file_count = summary.file_count,
            used_bytes = summary.used_bytes,
            byte_cap = summary.byte_cap,
            truncated = summary.truncated,
            "chat.send included topic files"
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
        entries: s
            .entries
            .iter()
            .map(|entry| ChatMemoryContextEntry {
                id: entry.id.clone(),
                created_at_ms: entry.created_at_ms,
                text_bytes: entry.text_bytes as u64,
                preview: entry.preview.clone(),
            })
            .collect(),
    });
    let topics = assembled.topics.as_ref().map(|s| ChatSendTopicsSummary {
        file_count: s.file_count as u64,
        bytes: s.used_bytes as u64,
        byte_cap: s.byte_cap as u64,
        truncated: s.truncated,
        files: s
            .files
            .iter()
            .map(|file| ChatTopicContextFile {
                name: file.name.clone(),
                bytes: file.bytes as u64,
            })
            .collect(),
    });
    let context_sources = assembled.explicit_context.clone();
    let assembled_messages = assembled.messages;
    let assembled_images = assembled.images;

    if !assembled_images.is_empty() {
        require_screenshot_support(Some(&payload.provider_id), Some(&payload.model_id)).await?;
    }

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
    let images_for_task = assembled_images;
    let route_for_task = route;

    tauri::async_runtime::spawn_blocking(move || {
        run_stream(
            app_for_task,
            registry_handle,
            stream_id_for_task,
            provider_id_for_task,
            model_id_for_task,
            messages_for_task,
            images_for_task,
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
        topics,
        context_sources,
    })
}

/// Resolve every prompt-context input before a stream id is registered.
/// Kept separate from transport so command-level ownership and scope
/// regressions can exercise the exact production preflight without
/// starting a provider or constructing a Tauri `AppHandle`.
fn prepare_chat_send_context(
    payload: &ChatSendPayload,
    state: &AppState,
) -> Result<crate::prompts::AssembledPrompt, IpcError> {
    let trusted_open = if payload.include_project_context {
        optional_trusted_open(state)
    } else {
        None
    };
    let local_owner_session = validate_context_owner(
        payload.context_owner.as_ref(),
        payload.include_project_context,
        !payload.context_sources.is_empty(),
        state,
    )?;

    check_attachment_requires_trust(
        payload.attachment.is_some()
            || (!payload.context_sources.is_empty() && local_owner_session.is_none()),
        trusted_open.is_some(),
    )?;

    let attachment_request = payload.attachment.as_ref().map(attachment_to_request);
    let project_root = trusted_open.as_ref().map(|project| project.root.as_path());
    let local_owner = local_owner_session
        .as_deref()
        .map(|session_id| (state.local_sessions_dir.as_path(), session_id));
    assemble_with_context_and_stores(
        ExplicitContextStores {
            project_root,
            user_memory_dir: state.user_memory_dir.as_path(),
            local_browser_owner: local_owner,
        },
        &payload.messages,
        attachment_request,
        &payload.context_sources,
        payload.mode,
    )
}

/// Drive the streaming loop, emitting `chat/token` events per delta
/// and exactly one terminal `chat/done` event. Always cleans up the
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
    images: Vec<crate::prompts::BrowserScreenshotImage>,
    cancel: Arc<AtomicBool>,
    route: ChatRoute,
) {
    let started = Instant::now();
    let deadline = started + CHAT_OVERALL_BUDGET;

    // seq is monotonic for the whole stream. Token events take
    // 0..n, the terminal `chat/done` takes n.
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
                "failed to emit chat/token event"
            );
        }
    };

    // Each adapter returns its own outcome / error shape; map each
    // into the common `chat/done` event here so the rest of the
    // function doesn't branch.
    let done = match route {
        ChatRoute::Ollama => {
            let image_bytes = images
                .into_iter()
                .map(|image| image.png_bytes)
                .collect::<Vec<_>>();
            let outcome = ollama::stream_chat_with_images(
                OLLAMA_HOST,
                OLLAMA_PORT,
                &model_id,
                &messages,
                &image_bytes,
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
        ChatRoute::Mlx {
            port,
            model_label,
            vision,
        } => {
            // D45 Codex HIGH: echo back the supervisor's
            // launched-model label as the OpenAI request's `model`
            // field. The frontend's `payload.modelId` (which gets
            // surfaced in the UI and round-trips through
            // `chat/done.model_id`) is intentionally kept as the
            // pretty inventory id; we only swap to the supervisor
            // label on the wire. The chat/done we emit still uses
            // `model_id` so the UI label doesn't shift to a long
            // path mid-conversation.
            let stop_sequences = if model_id == QWEN_CATALOG_ID || model_id == QWEN2_VL_CATALOG_ID {
                &[mlx_chat::QWEN_CHAT_STOP_SEQUENCE][..]
            } else {
                &[]
            };
            let image_bytes = images
                .into_iter()
                .map(|image| image.png_bytes)
                .collect::<Vec<_>>();
            let outcome = mlx_chat::stream_chat_with_stop_sequences_and_images(
                port,
                &model_label,
                &messages,
                stop_sequences,
                if vision { &image_bytes } else { &[] },
                vision,
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
        ChatRoute::AppleFoundation => {
            // Resource resolution happens inside Rust after the exact prompt
            // assembly path above. There is no PATH fallback and no Qwen
            // fallback: an Apple failure remains an Apple terminal event.
            let outcome = if platform_supports_apple_models() {
                app.path()
                    .resource_dir()
                    .map_err(|error| format!("could not resolve app resources: {error}"))
                    .map(|resources| NativeHelperPort::from_resource_dir(&resources))
                    .map_err(apple_chat::AppleChatError::Process)
                    .and_then(|helper| {
                        apple_chat::stream_chat_with(
                            &helper, &messages, cancel, emit_token, deadline, true,
                        )
                    })
            } else {
                Err(apple_chat::AppleChatError::OsUnsupported)
            };
            apple_outcome_to_done(
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
            "failed to emit chat/done event"
        );
    }
    registry.finish(&stream_id);
}

#[cfg(test)]
#[path = "send_tests.rs"]
mod tests;
