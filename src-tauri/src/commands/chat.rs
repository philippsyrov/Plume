//! `chat.send` + `chat.cancel` Tauri command handlers.
//!
//! D7 shipped `chat.send` as a synchronous call that returned the
//! full assistant message. D7.1 reshapes it: `chat.send` accepts a
//! **client-minted** `streamId`, validates it, spawns the streaming
//! task, and returns the same id back. The assistant reply arrives
//! over Tauri events (`chat.token` per delta, terminal
//! `chat.done`). `chat.cancel(streamId)` flips a cooperative cancel
//! flag.
//!
//! Why the client mints the id: Tauri events are not replayed, so
//! any event emitted between the IPC return and the frontend
//! registering its listener would be silently lost. Letting the
//! frontend pick the id means it can subscribe BEFORE calling
//! `chat.send`, closing the race entirely. The backend validates
//! that the id is well-formed and unique among live streams; a
//! duplicate id rejects with `BadArgument`. See
//! `docs/IPC_CONTRACT.md § chat` for the rationale.
//!
//! Validation order (matches the rest of the IPC surface):
//!   1. version
//!   2. payload shape (non-empty streamId within length cap,
//!      non-empty model, non-empty messages, no `Tool` role today,
//!      last message is from the user, every message has non-empty
//!      content). If `attachment` is present, its shape is also
//!      validated here (non-empty relPath, within length cap, no
//!      `..` segments, no leading slash).
//!   3. provider id (Ollama-only today)
//!   4. attachment resolution (D8): if present, require a trusted
//!      open project, then run `prompts::assemble` to fold the
//!      file content into the last user message. Errors here —
//!      `Blocked`, `NotFound`, `PathEscape`, `BadArgument` —
//!      surface synchronously so the frontend never spins a
//!      streaming UI for an attachment that will never read.
//!   5. register the streamId (rejects on duplicate); spawn
//!      streaming task; transport errors surface as
//!      `chat.done { finish: 'error', error }` events, not as a
//!      handler `Result::Err`. Subscribers join via Tauri events.
//!
//! Provider-not-Ollama, payload-shape, attachment, and duplicate-id
//! failures all return their typed `IpcError` synchronously — those
//! are the kinds of errors the frontend should react to before
//! showing any `Sending…` UI.
//!
//! Attachment scope (D8):
//!   * One project-file attachment per send.
//!   * Backend reads via the Rust-private `prompts::assemble`
//!     path. The redactor in `prompts::redact` is the only
//!     producer of `RedactedContent`; raw file bytes never leave
//!     this process.
//!   * No directory attachments, no glob expansion, no recursive
//!     reads, no streaming of multiple files. The frontend's
//!     visible chip is the source of truth for what got sent.
//!
//! What this handler deliberately does NOT do:
//!   - It does not validate the model id against the live
//!     `/api/tags` snapshot. The runtime is the source of truth;
//!     a bad id returns 404 from Ollama mid-call, which we map
//!     onto a typed `chat.done { finish: 'error' }` event.
//!   - It does not auto-start `ollama serve`. Reachability is the
//!     user's responsibility.
//!   - It does not re-canonicalize the project root from a
//!     frontend-supplied field. The canonical root comes from
//!     `ProjectSession` (same rule as `fs.read`).

use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};

use crate::chat::ollama::{self, ChatError, OllamaFrameStats, StreamOutcome};
use crate::chat::stream::ChatStreamRegistry;
use crate::chat::{ChatDoneEvent, ChatFinish, ChatMessage, ChatRole, ChatStats, ChatTokenEvent};
use crate::commands::project::AppState;
use crate::error::{IpcError, IpcRequest};
use crate::project::OpenProject;
use crate::prompts::{assemble, AttachmentRequest, LineRange};

/// Default localhost endpoint for Ollama. Centralizing port
/// overrides is roadmap (`docs/IPC_ROADMAP.md § Provider health`).
const OLLAMA_HOST: &str = "127.0.0.1";
const OLLAMA_PORT: u16 = 11434;

/// Cap on a single chat stream's total wall-clock duration. Five
/// minutes is generous on modest hardware — long enough for a 7 B
/// model on Metal to finish a paragraph, short enough that a stuck
/// daemon doesn't pin the registry slot forever. The streaming loop
/// checks this between line reads.
const CHAT_OVERALL_BUDGET: Duration = Duration::from_secs(300);

/// Connect timeout for the TCP handshake at the start of a stream.
/// This is much shorter than the overall budget because "Ollama is
/// not running" should surface immediately, not after 5 minutes.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

/// Event name for per-frame delta payloads (`ChatTokenEvent`).
const CHAT_TOKEN_EVENT: &str = "chat.token";
/// Event name for the terminal payload (`ChatDoneEvent`). Exactly
/// one of these fires per stream id.
const CHAT_DONE_EVENT: &str = "chat.done";

/// Hard cap on a client-minted stream id. UUID v4 is 36 chars; 128
/// is generous headroom without giving an attacker room to send a
/// large allocation through every chat call.
const MAX_STREAM_ID_LEN: usize = 128;

/// Cap on an attachment's relative-path string. The OS-level
/// `PATH_MAX` is 1024 on macOS and 4096 on Linux; 1024 is a useful
/// floor that catches obvious garbage (a JSON blob in the field)
/// without rejecting a legitimately deep relative path.
const MAX_ATTACHMENT_REL_PATH_LEN: usize = 1024;

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
}

/// Wire shape for the attachment field. Tagged so we can grow to
/// other attachment kinds (recent terminal output, selection-only
/// snippet, …) without a breaking change. The handler maps this
/// onto the internal `prompts::AttachmentRequest`.
///
/// D10 added the optional `startLine` + `endLine` pair on
/// `projectFile`. Both must be present or both absent — half a
/// range is a hard reject. When set, the backend slices the
/// redacted content to those lines (1-based, inclusive) before
/// folding it into the user message. The frontend never sends the
/// selected text itself; the slice happens after the prompt-read.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum AttachmentPayload {
    /// A file at `relPath` inside the currently-open trusted
    /// project root. Backend reads via the Rust-private
    /// `prompts::read::read_for_prompt` path; raw bytes never
    /// reach the frontend.
    #[serde(rename = "projectFile")]
    ProjectFile {
        rel_path: String,
        /// 1-based inclusive start of the requested line range.
        /// Must accompany `end_line`; either both fields are
        /// present or both are absent.
        #[serde(default)]
        start_line: Option<u32>,
        /// 1-based inclusive end of the requested line range.
        #[serde(default)]
        end_line: Option<u32>,
    },
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

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatCancelPayload {
    pub stream_id: String,
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
    let assembled = assemble(project_root, &payload.messages, attachment_request)?;
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

/// Reject `chat.send` with `NeedsApproval` when the caller asks for
/// an attachment but no trusted project is open. Pulled out into a
/// pure function so the trust-gate branch is testable without
/// standing up an `AppState` / `Tauri::State` test fixture.
fn check_attachment_requires_trust(
    has_attachment: bool,
    has_trusted_project: bool,
) -> Result<(), IpcError> {
    if has_attachment && !has_trusted_project {
        return Err(IpcError::NeedsApproval);
    }
    Ok(())
}

/// Returns the currently-open project if one is open AND its
/// canonical root is in the trust store; `None` otherwise.
///
/// D7.1 plain chat doesn't require a project at all; D8 attachments
/// and D11 project instructions both need a trusted project. This
/// helper lets the handler ask the question without committing to
/// rejecting when the project is missing — the caller decides
/// whether `None` is a hard error or a quiet skip.
fn optional_trusted_open(state: &AppState) -> Option<OpenProject> {
    let open = state.session.current()?;
    let trusted = {
        let store = state.trust.lock().expect("trust mutex poisoned");
        store.is_trusted(&open.root)
    };
    if trusted {
        Some(open)
    } else {
        None
    }
}

fn attachment_to_request(att: &AttachmentPayload) -> AttachmentRequest {
    match att {
        AttachmentPayload::ProjectFile {
            rel_path,
            start_line,
            end_line,
        } => {
            // `validate_attachment` already enforced "both or
            // neither" — we don't need to re-check here. Either
            // field being `Some` means both are `Some`.
            let line_range = match (start_line, end_line) {
                (Some(s), Some(e)) => Some(LineRange { start: *s, end: *e }),
                _ => None,
            };
            AttachmentRequest::ProjectFile {
                rel_path: rel_path.clone(),
                line_range,
            }
        }
    }
}

#[tauri::command]
pub async fn chat_cancel(
    req: IpcRequest<ChatCancelPayload>,
    state: State<'_, AppState>,
) -> Result<(), IpcError> {
    req.check_version()?;
    // Idempotent per the contract: cancelling a finished or unknown
    // stream is a successful no-op. The `cancel` return value is
    // only used for tracing here.
    let was_live = state.chat_streams.cancel(&req.payload.stream_id);
    if !was_live {
        tracing::debug!(
            stream = %req.payload.stream_id,
            "chat.cancel: stream id is unknown or already terminal (idempotent no-op)"
        );
    }
    Ok(())
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

/// Reject obviously malformed payloads with `BadArgument` before
/// any network call. Each branch is its own clause so the error
/// string names the failing field.
fn validate_payload(payload: &ChatSendPayload) -> Result<(), IpcError> {
    if payload.stream_id.trim().is_empty() {
        return Err(IpcError::BadArgument(
            "chat.send: streamId is empty".to_string(),
        ));
    }
    if payload.stream_id.len() > MAX_STREAM_ID_LEN {
        return Err(IpcError::BadArgument(format!(
            "chat.send: streamId exceeds {MAX_STREAM_ID_LEN} chars"
        )));
    }
    if payload.provider_id.trim().is_empty() {
        return Err(IpcError::BadArgument(
            "chat.send: providerId is empty".to_string(),
        ));
    }
    if payload.model_id.trim().is_empty() {
        return Err(IpcError::BadArgument(
            "chat.send: modelId is empty — pick a model in the provider panel first".to_string(),
        ));
    }
    if payload.messages.is_empty() {
        return Err(IpcError::BadArgument(
            "chat.send: messages array is empty".to_string(),
        ));
    }
    for (i, m) in payload.messages.iter().enumerate() {
        if m.content.is_empty() {
            return Err(IpcError::BadArgument(format!(
                "chat.send: messages[{i}] has empty content"
            )));
        }
        if matches!(m.role, ChatRole::Tool) {
            return Err(IpcError::BadArgument(format!(
                "chat.send: messages[{i}] uses the 'tool' role, which is not supported yet"
            )));
        }
    }
    let last = payload.messages.last().expect("non-empty checked above");
    if !matches!(last.role, ChatRole::User) {
        return Err(IpcError::BadArgument(
            "chat.send: last message must have role='user'".to_string(),
        ));
    }
    if let Some(att) = payload.attachment.as_ref() {
        validate_attachment(att)?;
    }
    Ok(())
}

/// Reject obviously bad attachment payloads before the handler
/// reaches for the project session. The full path-safety check
/// (canonicalize-then-ensure-inside) runs later in `assemble`; this
/// catches shapes that would never be a legitimate relative path.
fn validate_attachment(att: &AttachmentPayload) -> Result<(), IpcError> {
    match att {
        AttachmentPayload::ProjectFile {
            rel_path,
            start_line,
            end_line,
        } => {
            let trimmed = rel_path.trim();
            if trimmed.is_empty() {
                return Err(IpcError::BadArgument(
                    "chat.send: attachment.relPath is empty".into(),
                ));
            }
            if rel_path.len() > MAX_ATTACHMENT_REL_PATH_LEN {
                return Err(IpcError::BadArgument(format!(
                    "chat.send: attachment.relPath exceeds {MAX_ATTACHMENT_REL_PATH_LEN} chars"
                )));
            }
            // Absolute paths and bare `..` traversal are never legal
            // for a project-relative attachment. `assemble`'s
            // canonicalize-then-ensure-inside would catch escapes
            // too, but rejecting up front gives a clearer error
            // message and avoids reaching for the filesystem at all.
            if rel_path.starts_with('/') || rel_path.starts_with('\\') {
                return Err(IpcError::BadArgument(
                    "chat.send: attachment.relPath must be project-relative, not absolute".into(),
                ));
            }
            for segment in rel_path.split(['/', '\\']) {
                if segment == ".." {
                    return Err(IpcError::BadArgument(
                        "chat.send: attachment.relPath must not contain '..' segments".into(),
                    ));
                }
            }
            // NUL bytes in a path string are a hard reject — they'd
            // either fail filesystem syscalls or be silently
            // truncated on some platforms.
            if rel_path.contains('\0') {
                return Err(IpcError::BadArgument(
                    "chat.send: attachment.relPath contains NUL byte".into(),
                ));
            }
            // D10: line range is all-or-nothing. Half a range
            // (just startLine, or just endLine) is almost certainly
            // a frontend bug; reject so the caller fixes the
            // payload instead of silently treating it as
            // whole-file.
            validate_line_range(*start_line, *end_line)?;
        }
    }
    Ok(())
}

fn validate_line_range(start: Option<u32>, end: Option<u32>) -> Result<(), IpcError> {
    match (start, end) {
        (None, None) => Ok(()),
        (Some(_), None) => Err(IpcError::BadArgument(
            "chat.send: attachment.startLine set without endLine".into(),
        )),
        (None, Some(_)) => Err(IpcError::BadArgument(
            "chat.send: attachment.endLine set without startLine".into(),
        )),
        (Some(s), Some(e)) => {
            if s == 0 {
                return Err(IpcError::BadArgument(
                    "chat.send: attachment.startLine must be >= 1 (lines are 1-based)".into(),
                ));
            }
            if e < s {
                return Err(IpcError::BadArgument(format!(
                    "chat.send: attachment.endLine ({e}) must be >= startLine ({s})"
                )));
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::{ChatMessage, ChatRole};

    fn user_msg(content: &str) -> ChatMessage {
        ChatMessage {
            role: ChatRole::User,
            content: content.to_string(),
        }
    }

    fn assistant_msg(content: &str) -> ChatMessage {
        ChatMessage {
            role: ChatRole::Assistant,
            content: content.to_string(),
        }
    }

    fn ok_payload(messages: Vec<ChatMessage>) -> ChatSendPayload {
        ChatSendPayload {
            stream_id: "stream-test-0001".into(),
            provider_id: "ollama".into(),
            model_id: "llama3".into(),
            messages,
            attachment: None,
        }
    }

    fn payload_with_attachment(
        messages: Vec<ChatMessage>,
        attachment: AttachmentPayload,
    ) -> ChatSendPayload {
        ChatSendPayload {
            stream_id: "stream-test-attach".into(),
            provider_id: "ollama".into(),
            model_id: "llama3".into(),
            messages,
            attachment: Some(attachment),
        }
    }

    fn project_file_attachment(
        rel_path: &str,
        start_line: Option<u32>,
        end_line: Option<u32>,
    ) -> AttachmentPayload {
        AttachmentPayload::ProjectFile {
            rel_path: rel_path.into(),
            start_line,
            end_line,
        }
    }

    #[test]
    fn rejects_empty_stream_id() {
        let mut p = ok_payload(vec![user_msg("hi")]);
        p.stream_id = "   ".into();
        let err = validate_payload(&p).expect_err("blank stream id rejected");
        match err {
            IpcError::BadArgument(s) => assert!(s.contains("streamId")),
            other => panic!("expected BadArgument, got {other:?}"),
        }
    }

    #[test]
    fn rejects_overlong_stream_id() {
        let mut p = ok_payload(vec![user_msg("hi")]);
        p.stream_id = "x".repeat(MAX_STREAM_ID_LEN + 1);
        let err = validate_payload(&p).expect_err("overlong stream id rejected");
        match err {
            IpcError::BadArgument(s) => assert!(s.contains("streamId")),
            other => panic!("expected BadArgument, got {other:?}"),
        }
    }

    #[test]
    fn rejects_empty_model_id() {
        let mut p = ok_payload(vec![user_msg("hi")]);
        p.model_id = "   ".into();
        let err = validate_payload(&p).expect_err("blank model rejected");
        match err {
            IpcError::BadArgument(s) => assert!(s.contains("modelId")),
            other => panic!("expected BadArgument, got {other:?}"),
        }
    }

    #[test]
    fn rejects_empty_messages() {
        let p = ok_payload(vec![]);
        let err = validate_payload(&p).expect_err("empty messages rejected");
        match err {
            IpcError::BadArgument(s) => assert!(s.contains("messages")),
            other => panic!("expected BadArgument, got {other:?}"),
        }
    }

    #[test]
    fn rejects_tool_role_in_v1() {
        let p = ok_payload(vec![ChatMessage {
            role: ChatRole::Tool,
            content: "tool result".into(),
        }]);
        let err = validate_payload(&p).expect_err("tool rejected");
        match err {
            IpcError::BadArgument(s) => assert!(s.contains("tool")),
            other => panic!("expected BadArgument, got {other:?}"),
        }
    }

    #[test]
    fn rejects_when_last_message_is_assistant() {
        let p = ok_payload(vec![user_msg("hi"), assistant_msg("hey")]);
        let err = validate_payload(&p).expect_err("trailing assistant rejected");
        match err {
            IpcError::BadArgument(s) => assert!(s.contains("user")),
            other => panic!("expected BadArgument, got {other:?}"),
        }
    }

    #[test]
    fn accepts_well_formed_history() {
        let p = ok_payload(vec![user_msg("hi"), assistant_msg("hey"), user_msg("more")]);
        validate_payload(&p).expect("should pass");
    }

    #[test]
    fn rejects_empty_content() {
        let p = ok_payload(vec![user_msg("")]);
        let err = validate_payload(&p).expect_err("empty content rejected");
        match err {
            IpcError::BadArgument(s) => assert!(s.contains("content")),
            other => panic!("expected BadArgument, got {other:?}"),
        }
    }

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

    // ---- D8 attachment validation ----

    #[test]
    fn accepts_payload_without_attachment() {
        // Sanity: the new field is optional and the D7.1 shape still
        // passes validation untouched.
        let p = ok_payload(vec![user_msg("hi")]);
        validate_payload(&p).expect("D7.1 payload must still validate");
    }

    #[test]
    fn accepts_well_formed_project_file_attachment() {
        let p = payload_with_attachment(
            vec![user_msg("explain this file")],
            project_file_attachment("src/main.rs", None, None),
        );
        validate_payload(&p).expect("normal attachment must validate");
    }

    #[test]
    fn rejects_empty_attachment_rel_path() {
        let p = payload_with_attachment(
            vec![user_msg("hi")],
            project_file_attachment("   ", None, None),
        );
        let err = validate_payload(&p).expect_err("blank relPath rejected");
        match err {
            IpcError::BadArgument(s) => assert!(s.contains("relPath")),
            other => panic!("expected BadArgument, got {other:?}"),
        }
    }

    #[test]
    fn rejects_overlong_attachment_rel_path() {
        let p = payload_with_attachment(
            vec![user_msg("hi")],
            project_file_attachment(&"a".repeat(MAX_ATTACHMENT_REL_PATH_LEN + 1), None, None),
        );
        let err = validate_payload(&p).expect_err("overlong relPath rejected");
        match err {
            IpcError::BadArgument(s) => assert!(s.contains("relPath")),
            other => panic!("expected BadArgument, got {other:?}"),
        }
    }

    #[test]
    fn rejects_absolute_attachment_rel_path() {
        let p = payload_with_attachment(
            vec![user_msg("hi")],
            project_file_attachment("/etc/passwd", None, None),
        );
        let err = validate_payload(&p).expect_err("absolute path rejected");
        match err {
            IpcError::BadArgument(s) => assert!(s.contains("project-relative")),
            other => panic!("expected BadArgument, got {other:?}"),
        }
    }

    #[test]
    fn rejects_dotdot_traversal_in_attachment_rel_path() {
        // Even with a junk parent the `..` segment is a hard reject.
        let p = payload_with_attachment(
            vec![user_msg("hi")],
            project_file_attachment("src/../../etc/passwd", None, None),
        );
        let err = validate_payload(&p).expect_err("`..` segment rejected");
        match err {
            IpcError::BadArgument(s) => assert!(s.contains("'..'")),
            other => panic!("expected BadArgument, got {other:?}"),
        }
    }

    // ---- D10 line-range payload validation ----

    #[test]
    fn accepts_well_formed_line_range_attachment() {
        let p = payload_with_attachment(
            vec![user_msg("look at lines 12-18")],
            project_file_attachment("src/main.rs", Some(12), Some(18)),
        );
        validate_payload(&p).expect("normal line range must validate");
    }

    #[test]
    fn rejects_partial_line_range_start_only() {
        // A startLine without endLine is almost certainly a
        // frontend bug; reject so the caller has to be explicit.
        let p = payload_with_attachment(
            vec![user_msg("?")],
            project_file_attachment("src/main.rs", Some(10), None),
        );
        let err = validate_payload(&p).expect_err("partial range rejected");
        match err {
            IpcError::BadArgument(s) => assert!(s.contains("endLine"), "msg was: {s}"),
            other => panic!("expected BadArgument, got {other:?}"),
        }
    }

    #[test]
    fn rejects_partial_line_range_end_only() {
        let p = payload_with_attachment(
            vec![user_msg("?")],
            project_file_attachment("src/main.rs", None, Some(10)),
        );
        let err = validate_payload(&p).expect_err("partial range rejected");
        match err {
            IpcError::BadArgument(s) => assert!(s.contains("startLine"), "msg was: {s}"),
            other => panic!("expected BadArgument, got {other:?}"),
        }
    }

    #[test]
    fn rejects_zero_start_line() {
        // Lines are 1-based on every code surface (editor gutter,
        // grep, the model's own conventions). `0` is wrong.
        let p = payload_with_attachment(
            vec![user_msg("?")],
            project_file_attachment("src/main.rs", Some(0), Some(10)),
        );
        let err = validate_payload(&p).expect_err("zero startLine rejected");
        match err {
            IpcError::BadArgument(s) => assert!(s.contains("1-based"), "msg was: {s}"),
            other => panic!("expected BadArgument, got {other:?}"),
        }
    }

    #[test]
    fn rejects_end_line_before_start_line() {
        let p = payload_with_attachment(
            vec![user_msg("?")],
            project_file_attachment("src/main.rs", Some(20), Some(10)),
        );
        let err = validate_payload(&p).expect_err("inverted range rejected");
        match err {
            IpcError::BadArgument(s) => assert!(s.contains("endLine"), "msg was: {s}"),
            other => panic!("expected BadArgument, got {other:?}"),
        }
    }

    #[test]
    fn accepts_single_line_range_where_start_equals_end() {
        // start == end is a one-line range — common when the user
        // clicks on a single line and hits Attach.
        let p = payload_with_attachment(
            vec![user_msg("focus")],
            project_file_attachment("src/main.rs", Some(42), Some(42)),
        );
        validate_payload(&p).expect("single-line range must validate");
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

    // ---- D11: attachment-requires-trust gate ----

    #[test]
    fn check_attachment_requires_trust_passes_with_no_attachment() {
        // Plain chat without an attachment is allowed regardless
        // of whether a project is open or trusted — D7.1 behavior.
        check_attachment_requires_trust(false, false).expect("plain chat allowed");
        check_attachment_requires_trust(false, true).expect("plain chat allowed with trust");
    }

    #[test]
    fn check_attachment_requires_trust_passes_with_trusted_project() {
        // Attachment + trusted project is the green-path case.
        check_attachment_requires_trust(true, true).expect("attachment with trust allowed");
    }

    #[test]
    fn check_attachment_requires_trust_rejects_attachment_without_trust() {
        // The honest reject: caller wants to attach a file but
        // there's no trusted project to read it from. The handler
        // surfaces this as `NeedsApproval` so the frontend can
        // prompt for trust instead of silently dropping the
        // attachment.
        let err = check_attachment_requires_trust(true, false)
            .expect_err("attachment without trust must reject");
        assert!(
            matches!(err, IpcError::NeedsApproval),
            "expected NeedsApproval, got {err:?}"
        );
    }

    #[test]
    fn rejects_nul_byte_in_attachment_rel_path() {
        let p = payload_with_attachment(
            vec![user_msg("hi")],
            project_file_attachment("src/main\0.rs", None, None),
        );
        let err = validate_payload(&p).expect_err("NUL in relPath rejected");
        match err {
            IpcError::BadArgument(s) => assert!(s.contains("NUL")),
            other => panic!("expected BadArgument, got {other:?}"),
        }
    }
}
