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

use crate::chat::ollama::{self, ChatError, StreamOutcome};
use crate::chat::stream::ChatStreamRegistry;
use crate::chat::{ChatDoneEvent, ChatFinish, ChatMessage, ChatRole, ChatTokenEvent};
use crate::commands::project::AppState;
use crate::error::{IpcError, IpcRequest};
use crate::project::OpenProject;
use crate::prompts::{assemble, AttachmentRequest};

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
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum AttachmentPayload {
    /// A file at `relPath` inside the currently-open trusted
    /// project root. Backend reads via the Rust-private
    /// `prompts::read::read_for_prompt` path; raw bytes never
    /// reach the frontend.
    #[serde(rename = "projectFile")]
    ProjectFile { rel_path: String },
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

    // D8: assemble the final wire transcript. With no attachment
    // this is a clone of the messages array; with an attachment we
    // require a trusted open project, run the prompt-read +
    // redactor, and fold the file into the last user message. All
    // errors (`Blocked` for secret-pattern filenames, `NotFound`,
    // `PathEscape`, `BadArgument` for shape, …) surface
    // synchronously here so the frontend never spins up a
    // streaming UI for a request that already failed.
    let assembled_messages = match payload.attachment.as_ref() {
        None => payload.messages.clone(),
        Some(att) => {
            let open = require_trusted_open(&state)?;
            let request = attachment_to_request(att);
            let assembled = assemble(&open.root, &payload.messages, Some(request))?;
            if let Some(summary) = assembled.attachment.as_ref() {
                tracing::debug!(
                    rel_path = %summary.rel_path,
                    original_bytes = summary.original_bytes,
                    redactions = summary.redaction_count,
                    "chat.send attached file"
                );
            }
            assembled.messages
        }
    };

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
    })
}

/// "There is an open project AND its canonical root is in the trust
/// store." Mirrors `commands::fs::require_trusted_open` — D8 only
/// needs the same gate when an attachment is present.
fn require_trusted_open(state: &AppState) -> Result<OpenProject, IpcError> {
    let open = state.session.current().ok_or(IpcError::NeedsApproval)?;
    let trusted = {
        let store = state.trust.lock().expect("trust mutex poisoned");
        store.is_trusted(&open.root)
    };
    if !trusted {
        return Err(IpcError::NeedsApproval);
    }
    Ok(open)
}

fn attachment_to_request(att: &AttachmentPayload) -> AttachmentRequest {
    match att {
        AttachmentPayload::ProjectFile { rel_path } => AttachmentRequest::ProjectFile {
            rel_path: rel_path.clone(),
        },
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
        Ok(StreamOutcome::Done { model_id: served }) => ChatDoneEvent {
            id: stream_id.clone(),
            seq,
            finish: ChatFinish::Stop,
            model_id: Some(served),
            duration_ms,
            error: None,
        },
        Ok(StreamOutcome::Cancelled { model_id: served }) => ChatDoneEvent {
            id: stream_id.clone(),
            seq,
            finish: ChatFinish::Cancelled,
            model_id: served.or(Some(model_id.clone())),
            duration_ms,
            error: None,
        },
        Ok(StreamOutcome::EofBeforeDone { model_id: served }) => ChatDoneEvent {
            id: stream_id.clone(),
            seq,
            finish: ChatFinish::Length,
            model_id: served.or(Some(model_id.clone())),
            duration_ms,
            error: None,
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
        AttachmentPayload::ProjectFile { rel_path } => {
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
        }
    }
    Ok(())
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
            AttachmentPayload::ProjectFile {
                rel_path: "src/main.rs".into(),
            },
        );
        validate_payload(&p).expect("normal attachment must validate");
    }

    #[test]
    fn rejects_empty_attachment_rel_path() {
        let p = payload_with_attachment(
            vec![user_msg("hi")],
            AttachmentPayload::ProjectFile {
                rel_path: "   ".into(),
            },
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
            AttachmentPayload::ProjectFile {
                rel_path: "a".repeat(MAX_ATTACHMENT_REL_PATH_LEN + 1),
            },
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
            AttachmentPayload::ProjectFile {
                rel_path: "/etc/passwd".into(),
            },
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
            AttachmentPayload::ProjectFile {
                rel_path: "src/../../etc/passwd".into(),
            },
        );
        let err = validate_payload(&p).expect_err("`..` segment rejected");
        match err {
            IpcError::BadArgument(s) => assert!(s.contains("'..'")),
            other => panic!("expected BadArgument, got {other:?}"),
        }
    }

    #[test]
    fn rejects_nul_byte_in_attachment_rel_path() {
        let p = payload_with_attachment(
            vec![user_msg("hi")],
            AttachmentPayload::ProjectFile {
                rel_path: "src/main\0.rs".into(),
            },
        );
        let err = validate_payload(&p).expect_err("NUL in relPath rejected");
        match err {
            IpcError::BadArgument(s) => assert!(s.contains("NUL")),
            other => panic!("expected BadArgument, got {other:?}"),
        }
    }
}
