//! Prompt assembly: turn user instructions + optional file context
//! into a `Vec<ChatMessage>` for the chat adapter.
//!
//! This module owns the only path on the backend where file bytes
//! are read for a model. The reader is private to `prompts` — there
//! is no IPC verb that exposes a prompt-ready value, and the
//! `RedactedContent` type cannot be constructed outside this module.
//! See `docs/ARCHITECTURE.md § Display reads vs prompt reads` for
//! why the split exists.
//!
//! What lives here:
//!   * `read` — `read_for_prompt(root, target)`: secret-filename
//!     block, `.git/` whitelist, size cap, binary detection,
//!     content redaction. Caller never sees raw bytes. Visibility
//!     is `pub(in crate::prompts)` so only sibling modules
//!     (`assemble`, `instructions`) can call it.
//!   * `redact` — pattern-based redactor for the secret formats in
//!     `docs/SAFETY.md § Secret handling`. Hand-rolled so we add no
//!     new crate deps.
//!   * `instructions` — D11 reader for the project's root
//!     `AGENTS.md`. Returns `None` on missing / oversize / binary
//!     / unreadable so a broken instructions file doesn't fail
//!     the user's chat.
//!   * `assemble` — composes the final `Vec<ChatMessage>` for the
//!     model adapter: optional file attachment wrapped into the
//!     last user message (D8 + D10), optional project
//!     instructions prepended as a `system` message (D11), and the
//!     D15 propose-diff system message prepended FIRST when the
//!     caller passes `ChatMode::ProposeDiff` so the response-shape
//!     pin sits before project context in the final transcript.
//!     Also hosts the D12 `preview_context` path: same reads,
//!     same gates, no model call — answers "what would ride along
//!     on the next send?" for the chat panel's context-preview
//!     area.
//!   * `mode` — D15 `ChatMode` enum (`chat` / `proposeDiff`) plus
//!     the propose-diff system message that pins the model to a
//!     single fenced unified-diff response.
//!
//! Only `assemble` + `preview_context` + `ChatMode` (and their
//! small request/response types) are re-exported here. The reader,
//! the redactor, and the instructions probe stay inside the module
//! so the chat handler can't accidentally reach for the lower-level
//! primitives.
//!
//! Out of scope:
//!   * Multi-file attachments.
//!   * Recursive directory attachments.
//!   * `README.md` auto-context, nested per-directory instruction
//!     files, `.plume/` overlays — those are roadmap.
//!   * The `scoped-edit` / `agent-loop` prompt shapes — those land
//!     with their respective slices. (`propose-diff` shape pinning
//!     ships in D15 via `mode::propose_diff_system_message`.)
//!   * On-disk patch apply / validation for propose-diff replies —
//!     D15 ships the preview half only; Apply stays disabled.
//!   * Connection-string password redaction (deferred; see
//!     `docs/SAFETY.md § Secret handling`).

mod assemble;
mod attachment_slice;
mod context_manifest;
mod explicit_context;
mod instructions;
mod mode;
mod read;
pub(crate) mod redact;

pub use assemble::{
    apply_attachment, assemble, assemble_with_context, preview_context,
    preview_context_with_sources, AttachmentPreviewOutcome, AttachmentRequest, LineRange,
};
pub use explicit_context::{
    resolve_explicit_context_for_preview, resolve_explicit_context_for_send,
    validate_context_manifest, validate_context_source_refs, BrowserScreenshotImage,
    ContextSourceManifestItem, ContextSourcePreviewOutcome, ContextSourceRef,
    ExplicitContextResolved, EXPLICIT_CONTEXT_BYTE_CAP, MAX_EXPLICIT_CONTEXT_SOURCES,
};
pub use mode::ChatMode;
// `AssembledPrompt`, `InstructionsSummary`, `AttachmentSummary`,
// and `ContextPreview` are returned by `assemble` /
// `preview_context`; production callers access their fields
// without naming the types, so none are re-exported in the bin
// build. `AttachmentSummary` is re-exported under `cfg(test)` so
// the chat handler's mapping tests can construct
// `AttachmentPreviewOutcome::Ready(...)` values directly without
// reaching into a sibling module's privates.
#[cfg(test)]
pub use assemble::AttachmentSummary;
