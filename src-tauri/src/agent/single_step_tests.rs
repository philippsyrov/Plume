//! D96 single-step core tests — classification + event assembly, all pure
//! (no model, no server, no filesystem). The real MLX round-trip and the
//! `patch.validate_patch` bridge are exercised in `commands::agent` /
//! `agent::single_step`'s command-side tests and the Qwen smoke scripts.

use super::*;
use crate::agent::approval::{decide, ApprovalDecision, ApprovalLedger, ToolRequest};
use crate::agent::events::{AgentEvent, AgentToolKind};
use crate::agent::ApprovalPolicy;

const VALID_DIFF: &str = "--- a/greet.py\n\
    +++ b/greet.py\n\
    @@ -1,2 +1,2 @@\n\
     def greet(name):\n\
    -    return \"Hello, \" + name\n\
    +    return f\"Hello, {name}!\"\n";

fn valid_summary() -> ValidateSummary {
    ValidateSummary {
        valid: true,
        paths: vec!["greet.py".to_string()],
        detail: "1 file, 1 hunk".to_string(),
    }
}

fn kinds(events: &[crate::agent::events::AgentEventEnvelope]) -> Vec<&'static str> {
    events
        .iter()
        .map(|e| match &e.event {
            AgentEvent::MessageChunk { .. } => "messageChunk",
            AgentEvent::ToolProposed { .. } => "toolProposed",
            AgentEvent::ApprovalRequired { .. } => "approvalRequired",
            AgentEvent::ToolStarted { .. } => "toolStarted",
            AgentEvent::ToolFinished { .. } => "toolFinished",
            AgentEvent::ToolFailed { .. } => "toolFailed",
            AgentEvent::Paused { .. } => "paused",
            AgentEvent::Done { .. } => "done",
        })
        .collect()
}

// ─── classify_action ─────────────────────────────────────────────────────

#[test]
fn classifies_a_bare_unified_diff_as_propose_diff() {
    match classify_action(VALID_DIFF) {
        ProposedAction::ProposeDiff { diff } => assert_eq!(diff, VALID_DIFF),
        other => panic!("expected ProposeDiff, got {other:?}"),
    }
}

#[test]
fn classifies_a_fenced_diff_as_propose_diff() {
    // The ``` fence does not hide the ---/+++/@@ markers.
    let fenced = format!("```diff\n{VALID_DIFF}```\n");
    assert!(matches!(
        classify_action(&fenced),
        ProposedAction::ProposeDiff { .. }
    ));
}

#[test]
fn classifies_tool_request_sentinel_as_unsupported_tool() {
    let reply = "I need to look around first.\nTOOL_REQUEST: browser_open\n";
    match classify_action(reply) {
        ProposedAction::UnsupportedTool { name } => assert_eq!(name, "browser_open"),
        other => panic!("expected UnsupportedTool, got {other:?}"),
    }
}

#[test]
fn a_diff_wins_over_a_tool_request_line() {
    // A reply that is a diff AND happens to contain the sentinel text is
    // still the supported path — validate_patch is the final judge.
    let reply = format!("{VALID_DIFF}TOOL_REQUEST: browser_open\n");
    assert!(matches!(
        classify_action(&reply),
        ProposedAction::ProposeDiff { .. }
    ));
}

#[test]
fn classifies_plain_prose_as_no_action() {
    assert_eq!(
        classify_action("Sure, I can help with that. What file?"),
        ProposedAction::NoAction
    );
}

#[test]
fn partial_diff_markers_are_not_a_diff() {
    // A `+++` line with no `---` and no `@@` is not a diff.
    assert_eq!(
        classify_action("+++ here are my thoughts +++"),
        ProposedAction::NoAction
    );
}

// ─── build_single_step_events: valid propose-diff (the gated happy path) ──

#[test]
fn valid_diff_validates_then_gates_apply_and_pauses() {
    let action = ProposedAction::ProposeDiff {
        diff: VALID_DIFF.to_string(),
    };
    let events = build_single_step_events(
        42,
        VALID_DIFF,
        &action,
        Some(&valid_summary()),
        ApprovalDecision::Prompt,
    );

    assert_eq!(
        kinds(&events),
        vec![
            "messageChunk",
            "toolProposed", // validate (read-only)
            "toolStarted",
            "toolFinished",
            "toolProposed", // apply (write)
            "approvalRequired",
            "paused",
        ]
    );

    // seq is 0-based and strictly increasing; ts_ms is uniform.
    for (i, e) in events.iter().enumerate() {
        assert_eq!(e.seq, i as u64);
        assert_eq!(e.ts_ms, 42);
    }

    // The write is proposed and gated, never started — single-step never
    // applies. There is no ToolStarted/ToolFinished for the apply call.
    let apply_started = events.iter().any(|e| {
        matches!(&e.event, AgentEvent::ToolStarted { tool, .. } if *tool == AgentToolKind::Write)
    });
    assert!(!apply_started, "the write must never start in single-step");

    // The approval ask is on the write tool.
    let ask = events
        .iter()
        .find_map(|e| match &e.event {
            AgentEvent::ApprovalRequired { tool, .. } => Some(*tool),
            _ => None,
        })
        .expect("an approvalRequired frame");
    assert_eq!(ask, AgentToolKind::Write);
}

#[test]
fn allow_decision_still_does_not_apply() {
    // Even if a (hypothetical) policy returned Allow for the write,
    // single-step pauses without applying — no approvalRequired, no start.
    let action = ProposedAction::ProposeDiff {
        diff: VALID_DIFF.to_string(),
    };
    let events = build_single_step_events(
        1,
        VALID_DIFF,
        &action,
        Some(&valid_summary()),
        ApprovalDecision::Allow,
    );
    assert_eq!(
        kinds(&events),
        vec![
            "messageChunk",
            "toolProposed",
            "toolStarted",
            "toolFinished",
            "toolProposed",
            "paused",
        ]
    );
    let applied = events.iter().any(|e| {
        matches!(
            &e.event,
            AgentEvent::ToolStarted { tool, .. } | AgentEvent::ToolFinished { tool, .. }
                if *tool == AgentToolKind::Write
        )
    });
    assert!(!applied, "Allow must not cause an apply in single-step");
}

// ─── build_single_step_events: invalid diff ──────────────────────────────

#[test]
fn invalid_diff_fails_validate_and_ends_without_apply() {
    let action = ProposedAction::ProposeDiff {
        diff: "garbage".to_string(),
    };
    let invalid = ValidateSummary {
        valid: false,
        paths: Vec::new(),
        detail: "no diff block found".to_string(),
    };
    let events = build_single_step_events(
        7,
        "garbage",
        &action,
        Some(&invalid),
        ApprovalDecision::Prompt,
    );

    assert_eq!(
        kinds(&events),
        vec![
            "messageChunk",
            "toolProposed",
            "toolStarted",
            "toolFailed",
            "done",
        ]
    );
    // No write was ever proposed.
    let write_proposed = events.iter().any(|e| {
        matches!(&e.event, AgentEvent::ToolProposed { tool, .. } if *tool == AgentToolKind::Write)
    });
    assert!(!write_proposed, "an invalid diff must not propose an apply");
}

// ─── build_single_step_events: unsupported tool (the blocked path) ───────

#[test]
fn unsupported_tool_emits_a_blocked_failure_then_done() {
    let action = ProposedAction::UnsupportedTool {
        name: "browser_open".to_string(),
    };
    let events = build_single_step_events(
        9,
        "TOOL_REQUEST: browser_open",
        &action,
        None,
        ApprovalDecision::Prompt,
    );

    assert_eq!(
        kinds(&events),
        vec!["messageChunk", "toolProposed", "toolFailed", "done"]
    );
    // The blocked event names the tool and says why.
    let blocked = events
        .iter()
        .find_map(|e| match &e.event {
            AgentEvent::ToolFailed { error, .. } => Some(error.clone()),
            _ => None,
        })
        .expect("a toolFailed frame");
    assert!(blocked.contains("browser_open"));
    assert!(blocked.contains("not available"));
}

// ─── build_single_step_events: no action ─────────────────────────────────

#[test]
fn no_action_just_reports_and_finishes() {
    let events = build_single_step_events(
        3,
        "What would you like changed?",
        &ProposedAction::NoAction,
        None,
        ApprovalDecision::Prompt,
    );
    assert_eq!(kinds(&events), vec!["messageChunk", "done"]);
}

// ─── message truncation ──────────────────────────────────────────────────

#[test]
fn long_replies_are_truncated_in_the_message_chunk() {
    let long = "x".repeat(MAX_MESSAGE_CHARS + 500);
    let events = build_single_step_events(
        0,
        &long,
        &ProposedAction::NoAction,
        None,
        ApprovalDecision::Prompt,
    );
    match &events[0].event {
        AgentEvent::MessageChunk { text } => {
            assert!(text.ends_with("… (truncated)"));
            assert!(text.chars().count() <= MAX_MESSAGE_CHARS + "… (truncated)".chars().count());
        }
        other => panic!("expected MessageChunk, got {other:?}"),
    }
}

// ─── approval reuse: the apply write always prompts ──────────────────────

#[test]
fn applying_a_diff_prompts_under_every_policy() {
    // The gate single-step relies on: a write always prompts, so the apply
    // step is always surfaced as approvalRequired (never auto-run). This
    // pins single-step's dependency on `approval::decide`.
    let req = ToolRequest::Write {
        path: "greet.py".to_string(),
    };
    let empty = ApprovalLedger::new();
    for policy in [
        ApprovalPolicy::AskEach,
        ApprovalPolicy::AskOnWrite,
        ApprovalPolicy::AskOnFail,
    ] {
        assert_eq!(
            decide(policy, &req, &empty, None),
            ApprovalDecision::Prompt,
            "writes must always prompt under {policy:?}"
        );
    }
}
