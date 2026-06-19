//! Bounded agent-loop controller (D79) — agent-loop slice 3.
//!
//! The read/edit/test/fix loop's driver: it runs an abstract step up to
//! the iteration budget (`iterationCap` from the session config) and
//! stops on the first terminal condition — the step reports the task
//! done, the step needs the user (a `Prompt` from the approval gate or a
//! question), the step fails (fail-closed), the user aborts (the one-key
//! abort from `docs/SAFETY.md § "agent-loop always requires"`), or the
//! budget is exhausted.
//!
//! It is **pure control flow**: no model, no tools, no IPC. The step is a
//! closure the caller supplies, and the abort signal is a predicate, so
//! the whole controller is unit-tested with fakes. The real step (drive
//! the model, classify the tool request through `agent::approval`,
//! execute a read/patch/command, observe the result) and the IPC/UI that
//! surface the loop are slice 4 — hence `allow(dead_code)` until then.

#![allow(dead_code)]

use serde::Serialize;

/// What one iteration's step did, and what the controller should do next.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepOutcome {
    /// Ran an action; there is more work — keep iterating.
    Continue,
    /// The task is complete; stop with [`LoopOutcome::Done`].
    Done,
    /// The step needs the user before it can proceed (an approval prompt,
    /// a clarifying question). The loop yields; resuming is the caller's
    /// concern.
    Paused { reason: String },
    /// The step failed; stop. The loop fails closed — it does not retry
    /// on its own.
    Failed { reason: String },
}

/// Why the loop ended. Serializes as a tagged union (`{ "kind": "done" }`,
/// `{ "kind": "paused", "reason": "…" }`) for a future `agent.*` event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum LoopOutcome {
    /// The step reported the task complete.
    Done,
    /// The iteration budget was reached with work still ongoing.
    BudgetExhausted,
    /// The user aborted before or between iterations.
    Aborted,
    /// The step yielded for the user.
    Paused { reason: String },
    /// A step failed; the loop stopped fail-closed.
    Failed { reason: String },
}

/// The result of a controller run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoopReport {
    pub outcome: LoopOutcome,
    /// How many iterations actually ran (the step was invoked this many
    /// times). `0` when aborted before the first iteration or the budget
    /// was zero.
    pub iterations_run: u32,
}

/// Run the bounded loop.
///
/// * `max_iterations` — the budget (the session `iterationCap`). `0`
///   runs nothing and reports `BudgetExhausted`.
/// * `is_aborted` — checked *before* each iteration so a one-key abort
///   stops the loop promptly without interrupting a step mid-flight.
/// * `step` — runs one iteration; receives the 1-based iteration number.
///
/// Terminal precedence per iteration: abort is checked first, then the
/// step's outcome decides. After `max_iterations` `Continue`s the loop
/// reports `BudgetExhausted`.
pub fn run_loop(
    max_iterations: u32,
    is_aborted: impl Fn() -> bool,
    mut step: impl FnMut(u32) -> StepOutcome,
) -> LoopReport {
    let mut iterations_run = 0u32;
    while iterations_run < max_iterations {
        // Abort is checked between iterations: a step already in flight
        // runs to completion, but no new one starts once aborted.
        if is_aborted() {
            return LoopReport {
                outcome: LoopOutcome::Aborted,
                iterations_run,
            };
        }
        iterations_run += 1;
        match step(iterations_run) {
            StepOutcome::Continue => {}
            StepOutcome::Done => {
                return LoopReport {
                    outcome: LoopOutcome::Done,
                    iterations_run,
                };
            }
            StepOutcome::Paused { reason } => {
                return LoopReport {
                    outcome: LoopOutcome::Paused { reason },
                    iterations_run,
                };
            }
            StepOutcome::Failed { reason } => {
                return LoopReport {
                    outcome: LoopOutcome::Failed { reason },
                    iterations_run,
                };
            }
        }
    }
    LoopReport {
        outcome: LoopOutcome::BudgetExhausted,
        iterations_run,
    }
}

#[cfg(test)]
#[path = "controller_tests.rs"]
mod tests;
