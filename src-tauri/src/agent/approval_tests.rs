//! Tests for the approval decision core (D78).

use super::*;
use ApprovalDecision::{Allow, Prompt};
use ApprovalPolicy::{AskEach, AskOnFail, AskOnWrite};

fn cmd(parts: &[&str]) -> NormalizedCommand {
    normalize_command(&parts.iter().map(|s| s.to_string()).collect::<Vec<_>>()).expect("normalizes")
}

fn ledger_with(cmds: &[NormalizedCommand]) -> ApprovalLedger {
    let mut l = ApprovalLedger::new();
    for c in cmds {
        l.approve(c.clone());
    }
    l
}

// ─── normalize_command ──────────────────────────────────────────────────

#[test]
fn normalize_rejects_empty_and_blank() {
    assert_eq!(normalize_command(&[]), Err(NormalizeError::Empty));
    assert_eq!(
        normalize_command(&["   ".to_string()]),
        Err(NormalizeError::BlankProgram)
    );
}

#[test]
fn normalize_rejects_env_wrappers() {
    assert_eq!(
        normalize_command(&["env".into(), "A=1".into(), "npm".into()]),
        Err(NormalizeError::EnvWrapper)
    );
    assert_eq!(
        normalize_command(&["/usr/bin/env".into(), "npm".into()]),
        Err(NormalizeError::EnvWrapper)
    );
    assert_eq!(
        normalize_command(&["FOO=1".into(), "npm".into()]),
        Err(NormalizeError::EnvWrapper)
    );
}

#[test]
fn normalize_keeps_trailing_args_verbatim() {
    let a = cmd(&["npm", "test"]);
    let b = cmd(&["npm", "test", "--watch"]);
    assert_ne!(
        a, b,
        "`npm test` and `npm test --watch` are distinct approvals"
    );
    assert_eq!(a.argv, vec!["npm", "test"]);
}

// ─── ledger ─────────────────────────────────────────────────────────────

#[test]
fn ledger_approve_contains_revoke() {
    let mut l = ApprovalLedger::new();
    let c = cmd(&["cargo", "test"]);
    assert!(!l.contains(&c));
    l.approve(c.clone());
    assert!(l.contains(&c));
    assert_eq!(l.len(), 1);
    l.approve(c.clone()); // idempotent
    assert_eq!(l.len(), 1);
    assert!(l.revoke(&c));
    assert!(!l.contains(&c));
    assert!(!l.revoke(&c));
}

// ─── ask-each: everything prompts ───────────────────────────────────────

#[test]
fn ask_each_prompts_everything() {
    let c = cmd(&["npm", "test"]);
    let ledger = ledger_with(std::slice::from_ref(&c)); // even approved
    assert_eq!(
        decide(AskEach, &ToolRequest::ReadOnly, &ledger, None),
        Prompt
    );
    assert_eq!(
        decide(
            AskEach,
            &ToolRequest::Write {
                path: "src/a.rs".into()
            },
            &ledger,
            None
        ),
        Prompt
    );
    assert_eq!(
        decide(AskEach, &ToolRequest::Command(c), &ledger, None),
        Prompt
    );
}

// ─── ask-on-write ───────────────────────────────────────────────────────

#[test]
fn ask_on_write_readonly_is_silent() {
    let ledger = ApprovalLedger::new();
    assert_eq!(
        decide(AskOnWrite, &ToolRequest::ReadOnly, &ledger, None),
        Allow
    );
}

#[test]
fn ask_on_write_writes_always_prompt() {
    let ledger = ApprovalLedger::new();
    assert_eq!(
        decide(
            AskOnWrite,
            &ToolRequest::Write {
                path: "src/a.rs".into()
            },
            &ledger,
            None
        ),
        Prompt
    );
}

#[test]
fn ask_on_write_unapproved_command_prompts() {
    let ledger = ApprovalLedger::new();
    let c = cmd(&["npm", "test"]);
    assert_eq!(
        decide(AskOnWrite, &ToolRequest::Command(c), &ledger, None),
        Prompt
    );
}

#[test]
fn ask_on_write_approved_command_first_run_allows_then_repeat_prompts() {
    let c = cmd(&["npm", "test"]);
    let ledger = ledger_with(std::slice::from_ref(&c));
    // First run this session.
    assert_eq!(
        decide(AskOnWrite, &ToolRequest::Command(c.clone()), &ledger, None),
        Allow
    );
    // Repeat — re-approve every loop iteration (the doc's example).
    let repeat = CommandRunState {
        ran_before: true,
        previous_exit_nonzero: false,
    };
    assert_eq!(
        decide(AskOnWrite, &ToolRequest::Command(c), &ledger, Some(&repeat)),
        Prompt
    );
}

// ─── ask-on-fail ────────────────────────────────────────────────────────

#[test]
fn ask_on_fail_matches_on_write_for_readonly_writes_and_first_run() {
    let c = cmd(&["npm", "test"]);
    let ledger = ledger_with(std::slice::from_ref(&c));
    assert_eq!(
        decide(AskOnFail, &ToolRequest::ReadOnly, &ledger, None),
        Allow
    );
    assert_eq!(
        decide(
            AskOnFail,
            &ToolRequest::Write { path: "x".into() },
            &ledger,
            None
        ),
        Prompt
    );
    assert_eq!(
        decide(AskOnFail, &ToolRequest::Command(c), &ledger, None),
        Allow
    );
}

#[test]
fn ask_on_fail_allows_verifier_retry_after_failure() {
    let c = cmd(&["npm", "test"]);
    let ledger = ledger_with(std::slice::from_ref(&c));
    // Repeat after a non-zero exit: the verifier-retry case ask-on-fail
    // exists for.
    let failed_retry = CommandRunState {
        ran_before: true,
        previous_exit_nonzero: true,
    };
    assert_eq!(
        decide(
            AskOnFail,
            &ToolRequest::Command(c),
            &ledger,
            Some(&failed_retry)
        ),
        Allow
    );
}

#[test]
fn ask_on_fail_reprompts_gratuitous_rerun_after_success() {
    let c = cmd(&["npm", "test"]);
    let ledger = ledger_with(std::slice::from_ref(&c));
    // Repeat after a SUCCESSFUL run is not a verifier retry → prompt.
    let success_repeat = CommandRunState {
        ran_before: true,
        previous_exit_nonzero: false,
    };
    assert_eq!(
        decide(
            AskOnFail,
            &ToolRequest::Command(c),
            &ledger,
            Some(&success_repeat)
        ),
        Prompt
    );
}

#[test]
fn ask_on_fail_never_grants_first_run_to_unapproved_command_even_on_retry() {
    // Critical safety guarantee: a command NOT in the ledger never
    // auto-runs, even if a (different, failed) run is in flight.
    let unapproved = cmd(&["rm", "-rf", "build"]);
    let ledger = ApprovalLedger::new();
    let retry = CommandRunState {
        ran_before: true,
        previous_exit_nonzero: true,
    };
    assert_eq!(
        decide(
            AskOnFail,
            &ToolRequest::Command(unapproved),
            &ledger,
            Some(&retry)
        ),
        Prompt
    );
}

#[test]
fn decision_serializes_camel_case() {
    assert_eq!(
        serde_json::to_value(Allow).unwrap(),
        serde_json::json!("allow")
    );
    assert_eq!(
        serde_json::to_value(Prompt).unwrap(),
        serde_json::json!("prompt")
    );
}
