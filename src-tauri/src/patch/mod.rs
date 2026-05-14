//! D16: read-only `patch.validate` support.
//!
//! Plume's propose-diff mode (D15) lets the model emit a unified
//! diff in a fenced code block. D15 only renders it; D16 layers a
//! validator on top so the panel can show "valid diff · N files ·
//! M hunks" or "invalid diff: <reason>" under the rendered diff.
//!
//! What this module is:
//!   * `parse` — hand-rolled unified-diff parser. Accepts either a
//!     fenced ```diff/```patch block or a bare unified diff and
//!     returns one `ParsedFile` per `--- /+++ ` header pair, with
//!     hunk counts and a coarse change-type classification
//!     (create/delete/rename/modify). No new crate deps.
//!   * `validate` — orchestrator. Parses, then enforces project-
//!     root path safety on every file path the diff touches.
//!     Returns either an `Ok` summary or a list of structured
//!     errors. Both shapes go straight onto the wire — see
//!     `commands::patch`.
//!
//! What this module is NOT:
//!   * A patch applier. D16 still does NOT touch disk. The Apply
//!     button stays disabled even when validation passes; on-disk
//!     `patch.apply` is roadmap.
//!   * A semantic checker. We do not verify the diff's pre-image
//!     matches disk, we do not check hunk offsets line up with
//!     file contents, we do not detect overlapping hunks. Those
//!     belong to `patch.apply` when it lands.
//!   * A redactor. Diffs flow from the model back into the chat
//!     panel and then into this validator; they never come from
//!     disk through this path, so the secret-pattern redactor
//!     (which guards prompt-reads on the way OUT) is not involved.
//!
//! Path-safety strategy:
//!   * Lexical: reject absolute paths, reject any `..` component,
//!     reject NUL bytes, reject empty paths. These rules apply
//!     whether or not the file currently exists — a "create" diff
//!     for `src/new.rs` is fine even though the file isn't there
//!     yet.
//!   * Existing-file canonicalize: when the joined path EXISTS on
//!     disk, fall through to `safety::path::ensure_inside` so a
//!     symlinked-out file also gets caught.
//!   * `/dev/null` is a sentinel, not a path — it's only valid on
//!     one side of a header pair (create or delete).
//!
//! See `docs/IPC_CONTRACT.md § patch` for the wire shape and
//! `docs/SAFETY.md § Patch validation` for the boundary contract.

mod apply;
mod parse;
mod validate;

// Only the command handlers consume the public surface. Inner
// types (`ParsedFile`, `ParseError`, per-error helpers) stay
// private — production callers go through `validate_patch` /
// `apply_patch` and pattern-match on the response enums.
pub use apply::{apply_patch, PatchApplyResponse};
pub use validate::{validate_patch, PatchValidateResponse};
