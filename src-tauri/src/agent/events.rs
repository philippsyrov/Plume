//! Agent event protocol scaffold (D85) — agent-loop slice 5.
//!
//! The typed shapes a future agent run will stream to the UI. When the
//! loop controller (D79) drives a real step — model turn → tool proposed
//! → approval gate → execute → observe — each transition emits one
//! [`AgentEvent`], wrapped in an [`AgentEventEnvelope`] carrying a
//! monotonic `seq` and a timestamp. This mirrors the Hermes-style
//! structured stream from `docs/HERMES_AGENT_RESEARCH.md`: the frontend
//! renders a live transcript without parsing free text, and an
//! out-of-order or dropped frame is detectable via `seq`.
//!
//! **Scaffold only.** Nothing emits these yet — there is no model turn,
//! no tool execution, no IPC channel. This slice fixes the wire vocabulary
//! (backend types here, the frontend renderer skeleton in
//! `src/features/agent/AgentEventLog.tsx`) so the executing slice wires a
//! channel into shapes both ends already agree on. Hence `allow(dead_code)`
//! until that slice consumes it.
//!
//! The events deliberately carry only *descriptive* fields (a `callId` to
//! correlate a tool's lifecycle, a human `summary`, an `error` string) —
//! never the decision itself. Whether a proposed tool may run is the
//! approval gate's call (`agent::approval`); an `ApprovalRequired` event
//! only reports that the run stopped to ask, it does not pre-authorize.

#![allow(dead_code)]

use serde::{Deserialize, Serialize};

/// Coarse category of a tool the agent proposes / runs. Kept independent
/// of `agent::approval::ToolRequest` (which carries the *gating* detail);
/// this is only what the transcript needs to label a row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentToolKind {
    /// A read-only tool (fs read, grep, git status).
    Read,
    /// A write to a project-relative path.
    Write,
    /// A shell command.
    Command,
    /// A search / retrieval tool.
    Search,
    /// Anything not yet categorized.
    Other,
}

/// One event in an agent run's stream. Serializes as an internally-tagged
/// union (`{ "kind": "toolStarted", "callId": "…", "tool": "command" }`),
/// matching the `LoopOutcome` tagging convention so a single `kind`
/// discriminator drives both the controller's terminal state and the
/// transcript.
///
/// Lifecycle of a tool call: `toolProposed` → (optional `approvalRequired`
/// → resume) → `toolStarted` → `toolFinished` | `toolFailed`. All four
/// share a `callId` so the UI can collapse them into one row. `paused` /
/// `done` are run-level terminals that mirror `LoopOutcome`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "kind"
)]
pub enum AgentEvent {
    /// A delta of the assistant's streamed message text.
    MessageChunk { text: String },
    /// The model wants to call a tool; not yet gated or run.
    ToolProposed {
        call_id: String,
        tool: AgentToolKind,
        summary: String,
    },
    /// The approval gate stopped the run to ask the user about a proposed
    /// tool. Reports the stop; it does **not** authorize anything.
    ApprovalRequired {
        call_id: String,
        tool: AgentToolKind,
        prompt: String,
    },
    /// An approved tool began executing.
    ToolStarted {
        call_id: String,
        tool: AgentToolKind,
    },
    /// A tool finished successfully.
    ToolFinished {
        call_id: String,
        tool: AgentToolKind,
        summary: String,
    },
    /// A tool failed. The run fails closed (the loop does not self-retry).
    ToolFailed {
        call_id: String,
        tool: AgentToolKind,
        error: String,
    },
    /// The run yielded for the user (an approval prompt or a question).
    /// Mirrors `LoopOutcome::Paused`.
    Paused { reason: String },
    /// The run ended. Mirrors `LoopOutcome::Done`; `summary` is an
    /// optional closing note.
    Done { summary: Option<String> },
}

/// A stream frame: a monotonic sequence number and a timestamp wrapping
/// one [`AgentEvent`]. The event is flattened, so the wire shape is the
/// event's fields plus `seq` + `tsMs`:
/// `{ "seq": 3, "tsMs": 1700000000000, "kind": "messageChunk", "text": "…" }`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentEventEnvelope {
    /// 0-based, strictly increasing within a run. A gap signals a dropped
    /// frame; a repeat signals a replay.
    pub seq: u64,
    /// Unix epoch ms when the event was emitted (same clock as memory /
    /// ledger).
    pub ts_ms: u64,
    #[serde(flatten)]
    pub event: AgentEvent,
}

impl AgentEventEnvelope {
    pub fn new(seq: u64, ts_ms: u64, event: AgentEvent) -> Self {
        Self { seq, ts_ms, event }
    }
}

#[cfg(test)]
#[path = "events_tests.rs"]
mod tests;
