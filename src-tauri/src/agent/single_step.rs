//! D96: single-step local agent — the pure core.
//!
//! This is the first slice where the agent *does* something with a real
//! model turn, but it stays deliberately tiny: one step, one safe action
//! path. The I/O shell (`commands::agent::agent_single_step`) does the MLX
//! round-trip and runs the real `patch::validate_patch`; everything that
//! decides *what the transcript says* lives here, as pure functions over
//! plain data so the whole step is unit-testable without a model, a
//! server, or the filesystem.
//!
//! The contract:
//!
//!   1. The model is asked (propose-diff prompt) to reply with ONLY a
//!      unified diff. [`classify_action`] reads that reply into one of
//!      three outcomes:
//!        * [`ProposedAction::ProposeDiff`] — a diff-shaped reply. The
//!          supported path.
//!        * [`ProposedAction::UnsupportedTool`] — the model explicitly
//!          asked for some other tool (via the `TOOL_REQUEST:` sentinel
//!          the prompt documents). Blocked — only propose-diff is wired.
//!        * [`ProposedAction::NoAction`] — prose with no actionable diff.
//!   2. [`build_single_step_events`] turns the reply + classification +
//!      (for a diff) the validate outcome and the approval gate's verdict
//!      into the typed [`AgentEvent`] stream the existing `AgentEventLog`
//!      already renders.
//!
//! What this core deliberately does NOT do, matching the slice's gates:
//!   * It never *applies* a diff. Applying is a write; under every
//!     approval policy `approval::decide` returns `Prompt` for a write,
//!     and single-step never auto-executes one regardless — so the apply
//!     step is always surfaced as `ApprovalRequired` and the run
//!     `Paused`. The only thing that actually runs is `patch.validate`,
//!     which writes nothing.
//!   * It never runs a shell command, browses, or recurses. An
//!     unsupported tool request becomes a `ToolFailed` (the "blocked"
//!     event) and the run ends.

use super::approval::ApprovalDecision;
use super::events::{AgentEvent, AgentEventEnvelope, AgentToolKind};
use super::AgentMode;

/// Hard cap on the assistant text echoed into the opening `MessageChunk`.
/// A propose-diff reply *is* the diff, which can be long; the transcript
/// only needs enough to show what the model said, not the whole payload
/// (the diff is validated and summarized separately).
pub const MAX_MESSAGE_CHARS: usize = 2000;

/// Stable call ids for the at-most-two tool calls a single step can
/// surface. Deterministic (no clock / counter) so tests can assert the
/// exact wire shape.
const VALIDATE_CALL_ID: &str = "validate-1";
const APPLY_CALL_ID: &str = "apply-1";
const BLOCKED_CALL_ID: &str = "blocked-1";

/// What the model's reply asks the agent to do, as classified for this
/// step. Independent of approval — that gate runs after, on the apply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProposedAction {
    /// The reply is a unified diff (fenced or bare). The supported path:
    /// validate it, then surface apply behind the approval gate.
    ProposeDiff { diff: String },
    /// The reply explicitly requested a tool other than propose-diff via
    /// the documented `TOOL_REQUEST:` sentinel. Blocked in this slice.
    UnsupportedTool { name: String },
    /// Neither a diff nor a tool request — prose with nothing to run.
    NoAction,
}

/// Outcome of running the model's diff through `patch::validate_patch`,
/// flattened to just what the transcript needs. The command layer builds
/// this from a `PatchValidateResponse`; this module never touches the
/// patch types so it stays pure and dependency-light.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidateSummary {
    /// Did the diff pass shape + path-safety validation?
    pub valid: bool,
    /// Project-relative paths the diff touches (empty when invalid).
    pub paths: Vec<String>,
    /// Human one-liner — "2 file(s), 3 hunk(s)" on success, the headline
    /// validation error on failure.
    pub detail: String,
}

/// Whether the session's `agentMode` permits running a single step.
///
/// The mode axis ("what the model may do") gates this independently of the
/// approval policy ("when the user is asked") — see
/// `docs/SAFETY.md § "Agent autonomy is two independent axes"`. `chat` is
/// talk-only, so a step that asks the model to *propose a diff* requires
/// `propose-diff` or higher; the gear selector and the engine must agree.
/// The command checks this before it ever talks to the model.
pub fn mode_allows_step(mode: AgentMode) -> bool {
    match mode {
        AgentMode::Chat => false,
        AgentMode::ProposeDiff | AgentMode::ScopedEdit | AgentMode::AgentLoop => true,
    }
}

/// Classify a model reply into a [`ProposedAction`].
///
/// A diff-shaped reply wins over a sentinel — a real diff is the supported
/// path, and `patch::validate_patch` is the final judge of whether it is
/// actually valid (this only decides *which kind* of reply it is). The
/// `TOOL_REQUEST:` sentinel is checked only when the reply is not a diff.
pub fn classify_action(reply: &str) -> ProposedAction {
    if looks_like_unified_diff(reply) {
        return ProposedAction::ProposeDiff {
            diff: reply.to_string(),
        };
    }
    if let Some(name) = explicit_tool_request(reply) {
        return ProposedAction::UnsupportedTool { name };
    }
    ProposedAction::NoAction
}

/// Heuristic diff detector: a `--- ` line, a `+++ ` line, and at least one
/// `@@` hunk header anywhere in the reply (fenced or bare — a ``` fence
/// doesn't hide these markers). Lenient on purpose; `validate_patch` does
/// the strict parse. We only need to tell "the model tried to send a diff"
/// apart from "the model said something else".
fn looks_like_unified_diff(reply: &str) -> bool {
    let mut minus = false;
    let mut plus = false;
    let mut hunk = false;
    for line in reply.lines() {
        let t = line.trim_start();
        if t.starts_with("--- ") {
            minus = true;
        } else if t.starts_with("+++ ") {
            plus = true;
        } else if t.starts_with("@@") {
            hunk = true;
        }
        if minus && plus && hunk {
            return true;
        }
    }
    false
}

/// Extract a `TOOL_REQUEST: <name>` sentinel if the reply contains one.
/// The propose-diff prompt tells the model to use this exact line when it
/// wants a tool it can't express as a diff, which (a) gives a model an
/// honest escape hatch instead of hallucinating a diff, and (b) gives the
/// blocked-path a deterministic trigger to test. The name is the first
/// whitespace-delimited token after the colon.
fn explicit_tool_request(reply: &str) -> Option<String> {
    for line in reply.lines() {
        let t = line.trim();
        // Match the sentinel on this line; skip lines that aren't it.
        let Some(rest) = t
            .strip_prefix("TOOL_REQUEST:")
            .or_else(|| t.strip_prefix("tool_request:"))
        else {
            continue;
        };
        let name = rest.split_whitespace().next().unwrap_or("");
        if !name.is_empty() {
            return Some(name.to_string());
        }
    }
    None
}

/// Truncate the assistant reply for the opening `MessageChunk`. Keeps the
/// transcript readable when the reply is a large diff.
fn message_text(reply: &str) -> String {
    let trimmed = reply.trim();
    if trimmed.chars().count() <= MAX_MESSAGE_CHARS {
        return trimmed.to_string();
    }
    let head: String = trimmed.chars().take(MAX_MESSAGE_CHARS).collect();
    format!("{head}… (truncated)")
}

/// Tiny sequence builder: assigns a 0-based, strictly-increasing `seq` and
/// a uniform `ts_ms` to each event, exactly like `dry_run`.
struct Stream {
    now_ms: u64,
    seq: u64,
    out: Vec<AgentEventEnvelope>,
}

impl Stream {
    fn new(now_ms: u64) -> Self {
        Self {
            now_ms,
            seq: 0,
            out: Vec::new(),
        }
    }

    fn push(&mut self, event: AgentEvent) {
        self.out
            .push(AgentEventEnvelope::new(self.seq, self.now_ms, event));
        self.seq += 1;
    }
}

/// Assemble the typed event stream for one step.
///
/// `validate` is `Some` only for [`ProposedAction::ProposeDiff`] (the
/// command runs `patch.validate_patch` and passes the summary).
/// `apply_decision` is the approval gate's verdict on *applying* the
/// validated diff — `Prompt` under every real policy (writes always
/// prompt). When `Allow` (not reachable from today's policies, but kept
/// total), single-step still does not apply: it surfaces the proposal and
/// pauses, just without an explicit approval ask.
///
/// Terminal frame is `Paused` when the run stops awaiting the user's
/// apply approval, and `Done` otherwise (invalid diff, blocked tool, or no
/// action) — mirroring `LoopOutcome::Paused` / `Done`.
pub fn build_single_step_events(
    now_ms: u64,
    reply: &str,
    action: &ProposedAction,
    validate: Option<&ValidateSummary>,
    apply_decision: ApprovalDecision,
) -> Vec<AgentEventEnvelope> {
    let mut s = Stream::new(now_ms);
    s.push(AgentEvent::MessageChunk {
        text: message_text(reply),
    });

    match action {
        ProposedAction::ProposeDiff { .. } => {
            // The one thing single-step actually executes: validate. It is
            // read-only (writes nothing), so it runs without a gate.
            s.push(AgentEvent::ToolProposed {
                call_id: VALIDATE_CALL_ID.to_string(),
                tool: AgentToolKind::Read,
                summary: "validate the proposed diff".to_string(),
            });
            s.push(AgentEvent::ToolStarted {
                call_id: VALIDATE_CALL_ID.to_string(),
                tool: AgentToolKind::Read,
            });

            let v = validate.cloned().unwrap_or(ValidateSummary {
                valid: false,
                paths: Vec::new(),
                detail: "no validation result".to_string(),
            });

            if v.valid {
                s.push(AgentEvent::ToolFinished {
                    call_id: VALIDATE_CALL_ID.to_string(),
                    tool: AgentToolKind::Read,
                    summary: format!("diff is valid — {}", v.detail),
                });
                push_apply_gate(&mut s, &v, apply_decision);
            } else {
                // Invalid model diff: a model-quality outcome, not a Plume
                // bug, and nothing was written. Fail this tool, end the run.
                s.push(AgentEvent::ToolFailed {
                    call_id: VALIDATE_CALL_ID.to_string(),
                    tool: AgentToolKind::Read,
                    error: format!("diff did not validate — {}", v.detail),
                });
                s.push(AgentEvent::Done {
                    summary: Some(
                        "the model's diff did not validate; nothing was applied".to_string(),
                    ),
                });
            }
        }
        ProposedAction::UnsupportedTool { name } => {
            // The "blocked" path: surface the request, then fail it. We
            // never run anything but propose-diff in this slice.
            s.push(AgentEvent::ToolProposed {
                call_id: BLOCKED_CALL_ID.to_string(),
                tool: AgentToolKind::Other,
                summary: format!("model requested tool '{name}'"),
            });
            s.push(AgentEvent::ToolFailed {
                call_id: BLOCKED_CALL_ID.to_string(),
                tool: AgentToolKind::Other,
                error: format!(
                    "tool '{name}' is not available in single-step mode (propose-diff only)"
                ),
            });
            s.push(AgentEvent::Done {
                summary: Some(format!("blocked an unsupported tool request: '{name}'")),
            });
        }
        ProposedAction::NoAction => {
            s.push(AgentEvent::Done {
                summary: Some("the model did not propose a diff; nothing to apply".to_string()),
            });
        }
    }

    s.out
}

/// Surface applying the validated diff behind the approval gate. Applying
/// is a write, so it is proposed but never executed here; the run pauses
/// for the user. Whether we explicitly *ask* depends on the gate verdict.
fn push_apply_gate(s: &mut Stream, v: &ValidateSummary, decision: ApprovalDecision) {
    let target = if v.paths.is_empty() {
        "the project".to_string()
    } else {
        v.paths.join(", ")
    };
    s.push(AgentEvent::ToolProposed {
        call_id: APPLY_CALL_ID.to_string(),
        tool: AgentToolKind::Write,
        summary: format!("apply the diff to {target}"),
    });
    match decision {
        ApprovalDecision::Prompt => {
            s.push(AgentEvent::ApprovalRequired {
                call_id: APPLY_CALL_ID.to_string(),
                tool: AgentToolKind::Write,
                prompt: format!(
                    "Apply this diff to {target}? Validation passed ({}); applying writes files.",
                    v.detail
                ),
            });
            s.push(AgentEvent::Paused {
                reason: "waiting for approval to apply the proposed diff".to_string(),
            });
        }
        ApprovalDecision::Allow => {
            // Policy would not prompt, but single-step never auto-applies a
            // write. Pause for the user without an approval ask.
            s.push(AgentEvent::Paused {
                reason: "diff validated; apply is left to you — single-step never writes"
                    .to_string(),
            });
        }
    }
}

#[cfg(test)]
#[path = "single_step_tests.rs"]
mod tests;
