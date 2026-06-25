//! Tests for the agent event dry-run (D93). Pin ordering, the terminal
//! state, and tool-lifecycle correlation so the stream stays renderable.

use super::*;

const NOW: u64 = 1_700_000_000_000;

#[test]
fn seqs_are_zero_based_and_strictly_increasing() {
    let stream = scripted_dry_run(NOW);
    assert!(!stream.is_empty());
    for (i, env) in stream.iter().enumerate() {
        assert_eq!(env.seq, i as u64, "seq must be the 0-based index");
        assert_eq!(env.ts_ms, NOW, "every frame carries the run timestamp");
    }
}

#[test]
fn the_last_event_is_a_terminal_done() {
    let stream = scripted_dry_run(NOW);
    let last = &stream.last().unwrap().event;
    assert!(
        matches!(last, AgentEvent::Done { .. }),
        "stream must end in Done, got {last:?}"
    );
    // Done appears exactly once and only at the end.
    let done_count = stream
        .iter()
        .filter(|e| matches!(e.event, AgentEvent::Done { .. }))
        .count();
    assert_eq!(done_count, 1, "exactly one terminal Done");
}

#[test]
fn covers_every_event_kind() {
    let stream = scripted_dry_run(NOW);
    let has = |pred: fn(&AgentEvent) -> bool| stream.iter().any(|e| pred(&e.event));
    assert!(has(|e| matches!(e, AgentEvent::MessageChunk { .. })));
    assert!(has(|e| matches!(e, AgentEvent::ToolProposed { .. })));
    assert!(has(|e| matches!(e, AgentEvent::ApprovalRequired { .. })));
    assert!(has(|e| matches!(e, AgentEvent::ToolStarted { .. })));
    assert!(has(|e| matches!(e, AgentEvent::ToolFinished { .. })));
    assert!(has(|e| matches!(e, AgentEvent::ToolFailed { .. })));
    assert!(has(|e| matches!(e, AgentEvent::Paused { .. })));
    assert!(has(|e| matches!(e, AgentEvent::Done { .. })));
}

#[test]
fn every_proposed_tool_reaches_a_terminal_lifecycle_event() {
    let stream = scripted_dry_run(NOW);
    // Collect proposed call ids, then assert each one later finishes or
    // fails — no tool is left "proposed" forever.
    let proposed: Vec<&str> = stream
        .iter()
        .filter_map(|e| match &e.event {
            AgentEvent::ToolProposed { call_id, .. } => Some(call_id.as_str()),
            _ => None,
        })
        .collect();
    assert!(!proposed.is_empty());
    for id in proposed {
        let terminal = stream.iter().any(|e| match &e.event {
            AgentEvent::ToolFinished { call_id, .. } | AgentEvent::ToolFailed { call_id, .. } => {
                call_id == id
            }
            _ => false,
        });
        assert!(terminal, "call {id} never finished or failed");
    }
}

#[test]
fn approval_required_precedes_its_tool_started() {
    // For the write tool (t2), the approval stop must come before the run.
    let stream = scripted_dry_run(NOW);
    let approval_idx = stream.iter().position(
        |e| matches!(&e.event, AgentEvent::ApprovalRequired { call_id, .. } if call_id == "t2"),
    );
    let started_idx = stream.iter().position(
        |e| matches!(&e.event, AgentEvent::ToolStarted { call_id, .. } if call_id == "t2"),
    );
    assert!(approval_idx.is_some() && started_idx.is_some());
    assert!(approval_idx < started_idx, "approval must precede start");
}

#[test]
fn is_deterministic() {
    assert_eq!(scripted_dry_run(NOW), scripted_dry_run(NOW));
}
