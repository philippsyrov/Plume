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
//!     instructions prepended as a `system` message (D11).
//!     Also hosts the D12 `preview_context` path: same reads,
//!     same gates, no model call — answers "what would ride along
//!     on the next send?" for the chat panel's context-preview
//!     area.
//!
//! Only `assemble` + `preview_context` (and their small request/
//! response types) are re-exported here. The reader, the redactor,
//! and the instructions probe stay inside the module so the chat
//! handler can't accidentally reach for the lower-level primitives.
//!
//! Out of scope:
//!   * Multi-file attachments.
//!   * Recursive directory attachments.
//!   * `README.md` auto-context, nested per-directory instruction
//!     files, `.plume/` overlays — those are roadmap.
//!   * The `propose-diff` / `scoped-edit` / `agent-loop` prompt
//!     shapes — those land with their respective slices.
//!   * Connection-string password redaction (deferred; see
//!     `docs/SAFETY.md § Secret handling`).

mod assemble;
mod instructions;
mod read;
mod redact;

pub use assemble::{
    assemble, preview_context, AttachmentPreviewOutcome, AttachmentRequest, LineRange,
};
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
