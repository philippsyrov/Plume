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
use crate::prompts::{
    preview_context_with_sources, AttachmentPreviewOutcome, ContextSourceManifestItem,
    ContextSourcePreviewOutcome, ContextSourceRef,
};

use super::validate::validate_attachment;
use super::{
    attachment_to_request, optional_trusted_open, AttachmentPayload, ChatMemoryContextEntry,
    ChatTopicContextFile,
};

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
    /// Ordered explicit context references to resolve independently.
    #[serde(default)]
    pub context_sources: Vec<ContextSourceRef>,
    /// Defaults to true. No-project chat passes false so preview
    /// stays empty even when the backend session still has a trusted
    /// project open from earlier in the window.
    #[serde(default = "default_include_project_context")]
    pub include_project_context: bool,
}

fn default_include_project_context() -> bool {
    true
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
    /// One outcome per deduplicated requested explicit source, in order.
    pub context_sources: Vec<ChatContextSourcePreview>,
}

#[derive(Debug, Serialize)]
#[serde(
    tag = "status",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ChatContextSourcePreview {
    Ready {
        source: ContextSourceManifestItem,
    },
    Blocked {
        #[serde(rename = "ref")]
        source_ref: ContextSourceRef,
        reason: ChatContextBlockReason,
        message: String,
    },
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
    pub entries: Vec<ChatMemoryContextEntry>,
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
    pub files: Vec<ChatTopicContextFile>,
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
    if payload.attachment.is_some() && !payload.context_sources.is_empty() {
        return Err(IpcError::BadArgument(
            "chat.context cannot include both attachment and contextSources".into(),
        ));
    }
    crate::prompts::validate_context_source_refs(&payload.context_sources)?;

    let trusted_open = if payload.include_project_context {
        optional_trusted_open(&state)
    } else {
        None
    };
    let project_root = trusted_open.as_ref().map(|p| p.root.as_path());
    let attachment_request = payload.attachment.as_ref().map(attachment_to_request);
    let preview =
        preview_context_with_sources(project_root, attachment_request, &payload.context_sources);

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
        entries: s
            .entries
            .into_iter()
            .map(|entry| ChatMemoryContextEntry {
                id: entry.id,
                created_at_ms: entry.created_at_ms,
                text_bytes: entry.text_bytes as u64,
                preview: entry.preview,
            })
            .collect(),
    });

    let topics = preview.topics.map(|s| ChatContextTopicsPreview {
        file_count: s.file_count as u64,
        bytes: s.used_bytes as u64,
        byte_cap: s.byte_cap as u64,
        truncated: s.truncated,
        files: s
            .files
            .into_iter()
            .map(|file| ChatTopicContextFile {
                name: file.name,
                bytes: file.bytes as u64,
            })
            .collect(),
    });
    let context_sources = preview
        .explicit_context
        .into_iter()
        .map(|outcome| match outcome {
            ContextSourcePreviewOutcome::Ready(source) => {
                ChatContextSourcePreview::Ready { source }
            }
            ContextSourcePreviewOutcome::Blocked { source_ref, error } => {
                let reason = block_reason_for(&error);
                let message = error.to_string();
                ChatContextSourcePreview::Blocked {
                    source_ref,
                    reason,
                    message,
                }
            }
        })
        .collect();

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
        context_sources,
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
#[path = "context_tests.rs"]
mod tests;
