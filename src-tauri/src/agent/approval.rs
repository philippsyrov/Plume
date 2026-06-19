//! Approval decision core (D78) — agent-loop slice 2.
//!
//! The pure logic that answers one question: given the session's
//! `approvalPolicy`, what the agent wants to do next, and the set of
//! commands the user has already approved, should this action run
//! silently or stop and prompt? See `docs/SAFETY.md § approvalPolicy`
//! and `§ Approval ledger`.
//!
//! This slice is the **decision core only**. The persistent ledger
//! (`<project>/.plume/approvals.toml`), PATH resolution of the program
//! token to a basename + absolute binary path, the binary-mismatch
//! re-prompt, and expiry are a follow-up slice; here the ledger is an
//! in-memory set keyed by verbatim normalized argv, which is what the
//! decision needs. The decision function and the verb that records
//! approvals are wired by the loop controller (slice 3); until then
//! these items have no non-test caller, hence the module-level
//! `allow(dead_code)`.
#![allow(dead_code)]

use std::collections::HashSet;

use serde::Serialize;

use super::ApprovalPolicy;

/// A command's normalized argv, used as the ledger match key.
///
/// D78 keeps the argv verbatim (program token + trailing args). The
/// `docs/SAFETY.md § Argv normalization` rule of resolving the program
/// to a basename + absolute path and requiring *both* to match is
/// deferred to the persistence slice — that step needs `PATH` + the
/// filesystem and so isn't part of this pure core. Trailing args are
/// already kept verbatim here, matching the spec (`npm test` and
/// `npm test --watch` are different approvals).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct NormalizedCommand {
    pub argv: Vec<String>,
}

/// Why an argv could not be normalized into an approvable command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NormalizeError {
    /// The argv was empty.
    Empty,
    /// The program token (argv[0]) was blank.
    BlankProgram,
    /// An environment-mutating wrapper such as `env A=1 npm test`, or a
    /// leading `KEY=VAL` token. `docs/SAFETY.md` rejects these — approve
    /// the wrapped command on its own identity instead.
    EnvWrapper,
}

/// Normalize an argv into a [`NormalizedCommand`] (the ledger key), or
/// reject it. Pure: no `PATH` lookup, no filesystem.
pub fn normalize_command(argv: &[String]) -> Result<NormalizedCommand, NormalizeError> {
    let Some(program) = argv.first() else {
        return Err(NormalizeError::Empty);
    };
    if program.trim().is_empty() {
        return Err(NormalizeError::BlankProgram);
    }
    // Reject env-mutating wrappers: the `env` program itself, or a
    // program token shaped like a `KEY=VAL` assignment.
    let basename = program.rsplit(['/', '\\']).next().unwrap_or(program);
    if basename == "env" {
        return Err(NormalizeError::EnvWrapper);
    }
    if program.contains('=') {
        return Err(NormalizeError::EnvWrapper);
    }
    Ok(NormalizedCommand {
        argv: argv.to_vec(),
    })
}

/// In-memory set of commands the user has approved this session. The
/// decision function only needs membership; the persistent, expiring,
/// binary-matched ledger lands in a later slice.
#[derive(Debug, Default)]
pub struct ApprovalLedger {
    approved: HashSet<NormalizedCommand>,
}

impl ApprovalLedger {
    pub fn new() -> Self {
        Self::default()
    }
    /// Record an approval. Idempotent.
    pub fn approve(&mut self, cmd: NormalizedCommand) {
        self.approved.insert(cmd);
    }
    pub fn contains(&self, cmd: &NormalizedCommand) -> bool {
        self.approved.contains(cmd)
    }
    /// Remove an approval; returns whether it was present.
    pub fn revoke(&mut self, cmd: &NormalizedCommand) -> bool {
        self.approved.remove(cmd)
    }
    pub fn len(&self) -> usize {
        self.approved.len()
    }
    pub fn is_empty(&self) -> bool {
        self.approved.is_empty()
    }
}

/// What the agent wants to do next, classified for the approval gate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolRequest {
    /// A read-only tool (fs read, grep, git status) — never mutates.
    ReadOnly,
    /// A write to a project-relative path. Whether the write is
    /// permitted *at all* is the `fileAllowlist` gate (checked
    /// elsewhere); this decision is only "prompt or not".
    Write { path: String },
    /// A shell command identified by its normalized argv.
    Command(NormalizedCommand),
}

/// Per-session run state for a command, owned by the loop controller
/// (slice 3). Lets the policies distinguish a first run from a repeat,
/// and a verifier retry (the repeat of a just-*failed* command) from a
/// gratuitous re-run. Defaults to "first run, nothing failed".
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CommandRunState {
    /// This exact argv has already executed at least once this session.
    pub ran_before: bool,
    /// The previous run of this argv exited non-zero (verifier failed).
    pub previous_exit_nonzero: bool,
}

/// The gate's answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ApprovalDecision {
    /// Run without prompting.
    Allow,
    /// Stop and ask the user.
    Prompt,
}

/// Decide whether `request` may run without prompting under `policy`.
///
/// `run_state` is only consulted for [`ToolRequest::Command`]; pass
/// `None` to mean "first run this session, nothing failed".
///
/// Faithful to `docs/SAFETY.md § approvalPolicy`, and conservative —
/// any case not explicitly allowed is a `Prompt`:
///
/// * **`ask-each`** — everything prompts; no tool ever auto-runs.
/// * **`ask-on-write`** — read-only tools run silently; **writes always
///   prompt**; a command runs silently only on its *first* run this
///   session and only if its argv is in the ledger. A repeat of an
///   approved command re-prompts (the doc's "re-approve on every loop
///   iteration" case).
/// * **`ask-on-fail`** — same as `ask-on-write`, plus the one relaxation
///   it exists for: a repeat of a ledger-approved command whose previous
///   run **exited non-zero** (a verifier retry) runs silently.
///
/// Hard guarantees, pinned by tests:
/// * No policy ever grants first-run permission to a command whose argv
///   is not in the ledger — not even `ask-on-fail` on a retry.
/// * `ask-each` never returns `Allow`.
pub fn decide(
    policy: ApprovalPolicy,
    request: &ToolRequest,
    ledger: &ApprovalLedger,
    run_state: Option<&CommandRunState>,
) -> ApprovalDecision {
    use ApprovalDecision::{Allow, Prompt};

    if policy == ApprovalPolicy::AskEach {
        return Prompt;
    }

    match request {
        ToolRequest::ReadOnly => Allow,
        // Writes always prompt under a policy that isn't fully manual;
        // the fileAllowlist decides whether the write is permitted at
        // all, this only decides the prompt.
        ToolRequest::Write { .. } => Prompt,
        ToolRequest::Command(cmd) => {
            // Never grant first-run permission to an un-approved argv.
            if !ledger.contains(cmd) {
                return Prompt;
            }
            let rs = run_state.copied().unwrap_or_default();
            if !rs.ran_before {
                // First approved run this session: silent under both
                // ask-on-write and ask-on-fail.
                Allow
            } else {
                // A repeat. ask-on-write re-prompts; ask-on-fail allows
                // only the verifier-retry of a just-failed run.
                match policy {
                    ApprovalPolicy::AskOnFail if rs.previous_exit_nonzero => Allow,
                    _ => Prompt,
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "approval_tests.rs"]
mod tests;
