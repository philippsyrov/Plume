//! Tests for the agent event protocol scaffold (D85). Pin the wire shape
//! — the executing slice and the frontend renderer both depend on these
//! exact `kind` tags and camelCase fields — and prove round-trips.

use super::*;
use serde_json::json;

fn roundtrip(event: &AgentEvent) {
    let value = serde_json::to_value(event).expect("serialize");
    let back: AgentEvent = serde_json::from_value(value).expect("deserialize");
    assert_eq!(&back, event);
}

#[test]
fn message_chunk_shape() {
    let e = AgentEvent::MessageChunk {
        text: "hello".to_string(),
    };
    assert_eq!(
        serde_json::to_value(&e).unwrap(),
        json!({ "kind": "messageChunk", "text": "hello" })
    );
    roundtrip(&e);
}

#[test]
fn tool_lifecycle_shares_camel_case_call_id() {
    let proposed = AgentEvent::ToolProposed {
        call_id: "c1".to_string(),
        tool: AgentToolKind::Command,
        summary: "cargo test".to_string(),
    };
    assert_eq!(
        serde_json::to_value(&proposed).unwrap(),
        json!({
            "kind": "toolProposed",
            "callId": "c1",
            "tool": "command",
            "summary": "cargo test"
        })
    );

    let started = AgentEvent::ToolStarted {
        call_id: "c1".to_string(),
        tool: AgentToolKind::Command,
    };
    assert_eq!(
        serde_json::to_value(&started).unwrap(),
        json!({ "kind": "toolStarted", "callId": "c1", "tool": "command" })
    );

    let finished = AgentEvent::ToolFinished {
        call_id: "c1".to_string(),
        tool: AgentToolKind::Command,
        summary: "exit 0".to_string(),
    };
    assert_eq!(
        serde_json::to_value(&finished).unwrap(),
        json!({
            "kind": "toolFinished",
            "callId": "c1",
            "tool": "command",
            "summary": "exit 0"
        })
    );

    for e in [&proposed, &started, &finished] {
        roundtrip(e);
    }
}

#[test]
fn approval_required_reports_but_does_not_authorize() {
    let e = AgentEvent::ApprovalRequired {
        call_id: "c2".to_string(),
        tool: AgentToolKind::Write,
        prompt: "write src/main.rs?".to_string(),
    };
    let value = serde_json::to_value(&e).unwrap();
    assert_eq!(value["kind"], "approvalRequired");
    assert_eq!(value["tool"], "write");
    // It carries only a prompt — no decision / allow field.
    assert!(value.get("decision").is_none());
    assert!(value.get("allow").is_none());
    roundtrip(&e);
}

#[test]
fn tool_failed_shape() {
    let e = AgentEvent::ToolFailed {
        call_id: "c3".to_string(),
        tool: AgentToolKind::Command,
        error: "exit 1".to_string(),
    };
    assert_eq!(
        serde_json::to_value(&e).unwrap(),
        json!({ "kind": "toolFailed", "callId": "c3", "tool": "command", "error": "exit 1" })
    );
    roundtrip(&e);
}

#[test]
fn paused_and_done_mirror_loop_outcome_tags() {
    let paused = AgentEvent::Paused {
        reason: "awaiting approval".to_string(),
    };
    assert_eq!(
        serde_json::to_value(&paused).unwrap(),
        json!({ "kind": "paused", "reason": "awaiting approval" })
    );

    let done_with = AgentEvent::Done {
        summary: Some("all tests green".to_string()),
    };
    assert_eq!(
        serde_json::to_value(&done_with).unwrap(),
        json!({ "kind": "done", "summary": "all tests green" })
    );

    // None serializes as null, not an absent field.
    let done_none = AgentEvent::Done { summary: None };
    assert_eq!(
        serde_json::to_value(&done_none).unwrap(),
        json!({ "kind": "done", "summary": null })
    );

    roundtrip(&paused);
    roundtrip(&done_with);
    roundtrip(&done_none);
}

#[test]
fn all_tool_kinds_serialize_camel_case() {
    let pairs = [
        (AgentToolKind::Read, "read"),
        (AgentToolKind::Write, "write"),
        (AgentToolKind::Command, "command"),
        (AgentToolKind::Search, "search"),
        (AgentToolKind::Other, "other"),
    ];
    for (kind, wire) in pairs {
        assert_eq!(serde_json::to_value(kind).unwrap(), json!(wire));
    }
}

#[test]
fn envelope_flattens_event_with_seq_and_ts() {
    let env = AgentEventEnvelope::new(
        3,
        1_700_000_000_000,
        AgentEvent::MessageChunk {
            text: "hi".to_string(),
        },
    );
    assert_eq!(
        serde_json::to_value(&env).unwrap(),
        json!({
            "seq": 3,
            "tsMs": 1_700_000_000_000_u64,
            "kind": "messageChunk",
            "text": "hi"
        })
    );

    // Round-trips back to the same envelope.
    let value = serde_json::to_value(&env).unwrap();
    let back: AgentEventEnvelope = serde_json::from_value(value).expect("deserialize");
    assert_eq!(back, env);
}
