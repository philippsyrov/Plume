//! Tests for the session config command layer (D77). The handlers
//! themselves need a Tauri `AppState`, so we test the pure transition
//! (`apply_change`) and payload deserialization directly.

use super::*;

#[test]
fn set_mode_payload_camel_case() {
    let p: SetModePayload =
        serde_json::from_value(serde_json::json!({ "mode": "propose-diff" })).unwrap();
    assert_eq!(p.mode, AgentMode::ProposeDiff);
}

#[test]
fn set_approval_policy_payload_camel_case() {
    let p: SetApprovalPolicyPayload =
        serde_json::from_value(serde_json::json!({ "approvalPolicy": "ask-on-fail" })).unwrap();
    assert_eq!(p.approval_policy, ApprovalPolicy::AskOnFail);
}

#[test]
fn set_allowlist_payload_camel_case() {
    let p: SetAllowlistPayload = serde_json::from_value(serde_json::json!({
        "fileAllowlist": ["src/"],
        "commandAllowlist": [["cargo", "test"]],
        "iterationCap": 5
    }))
    .unwrap();
    assert_eq!(p.file_allowlist, vec!["src/".to_string()]);
    assert_eq!(
        p.command_allowlist,
        vec![vec!["cargo".to_string(), "test".to_string()]]
    );
    assert_eq!(p.iteration_cap, Some(5));
}

#[test]
fn set_allowlist_payload_rejects_snake_case() {
    let r = serde_json::from_value::<SetAllowlistPayload>(serde_json::json!({
        "file_allowlist": [],
        "command_allowlist": [],
        "iteration_cap": null
    }));
    assert!(r.is_err(), "snake_case must not deserialise");
}

#[test]
fn apply_change_rejects_agent_loop_without_gates() {
    let res = apply_change(&AgentConfig::default(), |c| c.mode = AgentMode::AgentLoop);
    let reasons = res.expect_err("agent-loop on a bare default must be refused");
    assert_eq!(reasons.len(), 3);
}

#[test]
fn apply_change_allows_agent_loop_after_gates_set() {
    let gated = apply_change(&AgentConfig::default(), |c| {
        c.file_allowlist = vec!["src/".into()];
        c.command_allowlist = vec![vec!["cargo".into(), "test".into()]];
        c.iteration_cap = Some(8);
    })
    .expect("setting gates is valid");
    let looped =
        apply_change(&gated, |c| c.mode = AgentMode::AgentLoop).expect("agent-loop now valid");
    assert_eq!(looped.mode, AgentMode::AgentLoop);
}

#[test]
fn apply_change_refuses_stripping_gates_while_in_agent_loop() {
    let looped = AgentConfig {
        mode: AgentMode::AgentLoop,
        approval_policy: ApprovalPolicy::AskOnFail,
        file_allowlist: vec!["src/".into()],
        command_allowlist: vec![vec!["cargo".into(), "test".into()]],
        iteration_cap: Some(8),
    };
    assert!(looped.validate().is_empty());
    let res = apply_change(&looped, |c| {
        c.file_allowlist.clear();
        c.command_allowlist.clear();
        c.iteration_cap = None;
    });
    assert!(
        res.is_err(),
        "must refuse stripping the gates out from under agent-loop"
    );
}

#[test]
fn apply_change_policy_is_independent_of_mode() {
    let r = apply_change(&AgentConfig::default(), |c| {
        c.approval_policy = ApprovalPolicy::AskOnFail
    })
    .expect("policy change is always valid");
    assert_eq!(r.approval_policy, ApprovalPolicy::AskOnFail);
    assert_eq!(r.mode, AgentMode::Chat, "mode unchanged");
}
