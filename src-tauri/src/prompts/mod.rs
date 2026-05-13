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
//!
//! Only `assemble` (and its small request/response types) is
//! re-exported here. The reader, the redactor, and the
//! instructions probe stay inside the module so the chat handler
//! can't accidentally reach for the lower-level primitives.
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

pub use assemble::{assemble, AttachmentRequest, LineRange};
// `AssembledPrompt` and `AttachmentSummary` are returned by
// `assemble`; callers access their fields without naming the types,
// so neither is re-exported. Keeping them out of the public surface
// also keeps the chat handler from accidentally consuming a
// summary outside the structured-tracing call it's used for today.
