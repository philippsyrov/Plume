//! `chat.context` Tauri command handler.
//!
//! Read-only preflight for the next `chat.send`. No stream id is
//! registered, no model is invoked. Runs the same trust check, the
//! same attachment validation, and the same prompt-read pipeline
//! `chat.send` would, then maps the in-Rust `ContextPreview` onto
//! the wire shape. Attachment rejections surface IN-BAND so a
//! blocked attachment doesn't hide the AGENTS.md preview alongside
//! it.

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::commands::project::AppState;
use crate::error::{IpcError, IpcRequest};
use crate::prompts::{preview_context, AttachmentPreviewOutcome};

use super::validate::validate_attachment;
use super::{attachment_to_request, optional_trusted_open, AttachmentPayload};

/// D12: `chat.context` payload — same shape as `chat.send` minus
/// the parts that only matter for actually running a model
/// (`streamId`, `providerId`, `modelId`, `messages`). The preview's
/// question is "what would ride along *on top of* my next prompt?",
/// not "what would the model say if I sent this prompt?", so the
/// transcript is intentionally out of scope.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatContextPayload {
    /// Optional read-only attachment to preview. Same wire shape as
    /// `ChatSendPayload.attachment` — the frontend can pass the
    /// exact same value it would use for `chat.send`.
    #[serde(default)]
    pub attachment: Option<AttachmentPayload>,
}

/// Response shape for `chat.context`. Mirrors what `chat.send` would
/// log on a successful accept; `attachment.status === 'blocked'`
/// stands in for the typed `IpcError` the actual send would reject
/// with so the UI can render BOTH the AGENTS.md preview and the
/// attachment rejection in a single round-trip.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatContextResponse {
    /// Forward-looking AGENTS.md preview. `null` covers every
    /// honest skip: no trusted project open, no AGENTS.md at root,
    /// or AGENTS.md present but unreadable.
    pub instructions: Option<ChatContextInstructionsPreview>,
    /// Per-attachment preview. `null` when the caller asked for
    /// no attachment. When the caller asked for one, this is
    /// always present — either `ready` (would ride along, here
    /// are the numbers) or `blocked` (would reject, here's why).
    pub attachment: Option<ChatContextAttachmentPreview>,
    /// D42: forward-looking project-memory preview. `null` when
    /// no trusted project is open, the store is empty, or the
    /// store is unreadable. `Some` when at least one entry would
    /// be folded into the next send. Counts mirror what
    /// `chat.send`'s response would carry on the same turn.
    pub memory: Option<ChatContextMemoryPreview>,
    /// D72: forward-looking curated topic-file preview (INDEX/USER/
    /// SOUL). `null` on the same honest skips as `memory`. Counts
    /// mirror what `chat.send`'s response carries on the same turn.
    pub topics: Option<ChatContextTopicsPreview>,
}

/// D42: wire shape for the project-memory preview surfaced through
/// `chat.context`. Field names match `ChatSendMemorySummary` so a
/// single TypeScript renderer covers both call sites.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatContextMemoryPreview {
    pub entry_count: u64,
    pub bytes: u64,
    pub byte_cap: u64,
    pub truncated: bool,
}

/// D72: wire shape for the curated topic-file preview. Field names
/// match `ChatSendTopicsSummary` so one TypeScript renderer covers
/// both call sites.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatContextTopicsPreview {
    pub file_count: u64,
    pub bytes: u64,
    pub byte_cap: u64,
    pub truncated: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatContextInstructionsPreview {
    pub source: String,
    pub original_bytes: u64,
    pub redaction_count: u64,
}

/// Per-attachment preview. The discriminator is `status` so the
/// TypeScript shape reads `attachment.status === 'ready'` / `=== 'blocked'`
/// — same idiom as the existing `selection.kind` enum.
///
/// IMPORTANT: every wire-field carries an explicit
/// `#[serde(rename = "...")]`. Serde's `rename_all` on the outer
/// enum only renames *variants* (which we override with explicit
/// `rename = "ready"` / `"blocked"` anyway); it does NOT cascade
/// into struct-variant fields for either direction (deserialize
/// or serialize). Without per-field renames the JSON goes out
/// with `rel_path` / `start_line` / `original_bytes` etc., and
/// the TypeScript side gets `undefined` for every field — the
/// chat panel then renders `undefined:undefined · NaN MB`. Same
/// class of bug `AttachmentPayload::ProjectFile` had on the
/// request side; pinned with serialization tests below.
#[derive(Debug, Serialize)]
#[serde(tag = "status")]
pub enum ChatContextAttachmentPreview {
    #[serde(rename = "ready")]
    Ready {
        #[serde(rename = "relPath")]
        rel_path: String,
        /// `None` means "whole file"; matches `AttachmentSummary`.
        #[serde(rename = "startLine")]
        start_line: Option<u32>,
        #[serde(rename = "endLine")]
        end_line: Option<u32>,
        #[serde(rename = "originalBytes")]
        original_bytes: u64,
        #[serde(rename = "redactionCount")]
        redaction_count: u64,
    },
    #[serde(rename = "blocked")]
    Blocked {
        #[serde(rename = "relPath")]
        rel_path: String,
        /// Stable kind code the UI can switch on without parsing
        /// `message`. Mirrors how `IpcError` is consumed: match on
        /// `kind`, never the human-readable text.
        reason: ChatContextBlockReason,
        /// Short human-readable explanation. Carries the same text
        /// the typed `IpcError` would have surfaced through
        /// `chat.send`, so the UI can show the same diagnostic
        /// without duplicating the mapping table.
        message: String,
    },
}

/// Stable enum of the reasons an attachment would reject. New
/// variants are additive (`#[non_exhaustive]` from the wire's
/// perspective: the TS layer treats an unknown reason as a generic
/// "blocked" with the human message).
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ChatContextBlockReason {
    /// File doesn't exist (or canonicalises to something that
    /// doesn't exist) at the project-relative path.
    NotFound,
    /// Path safety rejected it: escapes the project root, is a
    /// hardlink alias, or otherwise lies outside the trusted
    /// boundary.
    PathEscape,
    /// Prompt-read policy rejected it: secret-filename, oversize,
    /// binary content, `.git/` non-whitelist, etc.
    Blocked,
    /// Shape problem with the request (e.g. `endLine` past EOF,
    /// invalid range) — same kinds that `chat.send` raises as
    /// `BadArgument`.
    BadArgument,
    /// No trusted project open. The user can clear this by trusting
    /// the project, so it's reported the same way `chat.send`'s
    /// trust gate would.
    NeedsApproval,
    /// Internal / unexpected (IO error on read, etc.). The
    /// frontend can present this as "preview failed — see logs".
    Internal,
}

/// Read-only preflight for the next `chat.send`. No stream id is
/// registered, no model is invoked. The handler runs the same
/// trust check, the same attachment validation, and the same
/// prompt-read pipeline `chat.send` would, then maps the
/// in-Rust `ContextPreview` onto the wire shape.
#[tauri::command]
pub async fn chat_context(
    req: IpcRequest<ChatContextPayload>,
    state: State<'_, AppState>,
) -> Result<ChatContextResponse, IpcError> {
    req.check_version()?;
    let payload = req.payload;

    // Same shape gate `chat.send` applies — reject obviously bad
    // attachment shapes (empty / overlong relPath, `..` segments,
    // half a line range, NUL bytes) before reaching the filesystem.
    if let Some(att) = payload.attachment.as_ref() {
        validate_attachment(att)?;
    }

    let trusted_open = optional_trusted_open(&state);
    let project_root = trusted_open.as_ref().map(|p| p.root.as_path());
    let attachment_request = payload.attachment.as_ref().map(attachment_to_request);
    let preview = preview_context(project_root, attachment_request);

    let instructions = preview
        .instructions
        .map(|s| ChatContextInstructionsPreview {
            source: s.source,
            original_bytes: s.original_bytes,
            // `usize` → `u64` is widening on every target Plume runs
            // on; cast is safe.
            redaction_count: s.redaction_count as u64,
        });

    let attachment = preview.attachment.map(chat_context_attachment_from_outcome);

    let memory = preview.memory.map(|s| ChatContextMemoryPreview {
        // `usize` → `u64` is widening on every supported target.
        entry_count: s.entry_count as u64,
        bytes: s.used_bytes as u64,
        byte_cap: s.byte_cap as u64,
        truncated: s.truncated,
    });

    let topics = preview.topics.map(|s| ChatContextTopicsPreview {
        file_count: s.file_count as u64,
        bytes: s.used_bytes as u64,
        byte_cap: s.byte_cap as u64,
        truncated: s.truncated,
    });

    if let Some(att) = attachment.as_ref() {
        // Mirror the `chat.send` tracing shape so a log query for
        // "what did the attachment look like on this turn" finds
        // both the preview probe and the actual send.
        match att {
            ChatContextAttachmentPreview::Ready {
                rel_path,
                start_line,
                end_line,
                original_bytes,
                redaction_count,
            } => {
                let range_label = match (start_line, end_line) {
                    (Some(s), Some(e)) => format!("{s}-{e}"),
                    _ => "whole-file".to_string(),
                };
                tracing::debug!(
                    rel_path = %rel_path,
                    original_bytes = original_bytes,
                    redactions = redaction_count,
                    line_range = %range_label,
                    "chat.context ready"
                );
            }
            ChatContextAttachmentPreview::Blocked {
                rel_path,
                reason,
                message,
            } => {
                tracing::debug!(
                    rel_path = %rel_path,
                    reason = ?reason,
                    message = %message,
                    "chat.context blocked"
                );
            }
        }
    }

    Ok(ChatContextResponse {
        instructions,
        attachment,
        memory,
        topics,
    })
}

/// Map the in-Rust `AttachmentPreviewOutcome` onto the wire shape.
/// The `IpcError` carried by `Blocked` is mapped through
/// `block_reason_for` so the frontend can switch on a stable enum
/// rather than parsing the human-readable text.
fn chat_context_attachment_from_outcome(
    outcome: AttachmentPreviewOutcome,
) -> ChatContextAttachmentPreview {
    match outcome {
        AttachmentPreviewOutcome::Ready(summary) => ChatContextAttachmentPreview::Ready {
            rel_path: summary.rel_path,
            start_line: summary.line_range.map(|r| r.start),
            end_line: summary.line_range.map(|r| r.end),
            original_bytes: summary.original_bytes,
            redaction_count: summary.redaction_count as u64,
        },
        AttachmentPreviewOutcome::Blocked { rel_path, error } => {
            let reason = block_reason_for(&error);
            let message = error.to_string();
            ChatContextAttachmentPreview::Blocked {
                rel_path,
                reason,
                message,
            }
        }
    }
}

/// Map an `IpcError` onto its stable `ChatContextBlockReason` code.
/// Pure function so the mapping is testable without standing up an
/// `AppState` fixture.
fn block_reason_for(error: &IpcError) -> ChatContextBlockReason {
    match error {
        IpcError::NotFound(_) => ChatContextBlockReason::NotFound,
        IpcError::PathEscape(_) => ChatContextBlockReason::PathEscape,
        IpcError::Blocked(_) => ChatContextBlockReason::Blocked,
        IpcError::BadArgument(_) => ChatContextBlockReason::BadArgument,
        IpcError::NeedsApproval => ChatContextBlockReason::NeedsApproval,
        // Cancelled / ProviderDown / Version / Internal don't
        // reach `chat.context` from the preview path today —
        // preview never makes a network call, never cancels, never
        // mismatches versions (the envelope check fires first).
        // Anything that slips through here is a genuine bug; map
        // to Internal so the UI can surface it as "preview
        // failed".
        _ => ChatContextBlockReason::Internal,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prompts::LineRange;

    // ---- D12 chat.context response wire-shape (serde Serialize) ----
    //
    // Mirrors the `AttachmentPayload` request-side bug on the
    // RESPONSE side: serde's `rename_all` on an enum doesn't
    // cascade into struct-variant fields when serializing either,
    // so the JSON went out as snake_case and TypeScript got
    // `undefined` for every field. The tests below assert that
    // each field appears in camelCase on the wire AND that the
    // snake_case form never leaks through.

    #[test]
    fn serializes_ready_attachment_preview_with_camelcase_fields() {
        let value = ChatContextAttachmentPreview::Ready {
            rel_path: "docs/BOOTSTRAP.md".into(),
            start_line: Some(1),
            end_line: Some(3),
            original_bytes: 2048,
            redaction_count: 0,
        };
        let json = serde_json::to_string(&value).expect("Ready must serialize");
        // Positive: every camelCase field appears as a JSON key.
        for key in [
            "\"status\"",
            "\"relPath\"",
            "\"startLine\"",
            "\"endLine\"",
            "\"originalBytes\"",
            "\"redactionCount\"",
        ] {
            assert!(
                json.contains(key),
                "Ready JSON must contain {key}; got: {json}"
            );
        }
        // Negative: no snake_case form leaks through. A future
        // refactor that drops the per-field `rename = "..."`
        // annotations would re-introduce the original P2 bug;
        // these assertions fire if that happens.
        for leaked in [
            "\"rel_path\"",
            "\"start_line\"",
            "\"end_line\"",
            "\"original_bytes\"",
            "\"redaction_count\"",
        ] {
            assert!(
                !json.contains(leaked),
                "Ready JSON must NOT contain snake_case {leaked}; got: {json}"
            );
        }
        // Discriminator must be the lowercase "ready" the
        // TypeScript switch statement matches on.
        assert!(
            json.contains("\"status\":\"ready\""),
            "Ready JSON must carry status='ready'; got: {json}"
        );
    }

    #[test]
    fn serializes_ready_attachment_preview_with_null_line_range() {
        // Whole-file attach: `startLine` and `endLine` must be
        // present as JSON `null`, not omitted. The TypeScript shape
        // expects `startLine: number | null` and a missing field
        // would land as `undefined`, breaking the rendered chip.
        let value = ChatContextAttachmentPreview::Ready {
            rel_path: "src/main.rs".into(),
            start_line: None,
            end_line: None,
            original_bytes: 50,
            redaction_count: 0,
        };
        let json = serde_json::to_string(&value).expect("Ready must serialize");
        assert!(
            json.contains("\"startLine\":null"),
            "startLine must serialize as null when whole-file; got: {json}"
        );
        assert!(
            json.contains("\"endLine\":null"),
            "endLine must serialize as null when whole-file; got: {json}"
        );
    }

    #[test]
    fn serializes_blocked_attachment_preview_with_camelcase_fields() {
        let value = ChatContextAttachmentPreview::Blocked {
            rel_path: "src/.env".into(),
            reason: ChatContextBlockReason::Blocked,
            message: ".env is blocked by policy".into(),
        };
        let json = serde_json::to_string(&value).expect("Blocked must serialize");
        for key in ["\"status\"", "\"relPath\"", "\"reason\"", "\"message\""] {
            assert!(
                json.contains(key),
                "Blocked JSON must contain {key}; got: {json}"
            );
        }
        assert!(
            !json.contains("\"rel_path\""),
            "Blocked JSON must NOT contain snake_case rel_path; got: {json}"
        );
        assert!(
            json.contains("\"status\":\"blocked\""),
            "Blocked JSON must carry status='blocked'; got: {json}"
        );
        // The `reason` enum is unit-style; its variants are renamed
        // via the enum-level `rename_all = "camelCase"` (which IS
        // load-bearing for unit enums). Pin the camelCase form.
        assert!(
            json.contains("\"reason\":\"blocked\""),
            "Blocked JSON must carry reason='blocked' camelCase; got: {json}"
        );
    }

    #[test]
    fn serializes_block_reason_variants_in_camel_case() {
        // Pins every `ChatContextBlockReason` variant against the
        // exact wire string the TypeScript `ChatContextBlockReason`
        // union expects. A future enum-level change that drops
        // `rename_all = "camelCase"` would break this.
        let cases = [
            (ChatContextBlockReason::NotFound, "\"notFound\""),
            (ChatContextBlockReason::PathEscape, "\"pathEscape\""),
            (ChatContextBlockReason::Blocked, "\"blocked\""),
            (ChatContextBlockReason::BadArgument, "\"badArgument\""),
            (ChatContextBlockReason::NeedsApproval, "\"needsApproval\""),
            (ChatContextBlockReason::Internal, "\"internal\""),
        ];
        for (variant, expected) in cases {
            let json = serde_json::to_string(&variant).expect("variant must serialize");
            assert_eq!(json, expected, "variant did not serialize to {expected}");
        }
    }

    #[test]
    fn serializes_instructions_preview_with_camelcase_fields() {
        // Struct (not enum) — `rename_all = "camelCase"` DOES
        // cascade here. Pinned for safety so a refactor that
        // accidentally drops the struct-level attribute fires this
        // test rather than silently breaking the UI's AGENTS.md
        // chip.
        let value = ChatContextInstructionsPreview {
            source: "AGENTS.md".into(),
            original_bytes: 1234,
            redaction_count: 2,
        };
        let json = serde_json::to_string(&value).expect("instructions must serialize");
        for key in ["\"source\"", "\"originalBytes\"", "\"redactionCount\""] {
            assert!(
                json.contains(key),
                "instructions JSON must contain {key}; got: {json}"
            );
        }
        for leaked in ["\"original_bytes\"", "\"redaction_count\""] {
            assert!(
                !json.contains(leaked),
                "instructions JSON must NOT contain snake_case {leaked}; got: {json}"
            );
        }
    }

    // ---- D12: chat.context handler-level mapping ----
    //
    // The underlying preview behaviour (which paths reject, what
    // an AGENTS.md summary looks like, etc.) is pinned by tests in
    // `prompts::assemble`. Here we only test the chat-handler-side
    // mapping from `AttachmentPreviewOutcome` → wire shape, so the
    // mapping table doesn't drift.

    #[test]
    fn block_reason_for_maps_each_ipc_error_to_its_stable_code() {
        // Each IpcError variant the preview path can produce must
        // map to a distinct, stable `ChatContextBlockReason`. The
        // mapping is part of the wire contract — drift here would
        // silently retag rejections.
        assert!(matches!(
            block_reason_for(&IpcError::NotFound("x".into())),
            ChatContextBlockReason::NotFound
        ));
        assert!(matches!(
            block_reason_for(&IpcError::PathEscape("x".into())),
            ChatContextBlockReason::PathEscape
        ));
        assert!(matches!(
            block_reason_for(&IpcError::Blocked("x".into())),
            ChatContextBlockReason::Blocked
        ));
        assert!(matches!(
            block_reason_for(&IpcError::BadArgument("x".into())),
            ChatContextBlockReason::BadArgument
        ));
        assert!(matches!(
            block_reason_for(&IpcError::NeedsApproval),
            ChatContextBlockReason::NeedsApproval
        ));
        // Variants the preview shouldn't produce today still map
        // to a defined value (Internal) so the wire response never
        // carries an undefined discriminator.
        assert!(matches!(
            block_reason_for(&IpcError::Internal("x".into())),
            ChatContextBlockReason::Internal
        ));
        assert!(matches!(
            block_reason_for(&IpcError::Cancelled),
            ChatContextBlockReason::Internal
        ));
    }

    #[test]
    fn chat_context_attachment_ready_maps_summary_fields_verbatim() {
        // The wire shape echoes the in-Rust summary. We're testing
        // that no field is dropped or transformed — `usize` →
        // `u64` widens cleanly and `LineRange` flattens into the
        // `startLine` / `endLine` pair.
        use crate::prompts::AttachmentRequest;
        let outcome = AttachmentPreviewOutcome::Ready(crate::prompts::AttachmentSummary {
            rel_path: "src/foo.rs".into(),
            original_bytes: 1234,
            redaction_count: 2,
            line_range: Some(LineRange { start: 4, end: 7 }),
        });
        // Use the request type just to exercise the path the
        // handler uses; the helper itself doesn't take a request.
        let _ = AttachmentRequest::ProjectFile {
            rel_path: "src/foo.rs".into(),
            line_range: None,
        };
        match chat_context_attachment_from_outcome(outcome) {
            ChatContextAttachmentPreview::Ready {
                rel_path,
                start_line,
                end_line,
                original_bytes,
                redaction_count,
            } => {
                assert_eq!(rel_path, "src/foo.rs");
                assert_eq!(start_line, Some(4));
                assert_eq!(end_line, Some(7));
                assert_eq!(original_bytes, 1234);
                assert_eq!(redaction_count, 2);
            }
            other => panic!("expected Ready, got {other:?}"),
        }
    }

    #[test]
    fn chat_context_attachment_ready_whole_file_has_null_range() {
        // Whole-file attachments (line_range == None) must yield
        // `null` for both `startLine` and `endLine` on the wire so
        // the UI can render `src/foo.rs` without a trailing
        // `:undefined–undefined`.
        let outcome = AttachmentPreviewOutcome::Ready(crate::prompts::AttachmentSummary {
            rel_path: "src/foo.rs".into(),
            original_bytes: 50,
            redaction_count: 0,
            line_range: None,
        });
        match chat_context_attachment_from_outcome(outcome) {
            ChatContextAttachmentPreview::Ready {
                start_line,
                end_line,
                ..
            } => {
                assert_eq!(start_line, None);
                assert_eq!(end_line, None);
            }
            other => panic!("expected Ready, got {other:?}"),
        }
    }

    #[test]
    fn chat_context_attachment_blocked_carries_reason_and_message() {
        // The Blocked variant must surface the IpcError's
        // human-readable text on the wire so the UI can show the
        // same diagnostic `chat.send` would have, without
        // duplicating the mapping.
        let outcome = AttachmentPreviewOutcome::Blocked {
            rel_path: "src/.env".into(),
            error: IpcError::Blocked(".env is blocked by policy".into()),
        };
        match chat_context_attachment_from_outcome(outcome) {
            ChatContextAttachmentPreview::Blocked {
                rel_path,
                reason,
                message,
            } => {
                assert_eq!(rel_path, "src/.env");
                assert!(matches!(reason, ChatContextBlockReason::Blocked));
                assert!(
                    message.contains(".env is blocked"),
                    "message must echo the IpcError text, got: {message}"
                );
            }
            other => panic!("expected Blocked, got {other:?}"),
        }
    }

    #[test]
    fn chat_context_attachment_blocked_needs_approval_maps_to_typed_reason() {
        // NeedsApproval is the typed reason for "no trusted project,
        // can't read the attachment". The UI flips the chip to a
        // warn-coloured "Trust required" hint based on this code.
        let outcome = AttachmentPreviewOutcome::Blocked {
            rel_path: "anything.rs".into(),
            error: IpcError::NeedsApproval,
        };
        match chat_context_attachment_from_outcome(outcome) {
            ChatContextAttachmentPreview::Blocked { reason, .. } => {
                assert!(matches!(reason, ChatContextBlockReason::NeedsApproval));
            }
            other => panic!("expected Blocked, got {other:?}"),
        }
    }
}
