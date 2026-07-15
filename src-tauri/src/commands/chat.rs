//! `chat.send` + `chat.cancel` + `chat.context` Tauri command
//! handlers.
//!
//! D7 shipped `chat.send` as a synchronous call that returned the
//! full assistant message. D7.1 reshapes it: `chat.send` accepts a
//! **client-minted** `streamId`, validates it, spawns the streaming
//! task, and returns the same id back. The assistant reply arrives
//! over Tauri events (`chat/token` per delta, terminal
//! `chat/done`). `chat.cancel(streamId)` flips a cooperative cancel
//! flag.
//!
//! D12 adds `chat.context`: a read-only IPC that runs the same
//! preflight reads `chat.send` would (probe AGENTS.md, resolve +
//! redact the optional attachment, validate the line range) and
//! returns a small summary the UI renders as a "what would ride
//! along on the next send" preview area. The verb invokes no
//! model, registers no stream id, and surfaces attachment
//! rejections IN-BAND so a blocked attachment doesn't hide the
//! AGENTS.md preview alongside it. The two paths share helpers
//! (`prompts::preview_context` and `prompts::assemble`) so the
//! preview's numbers always match what the actual send would log.
//!
//! D23 module layout: `chat.rs` is the orchestrator. The three
//! `#[tauri::command]` entry points live in focused siblings:
//! `chat/send.rs` owns `chat_send` + `run_stream` plus
//! `ChatSendPayload` / `ChatSendStartedResponse` (its provider
//! routing, outcome/stats translation, and tests live in
//! `send_route.rs` / `send_outcome.rs` / `send_tests.rs` —
//! D116/D118/D120 splits);
//! `chat/cancel.rs` owns `chat_cancel` and `ChatCancelPayload`;
//! `chat/context.rs` owns `chat_context` + the outcome mapping
//! and the `ChatContext*` response types; `chat/validate.rs` owns
//! the payload-shape validators. Public verbs are re-exported
//! below so `crate::commands::chat::{chat_send, chat_cancel,
//! chat_context}` still resolves through `tauri::generate_handler!`
//! in `main.rs`. The shared bits stay here: top-of-file constants,
//! the `AttachmentPayload` wire enum (used by both `chat.send` and
//! `chat.context`), and the three small helpers
//! (`check_attachment_requires_trust`, `optional_trusted_open`,
//! `attachment_to_request`) every submodule reaches for. See
//! `docs/IPC_CONTRACT.md § chat` for the validation order and
//! trust-gate rationale.

use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::commands::project::AppState;
use crate::commands::sessions::{map_store_err, scope_dir, SessionScope};
use crate::error::IpcError;
use crate::project::OpenProject;
use crate::prompts::{AttachmentRequest, LineRange};
use crate::sessions;

mod cancel;
mod context;
mod send;
mod validate;
mod vision;

// D99: re-export the attachment shape validator so the single-step agent
// command (a sibling under `commands`) can run the same pre-flight check
// chat.send / chat.context run, rather than skipping it and letting a
// half-range silently become whole-file or a `startLine: 0` reach the
// slice underflow.
pub(super) use validate::validate_attachment;

pub use cancel::chat_cancel;
pub use context::chat_context;
pub use send::chat_send;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatMemoryContextEntry {
    pub id: String,
    pub created_at_ms: u64,
    pub text_bytes: u64,
    pub preview: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatTopicContextFile {
    pub name: String,
    pub bytes: u64,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChatContextOwner {
    pub scope: SessionScope,
    pub session_id: String,
}

pub(super) fn validate_context_owner(
    owner: Option<&ChatContextOwner>,
    include_project_context: bool,
    has_sources: bool,
    state: &AppState,
) -> Result<Option<String>, IpcError> {
    let Some(owner) = owner else {
        if has_sources && !include_project_context {
            return Err(IpcError::BadArgument(
                "local explicit context requires contextOwner".into(),
            ));
        }
        return Ok(None);
    };
    if (owner.scope == SessionScope::Project) != include_project_context {
        return Err(IpcError::BadArgument(
            "contextOwner scope does not match this chat surface".into(),
        ));
    }
    let dir = scope_dir(owner.scope, state)?;
    if !sessions::session_exists(&dir, &owner.session_id).map_err(map_store_err)? {
        return Err(IpcError::NotFound("context owner session".into()));
    }
    Ok((owner.scope == SessionScope::Local).then(|| owner.session_id.clone()))
}

/// Default localhost endpoint for Ollama. Centralizing port
/// overrides is roadmap (`docs/IPC_ROADMAP.md § Provider health`).
pub(super) const OLLAMA_HOST: &str = "127.0.0.1";
pub(super) const OLLAMA_PORT: u16 = 11434;

/// Cap on a single chat stream's total wall-clock duration. Five
/// minutes is generous on modest hardware — long enough for a 7 B
/// model on Metal to finish a paragraph, short enough that a stuck
/// daemon doesn't pin the registry slot forever. The streaming loop
/// checks this between line reads.
// `pub` (not `pub(super)`): the D129C `plume_bench` sidecar reuses the
// real product budget so benchmark and app behavior cannot drift.
pub const CHAT_OVERALL_BUDGET: Duration = Duration::from_secs(300);

/// Connect timeout for the TCP handshake at the start of a stream.
/// This is much shorter than the overall budget because "Ollama is
/// not running" should surface immediately, not after 5 minutes.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

/// Event name for per-frame delta payloads (`ChatTokenEvent`).
pub(super) const CHAT_TOKEN_EVENT: &str = "chat/token";
/// Event name for the terminal payload (`ChatDoneEvent`). Exactly
/// one of these fires per stream id.
pub(super) const CHAT_DONE_EVENT: &str = "chat/done";

/// Hard cap on a client-minted stream id. UUID v4 is 36 chars; 128
/// is generous headroom without giving an attacker room to send a
/// large allocation through every chat call.
pub(super) const MAX_STREAM_ID_LEN: usize = 128;

/// Cap on an attachment's relative-path string. The OS-level
/// `PATH_MAX` is 1024 on macOS and 4096 on Linux; 1024 is a useful
/// floor that catches obvious garbage (a JSON blob in the field)
/// without rejecting a legitimately deep relative path.
pub(super) const MAX_ATTACHMENT_REL_PATH_LEN: usize = 1024;

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
#[serde(tag = "kind")]
pub enum AttachmentPayload {
    /// A file at `relPath` inside the currently-open trusted
    /// project root. Backend reads via the Rust-private
    /// `prompts::read::read_for_prompt` path; raw bytes never
    /// reach the frontend.
    ///
    /// IMPORTANT: every wire-field carries an explicit
    /// `#[serde(rename = "...")]`. Serde's `rename_all` on the
    /// outer enum only renames *variants*; it does NOT cascade
    /// into struct-variant fields. Relying on the enum-level
    /// attribute silently leaves the fields as `rel_path` /
    /// `start_line` / `end_line` on the wire, and a camelCase
    /// payload from the TypeScript side fails to deserialize with
    /// `missing field rel_path`. Per-field renames are the safest
    /// fix — any new field added here MUST carry its own
    /// `rename = "..."` annotation.
    #[serde(rename = "projectFile")]
    ProjectFile {
        #[serde(rename = "relPath")]
        rel_path: String,
        /// 1-based inclusive start of the requested line range.
        /// Must accompany `end_line`; either both fields are
        /// present or both are absent.
        #[serde(rename = "startLine", default)]
        start_line: Option<u32>,
        /// 1-based inclusive end of the requested line range.
        #[serde(rename = "endLine", default)]
        end_line: Option<u32>,
    },
}

/// Reject `chat.send` with `NeedsApproval` when the caller asks for
/// an attachment but no trusted project is open. Pulled out into a
/// pure function so the trust-gate branch is testable without
/// standing up an `AppState` / `Tauri::State` test fixture.
pub(super) fn check_attachment_requires_trust(
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
pub(super) fn optional_trusted_open(state: &AppState) -> Option<OpenProject> {
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

pub(super) fn attachment_to_request(att: &AttachmentPayload) -> AttachmentRequest {
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

#[cfg(test)]
mod tests {
    use super::*;

    // ---- D8 attachment wire-shape (serde camelCase) ----
    //
    // These tests pin the JSON ↔ Rust mapping for `AttachmentPayload`
    // so a future refactor doesn't silently regress it. The
    // packaged app sends camelCase from TypeScript; the enum-level
    // `rename_all` does NOT cascade into struct-variant fields, so
    // each field carries an explicit `#[serde(rename = "...")]`.
    // Without these tests, the bug was invisible because the rest
    // of the test suite constructs `AttachmentPayload` values
    // directly in Rust, bypassing serde entirely.

    #[test]
    fn deserializes_project_file_attachment_with_camelcase_line_range() {
        // The exact shape the TypeScript `chat.context` / `chat.send`
        // calls put on the wire when the user attaches a line range.
        let json =
            r#"{"kind":"projectFile","relPath":"docs/BOOTSTRAP.md","startLine":1,"endLine":3}"#;
        let parsed: AttachmentPayload =
            serde_json::from_str(json).expect("camelCase line-range payload must deserialize");
        match parsed {
            AttachmentPayload::ProjectFile {
                rel_path,
                start_line,
                end_line,
            } => {
                assert_eq!(rel_path, "docs/BOOTSTRAP.md");
                assert_eq!(start_line, Some(1));
                assert_eq!(end_line, Some(3));
            }
        }
    }

    #[test]
    fn deserializes_project_file_attachment_without_line_range() {
        // The whole-file D8 shape — no `startLine` / `endLine` keys
        // at all. Serde must accept that and default both to `None`.
        let json = r#"{"kind":"projectFile","relPath":"src/main.rs"}"#;
        let parsed: AttachmentPayload =
            serde_json::from_str(json).expect("camelCase whole-file payload must deserialize");
        match parsed {
            AttachmentPayload::ProjectFile {
                rel_path,
                start_line,
                end_line,
            } => {
                assert_eq!(rel_path, "src/main.rs");
                assert_eq!(start_line, None);
                assert_eq!(end_line, None);
            }
        }
    }

    #[test]
    fn rejects_snake_case_rel_path_on_the_wire() {
        // Belt-and-suspenders: confirm that the OLD (broken)
        // shape — snake_case keys — no longer parses. If a future
        // refactor accidentally adds `#[serde(alias = "rel_path")]`
        // or otherwise widens the accepted shape, this test fires
        // so we can decide whether that's intentional.
        let json = r#"{"kind":"projectFile","rel_path":"docs/BOOTSTRAP.md"}"#;
        let err = serde_json::from_str::<AttachmentPayload>(json)
            .expect_err("snake_case relPath must be rejected on the wire");
        // The error text mentions the missing camelCase field —
        // exact wording is a serde implementation detail, so we
        // only assert the field name appears somewhere.
        assert!(
            err.to_string().contains("relPath"),
            "expected error to mention 'relPath'; got: {err}"
        );
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
}
