//! Tests for the bounded loop controller (D79).

use super::*;
use std::cell::Cell;

/// Never-abort predicate.
fn never() -> bool {
    false
}

#[test]
fn budget_exhausted_when_step_always_continues() {
    let calls = Cell::new(0u32);
    let report = run_loop(3, never, |i| {
        calls.set(i);
        StepOutcome::Continue
    });
    assert_eq!(report.outcome, LoopOutcome::BudgetExhausted);
    assert_eq!(report.iterations_run, 3);
    assert_eq!(calls.get(), 3, "step ran exactly the budget");
}

#[test]
fn step_receives_one_based_iteration_numbers() {
    let seen = std::cell::RefCell::new(Vec::new());
    let report = run_loop(4, never, |i| {
        seen.borrow_mut().push(i);
        if i == 3 {
            StepOutcome::Done
        } else {
            StepOutcome::Continue
        }
    });
    assert_eq!(report.outcome, LoopOutcome::Done);
    assert_eq!(report.iterations_run, 3);
    assert_eq!(*seen.borrow(), vec![1, 2, 3], "1-based, stops at Done");
}

#[test]
fn done_stops_immediately() {
    let report = run_loop(10, never, |_| StepOutcome::Done);
    assert_eq!(report.outcome, LoopOutcome::Done);
    assert_eq!(report.iterations_run, 1);
}

#[test]
fn failed_stops_fail_closed() {
    let report = run_loop(10, never, |_| StepOutcome::Failed {
        reason: "verifier blew up".into(),
    });
    assert_eq!(
        report.outcome,
        LoopOutcome::Failed {
            reason: "verifier blew up".into()
        }
    );
    assert_eq!(report.iterations_run, 1);
}

#[test]
fn paused_yields_to_user() {
    let report = run_loop(10, never, |_| StepOutcome::Paused {
        reason: "approve `cargo test`".into(),
    });
    assert_eq!(
        report.outcome,
        LoopOutcome::Paused {
            reason: "approve `cargo test`".into()
        }
    );
    assert_eq!(report.iterations_run, 1);
}

#[test]
fn abort_before_first_iteration_runs_nothing() {
    let calls = Cell::new(0u32);
    let report = run_loop(
        5,
        || true, // aborted from the start
        |_| {
            calls.set(calls.get() + 1);
            StepOutcome::Continue
        },
    );
    assert_eq!(report.outcome, LoopOutcome::Aborted);
    assert_eq!(report.iterations_run, 0);
    assert_eq!(calls.get(), 0, "step never ran");
}

#[test]
fn abort_between_iterations_stops_after_completed_steps() {
    // Abort becomes true after the first iteration completes.
    let ran = Cell::new(0u32);
    let report = run_loop(
        5,
        || ran.get() >= 1,
        |_| {
            ran.set(ran.get() + 1);
            StepOutcome::Continue
        },
    );
    assert_eq!(report.outcome, LoopOutcome::Aborted);
    assert_eq!(report.iterations_run, 1, "one step completed before abort");
}

#[test]
fn zero_budget_runs_nothing() {
    let calls = Cell::new(0u32);
    let report = run_loop(0, never, |_| {
        calls.set(calls.get() + 1);
        StepOutcome::Continue
    });
    assert_eq!(report.outcome, LoopOutcome::BudgetExhausted);
    assert_eq!(report.iterations_run, 0);
    assert_eq!(calls.get(), 0);
}

#[test]
fn outcome_serializes_as_tagged_union() {
    assert_eq!(
        serde_json::to_value(LoopOutcome::Done).unwrap(),
        serde_json::json!({ "kind": "done" })
    );
    assert_eq!(
        serde_json::to_value(LoopOutcome::BudgetExhausted).unwrap(),
        serde_json::json!({ "kind": "budgetExhausted" })
    );
    assert_eq!(
        serde_json::to_value(LoopOutcome::Paused {
            reason: "why".into()
        })
        .unwrap(),
        serde_json::json!({ "kind": "paused", "reason": "why" })
    );
}

#[test]
fn report_serializes_camel_case() {
    let report = LoopReport {
        outcome: LoopOutcome::Done,
        iterations_run: 2,
    };
    let json = serde_json::to_value(&report).unwrap();
    assert_eq!(json["iterationsRun"], serde_json::json!(2));
    assert_eq!(json["outcome"], serde_json::json!({ "kind": "done" }));
}
