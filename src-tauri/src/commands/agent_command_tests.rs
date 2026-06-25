//! Tests for the agent dry-run IPC (D93). The event ordering / terminal
//! state live in `agent::dry_run` tests; here we pin the command's wire
//! shape — the events array of flattened envelopes the frontend renders.

use super::*;

#[test]
fn response_serializes_events_as_flattened_envelopes() {
    let resp = AgentDryRunResponse {
        events: scripted_dry_run(1_700_000_000_000),
    };
    let v = serde_json::to_value(&resp).unwrap();
    let events = v["events"].as_array().expect("events array");
    assert!(!events.is_empty());

    // First frame: seq + tsMs + a flattened event (kind + payload).
    let first = &events[0];
    assert_eq!(first["seq"], 0);
    assert!(first["tsMs"].is_number());
    assert!(first["kind"].is_string());

    // Last frame is the terminal done.
    let last = events.last().unwrap();
    assert_eq!(last["kind"], "done");
}

#[test]
fn response_is_camel_case_and_carries_call_ids() {
    let resp = AgentDryRunResponse {
        events: scripted_dry_run(1),
    };
    let v = serde_json::to_value(&resp).unwrap();
    // A toolProposed frame uses camelCase callId on the wire.
    let proposed = v["events"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["kind"] == "toolProposed")
        .expect("a toolProposed frame");
    assert!(proposed["callId"].is_string());
}
