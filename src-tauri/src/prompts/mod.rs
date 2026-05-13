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
//! What lives here (D8 scope):
//!   * `read` — `read_for_prompt(root, target)`: secret-filename
//!     block, size cap, binary detection, content redaction. Caller
//!     never sees raw bytes. Visibility is `pub(in crate::prompts)`
//!     so `assemble` is the only caller.
//!   * `redact` — pattern-based redactor for the secret formats in
//!     `docs/SAFETY.md § Secret handling`. Hand-rolled so we add no
//!     new crate deps.
//!   * `assemble` — wraps an optional file attachment into the last
//!     user message of a chat transcript.
//!
//! Only `assemble` (and its small request/response types) is
//! re-exported here. The reader and the redactor stay inside the
//! module so the chat handler can't accidentally reach for the
//! lower-level primitives.
//!
//! Out of scope for D8:
//!   * Multi-file attachments.
//!   * Recursive directory attachments.
//!   * The `propose-diff` / `scoped-edit` / `agent-loop` prompt
//!     shapes — those land with their respective slices.
//!   * Connection-string password redaction (deferred; see
//!     `docs/SAFETY.md § Secret handling`).

mod assemble;
mod read;
mod redact;

pub use assemble::{assemble, AttachmentRequest};
// `AssembledPrompt` and `AttachmentSummary` are returned by
// `assemble`; callers access their fields without naming the types,
// so neither is re-exported. Keeping them out of the public surface
// also keeps the chat handler from accidentally consuming a
// summary outside the structured-tracing call it's used for today.
