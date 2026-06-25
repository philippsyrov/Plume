//! Agent event dry-run (D93) — plumbing proof for the D85 event stream.
//!
//! A deterministic, dev-only "agent run" that emits a fixed sequence of
//! typed [`AgentEvent`]s (wrapped in [`AgentEventEnvelope`]s with monotonic
//! `seq`) **without running anything real** — no model, no shell, no patch,
//! no file writes. Its only job is to prove the typed stream can drive the
//! existing `AgentEventLog` surface end to end (IPC → state → render).
//!
//! The script walks every event kind through a realistic shape: the model
//! speaks, proposes a tool, the gate stops for approval, the tool runs and
//! finishes, repeat, then a terminal `Done`. Tool lifecycle events share a
//! `callId` so the UI can correlate them. `approvalRequired` only *reports*
//! the stop — it authorizes nothing (there is nothing to authorize here).
//!
//! When the real loop controller (D79) drives real tools, it will emit
//! these same shapes; this module is the contract rehearsal.

use super::events::{AgentEvent, AgentEventEnvelope, AgentToolKind};

/// Build the scripted dry-run stream. `now_ms` stamps every frame (the
/// same wall-clock for all — a dry run has no real elapsed time); `seq`
/// is 0-based and strictly increasing. The last frame is always `Done`.
pub fn scripted_dry_run(now_ms: u64) -> Vec<AgentEventEnvelope> {
    let mut out: Vec<AgentEventEnvelope> = Vec::new();
    let mut seq: u64 = 0;
    let mut push = |ev: AgentEvent| {
        out.push(AgentEventEnvelope::new(seq, now_ms, ev));
        seq += 1;
    };

    // Turn 1 — read-only search, auto-approved (read tools don't prompt).
    push(AgentEvent::MessageChunk {
        text: "Looking at the project to find the TODO.".to_string(),
    });
    push(AgentEvent::ToolProposed {
        call_id: "t1".to_string(),
        tool: AgentToolKind::Search,
        summary: "search the project for \"TODO\"".to_string(),
    });
    push(AgentEvent::ToolStarted {
        call_id: "t1".to_string(),
        tool: AgentToolKind::Search,
    });
    push(AgentEvent::ToolFinished {
        call_id: "t1".to_string(),
        tool: AgentToolKind::Search,
        summary: "3 matches in 2 files".to_string(),
    });

    // Turn 2 — a write that stops for approval, then runs.
    push(AgentEvent::MessageChunk {
        text: "I'll fix the first one in src/lib.rs.".to_string(),
    });
    push(AgentEvent::ToolProposed {
        call_id: "t2".to_string(),
        tool: AgentToolKind::Write,
        summary: "edit src/lib.rs (1 hunk)".to_string(),
    });
    push(AgentEvent::ApprovalRequired {
        call_id: "t2".to_string(),
        tool: AgentToolKind::Write,
        prompt: "Apply a 1-hunk edit to src/lib.rs?".to_string(),
    });
    push(AgentEvent::ToolStarted {
        call_id: "t2".to_string(),
        tool: AgentToolKind::Write,
    });
    push(AgentEvent::ToolFinished {
        call_id: "t2".to_string(),
        tool: AgentToolKind::Write,
        summary: "applied 1 hunk".to_string(),
    });

    // Turn 3 — run the verifier; it fails once, then the run ends.
    push(AgentEvent::MessageChunk {
        text: "Running the verifier.".to_string(),
    });
    push(AgentEvent::ToolProposed {
        call_id: "t3".to_string(),
        tool: AgentToolKind::Command,
        summary: "cargo test".to_string(),
    });
    push(AgentEvent::ToolStarted {
        call_id: "t3".to_string(),
        tool: AgentToolKind::Command,
    });
    push(AgentEvent::ToolFailed {
        call_id: "t3".to_string(),
        tool: AgentToolKind::Command,
        error: "1 test failed: greet_formats_name".to_string(),
    });
    push(AgentEvent::Paused {
        reason: "verifier failed — awaiting a fix decision".to_string(),
    });
    push(AgentEvent::Done {
        summary: Some("dry run complete (no real tools ran)".to_string()),
    });

    out
}

#[cfg(test)]
#[path = "dry_run_tests.rs"]
mod tests;
