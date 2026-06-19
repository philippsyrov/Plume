//! Tests for `agent` config validation + serialization (D77).

use super::*;

#[test]
fn default_is_least_privilege_and_valid() {
    let c = AgentConfig::default();
    assert_eq!(c.mode, AgentMode::Chat);
    assert_eq!(c.approval_policy, ApprovalPolicy::AskEach);
    assert!(c.file_allowlist.is_empty());
    assert!(c.command_allowlist.is_empty());
    assert!(c.iteration_cap.is_none());
    assert!(c.validate().is_empty(), "default must be valid");
}

#[test]
fn chat_proposediff_scopededit_valid_without_allowlists() {
    for mode in [
        AgentMode::Chat,
        AgentMode::ProposeDiff,
        AgentMode::ScopedEdit,
    ] {
        let c = AgentConfig {
            mode,
            ..Default::default()
        };
        assert!(
            c.validate().is_empty(),
            "{mode:?} should be valid with empty allowlists"
        );
    }
}

#[test]
fn agent_loop_requires_allowlists_and_cap() {
    let c = AgentConfig {
        mode: AgentMode::AgentLoop,
        ..Default::default()
    };
    let reasons = c.validate();
    assert_eq!(reasons.len(), 3, "got {reasons:?}");
    assert!(reasons.iter().any(|r| r.contains("fileAllowlist")));
    assert!(reasons.iter().any(|r| r.contains("commandAllowlist")));
    assert!(reasons.iter().any(|r| r.contains("iterationCap")));
}

#[test]
fn agent_loop_valid_when_fully_gated() {
    let c = AgentConfig {
        mode: AgentMode::AgentLoop,
        approval_policy: ApprovalPolicy::AskOnFail,
        file_allowlist: vec!["src/".into()],
        command_allowlist: vec![vec!["cargo".into(), "test".into()]],
        iteration_cap: Some(10),
    };
    assert!(c.validate().is_empty(), "{:?}", c.validate());
}

#[test]
fn iteration_cap_bounds() {
    let mk = |cap| AgentConfig {
        iteration_cap: Some(cap),
        ..Default::default()
    };
    assert!(!mk(0).validate().is_empty(), "0 rejected");
    assert!(mk(1).validate().is_empty(), "1 ok");
    assert!(mk(MAX_ITERATION_CAP).validate().is_empty(), "max ok");
    assert!(
        !mk(MAX_ITERATION_CAP + 1).validate().is_empty(),
        "over max rejected"
    );
}

#[test]
fn file_allowlist_rejects_escapes() {
    for bad in [
        "/etc/passwd",
        "../secret",
        "a/../b",
        "C:\\windows",
        "",
        "\\abs",
        "a/b/../../c",
    ] {
        let c = AgentConfig {
            file_allowlist: vec![bad.into()],
            ..Default::default()
        };
        assert!(!c.validate().is_empty(), "should reject {bad:?}");
    }
}

#[test]
fn file_allowlist_accepts_relative() {
    let c = AgentConfig {
        file_allowlist: vec!["src/main.rs".into(), "tests/".into(), "a/b/c.txt".into()],
        ..Default::default()
    };
    assert!(c.validate().is_empty(), "{:?}", c.validate());
}

#[test]
fn allowlist_size_caps() {
    let many: Vec<String> = (0..=MAX_ALLOWLIST_ENTRIES)
        .map(|i| format!("f{i}"))
        .collect();
    let c = AgentConfig {
        file_allowlist: many,
        ..Default::default()
    };
    assert!(c.validate().iter().any(|r| r.contains("max is")));
}

#[test]
fn command_allowlist_rejects_empty_argv_and_blank_program() {
    let c = AgentConfig {
        command_allowlist: vec![vec![]],
        ..Default::default()
    };
    assert!(!c.validate().is_empty(), "empty argv rejected");
    let c2 = AgentConfig {
        command_allowlist: vec![vec!["   ".into()]],
        ..Default::default()
    };
    assert!(!c2.validate().is_empty(), "blank program rejected");
}

#[test]
fn command_allowlist_rejects_env_wrappers() {
    // The allowlist must refuse the same env-mutating wrappers the
    // approval / ledger layer refuses, so the settings UI can't commit a
    // command identity the gate would never honor.
    let env_wrapper = AgentConfig {
        command_allowlist: vec![vec![
            "env".into(),
            "A=1".into(),
            "npm".into(),
            "test".into(),
        ]],
        ..Default::default()
    };
    let reasons = env_wrapper.validate();
    assert!(
        reasons.iter().any(|r| r.contains("env-mutating wrapper")),
        "env wrapper rejected: {reasons:?}"
    );

    let leading_assignment = AgentConfig {
        command_allowlist: vec![vec!["FOO=1".into(), "npm".into()]],
        ..Default::default()
    };
    assert!(
        leading_assignment
            .validate()
            .iter()
            .any(|r| r.contains("env-mutating wrapper")),
        "leading KEY=VAL token rejected"
    );

    // `/usr/bin/env npm` (absolute path to the env binary) is rejected too.
    let abs_env = AgentConfig {
        command_allowlist: vec![vec!["/usr/bin/env".into(), "npm".into()]],
        ..Default::default()
    };
    assert!(!abs_env.validate().is_empty(), "absolute env rejected");
}

#[test]
fn command_allowlist_accepts_normal_argv() {
    let c = AgentConfig {
        command_allowlist: vec![
            vec!["cargo".into(), "test".into()],
            vec!["npm".into(), "run".into(), "build".into()],
        ],
        ..Default::default()
    };
    assert!(c.validate().is_empty(), "{:?}", c.validate());
}

#[test]
fn enums_serialize_kebab_case() {
    assert_eq!(
        serde_json::to_value(AgentMode::AgentLoop).unwrap(),
        serde_json::json!("agent-loop")
    );
    assert_eq!(
        serde_json::to_value(AgentMode::ProposeDiff).unwrap(),
        serde_json::json!("propose-diff")
    );
    assert_eq!(
        serde_json::to_value(ApprovalPolicy::AskOnFail).unwrap(),
        serde_json::json!("ask-on-fail")
    );
}

#[test]
fn enums_deserialize_kebab_case() {
    let m: AgentMode = serde_json::from_value(serde_json::json!("scoped-edit")).unwrap();
    assert_eq!(m, AgentMode::ScopedEdit);
    let p: ApprovalPolicy = serde_json::from_value(serde_json::json!("ask-on-write")).unwrap();
    assert_eq!(p, ApprovalPolicy::AskOnWrite);
}

#[test]
fn config_serializes_camel_case() {
    let json = serde_json::to_value(AgentConfig::default()).unwrap();
    assert_eq!(
        json,
        serde_json::json!({
            "mode": "chat",
            "approvalPolicy": "ask-each",
            "fileAllowlist": [],
            "commandAllowlist": [],
            "iterationCap": null,
        })
    );
}

#[test]
fn response_ok_and_err_serialize_with_discriminator() {
    let ok = AgentConfigResponse::ok(AgentConfig::default());
    let v = serde_json::to_value(&ok).unwrap();
    assert_eq!(v["ok"], serde_json::json!(true));
    assert!(v["state"].is_object());

    let err = AgentConfigResponse::err(vec!["nope".into()]);
    let v = serde_json::to_value(&err).unwrap();
    assert_eq!(v["ok"], serde_json::json!(false));
    assert_eq!(v["reasons"], serde_json::json!(["nope"]));
}
