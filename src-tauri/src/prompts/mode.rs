//! Chat mode (D15).
//!
//! Defines the top-level "what shape should the model's response
//! take?" switch carried on every `chat.send`. Today there are two
//! modes:
//!
//!   * `Chat` — the existing D7.1 free-form text streaming path.
//!     No system message is prepended on behalf of the mode; the
//!     model responds however it likes.
//!
//!   * `ProposeDiff` — the model is instructed to respond with a
//!     unified diff inside a single fenced code block. Plume
//!     renders the diff with per-line coloring and exposes a
//!     *disabled* Apply button next to the rendered preview.
//!     **No file writes happen as a result of this mode.** The
//!     boundary between "preview" and "apply" is exactly where
//!     D15 stops; the apply path is roadmap (`docs/IPC_ROADMAP.md
//!     § Patch checkpoint / revert`).
//!
//! The enum is `#[derive(Default)]` with `Chat` as the default so
//! a payload that omits `mode` keeps the D7.1 wire shape exactly
//! — additive contract, no breaking change for older frontends.

use crate::chat::{ChatMessage, ChatRole};

/// The top-level shape of the model response Plume wants.
///
/// Wire form is camelCase: `"chat"` / `"proposeDiff"` (serde
/// `rename_all` applies to variant names of this unit enum).
/// New variants are additive — any unrecognised mode the backend
/// receives is rejected with `BadArgument` at the handler so the
/// frontend learns about the typo before a stream is registered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ChatMode {
    /// Free-form text response (D7.1 default).
    #[default]
    Chat,
    /// Unified-diff preview response (D15). System message
    /// prepended by the assembler; rendering is the frontend's
    /// responsibility; **applying is out of scope**.
    ProposeDiff,
}

/// Build the system message that pins the model to the
/// propose-diff response shape. Pulled into its own function so
/// the prompt text lives in one auditable place and a future
/// `scoped-edit` / `agent-loop` mode can layer alongside without
/// touching the assembler's control flow.
///
/// The wording deliberately:
/// - Constrains output to a SINGLE fenced ```diff block — keeps
///   the frontend parser simple and the boundary between "model
///   said yes" and "model said no" easy to detect.
/// - Names the git unified-diff format (`--- a/`, `+++ b/`, `@@`)
///   so models default to a parseable shape rather than free-form
///   pseudocode.
/// - Tells the model PLUME WILL NOT APPLY the diff. The model is
///   not the right place to enforce safety, but stating the
///   contract here keeps it consistent in summarisation responses
///   ("I produced a diff — review and apply manually").
/// - Provides an explicit fallback (`text` fence) for "I can't
///   produce a diff here's why" so models that refuse the request
///   surface readable prose instead of an empty `diff` fence.
pub fn propose_diff_system_message() -> ChatMessage {
    let body = "You are in \"propose-diff\" mode. Respond with a UNIFIED DIFF inside a single fenced ```diff code block. No prose before or after the fence.\n\
\n\
Use git-style headers: `--- a/<path>` and `+++ b/<path>`, then `@@ -<start>,<count> +<start>,<count> @@` hunk markers. Quote 2-3 lines of unchanged context above and below each change so the diff is easy to apply by hand.\n\
\n\
Plume will NOT apply this diff automatically. The user reviews the rendered preview and decides whether to apply by hand. Be precise: an unappliable diff (wrong line numbers, missing context) wastes the user's time.\n\
\n\
If you cannot produce a diff — the request is ambiguous, you need to inspect a file you weren't given, or the change would conflict with itself — respond instead with a single fenced ```text code block explaining what you need. Do not mix prose and diff."
        .to_string();
    ChatMessage {
        role: ChatRole::System,
        content: body,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chat_mode_defaults_to_chat() {
        // The serde `#[default]` is what makes a payload with the
        // `mode` field omitted decode to the D7.1 behaviour. Pin
        // the default so a future refactor that moves the
        // attribute doesn't silently flip newer frontends into
        // propose-diff.
        assert!(matches!(ChatMode::default(), ChatMode::Chat));
    }

    #[test]
    fn chat_mode_deserializes_chat_in_camel_case() {
        let m: ChatMode = serde_json::from_str("\"chat\"").expect("chat must parse");
        assert!(matches!(m, ChatMode::Chat));
    }

    #[test]
    fn chat_mode_deserializes_propose_diff_in_camel_case() {
        let m: ChatMode = serde_json::from_str("\"proposeDiff\"").expect("proposeDiff must parse");
        assert!(matches!(m, ChatMode::ProposeDiff));
    }

    #[test]
    fn chat_mode_rejects_snake_case_propose_diff() {
        // `propose_diff` on the wire is exactly the kind of regression
        // the D12 wire-shape pin caught for `AttachmentPayload`. Pin
        // it here too so a future refactor that drops `rename_all`
        // is caught at unit-test time, not by Codex smoke.
        let err = serde_json::from_str::<ChatMode>("\"propose_diff\"")
            .expect_err("snake_case must be rejected");
        let msg = err.to_string();
        assert!(
            msg.contains("propose_diff") || msg.contains("variant"),
            "expected variant-name error, got: {msg}"
        );
    }

    #[test]
    fn chat_mode_rejects_unknown_variant() {
        // A frontend bug or a wire-shape skew should reject loudly.
        // We surface this as `BadArgument` at the handler; the
        // serde-level rejection is the upstream half of that.
        let err =
            serde_json::from_str::<ChatMode>("\"somethingElse\"").expect_err("unknown rejects");
        let msg = err.to_string();
        assert!(
            msg.contains("variant") || msg.contains("somethingElse"),
            "expected unknown-variant error, got: {msg}"
        );
    }

    #[test]
    fn propose_diff_system_message_pins_format_and_boundary() {
        // The prompt text is contract: the model is steered toward
        // unified diff, told Plume won't apply, and given a
        // structured fallback. Any future edit to the text MUST
        // keep all three properties. A drift here is the most
        // likely source of "model wandered off and gave us prose
        // instead of a diff" reports.
        let msg = propose_diff_system_message();
        assert!(matches!(msg.role, ChatRole::System));
        let body = msg.content;
        assert!(body.contains("propose-diff"));
        assert!(body.contains("UNIFIED DIFF"));
        assert!(body.contains("```diff"));
        assert!(body.contains("NOT apply"));
        assert!(body.contains("```text"));
    }
}
