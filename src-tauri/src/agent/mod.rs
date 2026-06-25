//! Agent autonomy configuration (D77).
//!
//! Two independent axes plus explicit allowlists, per
//! `docs/SAFETY.md § "Agent autonomy is two independent axes"`:
//!
//! - `agentMode` — what the model may do: `chat` < `propose-diff` <
//!   `scoped-edit` < `agent-loop`.
//! - `approvalPolicy` — when the user is asked: `ask-each` /
//!   `ask-on-write` / `ask-on-fail`.
//! - `fileAllowlist` / `commandAllowlist` / `iterationCap` — the
//!   explicit gates the higher modes require.
//!
//! This module is the session-config substrate the IPC roadmap reserves
//! under `session.setMode` / `session.setApprovalPolicy` /
//! `session.setAllowlist` / `session.state`. Until it landed, the
//! session was hardcoded to `ask-each` with empty allowlists.
//!
//! It is **pure state + validation** — no tool execution, no model, no
//! loop controller. Those are later slices. The one invariant enforced
//! here is the fail-closed rule from `docs/SAFETY.md § "agent-loop
//! always requires"`: a config in `agent-loop` mode is only valid with a
//! non-empty `fileAllowlist`, a non-empty `commandAllowlist`, and an
//! `iterationCap`. Every setter validates the *resulting* config and
//! refuses to commit an invalid one, so the session can never be left
//! half-configured into autonomy.

use serde::{Deserialize, Serialize};

pub mod approval;
pub mod catalog;
pub mod controller;
pub mod dry_run;
pub mod events;
pub mod ledger;

/// Hard ceiling on the user-requested agent-loop iteration cap. A
/// request above this is rejected (not silently clamped) so the caller's
/// intent stays honest, matching how `memory.search` treats its limit.
pub const MAX_ITERATION_CAP: u32 = 100;

/// Bound on the number of entries in either allowlist. Keeps validation
/// and later per-write membership checks cheap, and a list longer than
/// this is almost certainly a mistake (e.g. the whole repo pasted in).
pub const MAX_ALLOWLIST_ENTRIES: usize = 64;

/// Bound on a single allowlist path entry or command argument, in bytes.
pub const MAX_ALLOWLIST_ENTRY_BYTES: usize = 512;

/// What the model is allowed to do. Independent of [`ApprovalPolicy`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AgentMode {
    Chat,
    ProposeDiff,
    ScopedEdit,
    AgentLoop,
}

/// When the user is asked. Independent of [`AgentMode`].
//
// The `Ask*` prefix is intentional — these are the exact policy names
// from `docs/SAFETY.md` (`ask-each` / `ask-on-write` / `ask-on-fail`),
// so the shared prefix is the spec, not an accident.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[allow(clippy::enum_variant_names)]
pub enum ApprovalPolicy {
    AskEach,
    AskOnWrite,
    AskOnFail,
}

/// The session's agent-autonomy configuration. Window-scoped state
/// (held in `AppState`), reset to [`AgentConfig::default`] on every
/// project open so one project's allowlists never carry into another.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentConfig {
    pub mode: AgentMode,
    pub approval_policy: ApprovalPolicy,
    /// Project-relative path entries the agent may write under in
    /// `scoped-edit` / `agent-loop`. Empty means "no writes".
    pub file_allowlist: Vec<String>,
    /// Approved argv vectors the agent may run. Empty means "no
    /// commands". Each entry is a full argv (`["cargo", "test"]`).
    pub command_allowlist: Vec<Vec<String>>,
    /// Maximum agent-loop iterations. `None` until set; required before
    /// `agent-loop` mode is valid.
    pub iteration_cap: Option<u32>,
}

impl Default for AgentConfig {
    fn default() -> Self {
        // Matches the documented v1 lock: `ask-each` with empty
        // allowlists, the least-privilege starting point.
        Self {
            mode: AgentMode::Chat,
            approval_policy: ApprovalPolicy::AskEach,
            file_allowlist: Vec::new(),
            command_allowlist: Vec::new(),
            iteration_cap: None,
        }
    }
}

impl AgentConfig {
    /// Validate the whole config. Returns the list of reasons it is
    /// invalid (empty list = valid). Collecting all reasons (rather than
    /// failing on the first) lets the UI show everything the user must
    /// fix to enter `agent-loop` in one pass.
    pub fn validate(&self) -> Vec<String> {
        let mut reasons = Vec::new();

        if let Some(cap) = self.iteration_cap {
            if cap == 0 || cap > MAX_ITERATION_CAP {
                reasons.push(format!(
                    "iterationCap must be between 1 and {MAX_ITERATION_CAP}; got {cap}"
                ));
            }
        }

        if self.file_allowlist.len() > MAX_ALLOWLIST_ENTRIES {
            reasons.push(format!(
                "fileAllowlist has {} entries; max is {MAX_ALLOWLIST_ENTRIES}",
                self.file_allowlist.len()
            ));
        }
        for entry in &self.file_allowlist {
            if let Err(why) = validate_allowlist_path(entry) {
                reasons.push(format!("fileAllowlist entry {entry:?}: {why}"));
            }
        }

        if self.command_allowlist.len() > MAX_ALLOWLIST_ENTRIES {
            reasons.push(format!(
                "commandAllowlist has {} entries; max is {MAX_ALLOWLIST_ENTRIES}",
                self.command_allowlist.len()
            ));
        }
        for argv in &self.command_allowlist {
            if let Err(why) = validate_allowlist_argv(argv) {
                reasons.push(format!("commandAllowlist entry {argv:?}: {why}"));
            }
        }

        // The fail-closed rule: agent-loop is only valid fully gated.
        if self.mode == AgentMode::AgentLoop {
            if self.file_allowlist.is_empty() {
                reasons.push("agent-loop requires a non-empty fileAllowlist".to_string());
            }
            if self.command_allowlist.is_empty() {
                reasons.push("agent-loop requires a non-empty commandAllowlist".to_string());
            }
            if self.iteration_cap.is_none() {
                reasons.push("agent-loop requires an iterationCap".to_string());
            }
        }

        reasons
    }
}

/// Validate a `fileAllowlist` entry: non-empty, bounded, project-relative
/// (no absolute path, no `..` escape), no NUL. These entries gate writes
/// in a later slice, so reject anything path-shaped that could escape the
/// project root now rather than at write time.
fn validate_allowlist_path(entry: &str) -> Result<(), String> {
    let trimmed = entry.trim();
    if trimmed.is_empty() {
        return Err("empty".to_string());
    }
    if entry.len() > MAX_ALLOWLIST_ENTRY_BYTES {
        return Err(format!(
            "{} bytes; max is {MAX_ALLOWLIST_ENTRY_BYTES}",
            entry.len()
        ));
    }
    if entry.contains('\0') {
        return Err("contains NUL".to_string());
    }
    if entry.starts_with('/') || entry.starts_with('\\') {
        return Err("must be project-relative (no leading slash)".to_string());
    }
    // Windows drive-letter absolute, e.g. `C:\...`.
    let bytes = entry.as_bytes();
    if bytes.len() >= 2 && bytes[1] == b':' && bytes[0].is_ascii_alphabetic() {
        return Err("must be project-relative (no drive letter)".to_string());
    }
    // Any `..` path component is an escape attempt.
    if entry.split(['/', '\\']).any(|component| component == "..") {
        return Err("must not contain a `..` component".to_string());
    }
    Ok(())
}

/// Validate a `commandAllowlist` argv: a normalizable command identity
/// (non-empty, non-blank program, **not an env-mutating wrapper**), with
/// every argument NUL-free and size-bounded.
///
/// The env-wrapper rejection reuses `approval::normalize_command` so an
/// allowlist can never hold a command the approval / ledger layer would
/// refuse. Without it the settings UI could commit `env A=1 npm test`
/// (parsed to `["env", "A=1", "npm", "test"]`) as a "valid" allowlist
/// entry the gate would then never honor — see `docs/SAFETY.md § argv
/// normalization`. Approve the wrapped command on its own identity instead.
fn validate_allowlist_argv(argv: &[String]) -> Result<(), String> {
    if let Err(why) = approval::normalize_command(argv) {
        return Err(match why {
            approval::NormalizeError::Empty => "empty argv".to_string(),
            approval::NormalizeError::BlankProgram => "program token is empty".to_string(),
            approval::NormalizeError::EnvWrapper => {
                "env-mutating wrapper (`env …`, or a leading KEY=VAL token) is not an \
                 approvable command; allowlist the wrapped command on its own identity"
                    .to_string()
            }
        });
    }
    for arg in argv {
        if arg.len() > MAX_ALLOWLIST_ENTRY_BYTES {
            return Err(format!(
                "argument exceeds {MAX_ALLOWLIST_ENTRY_BYTES} bytes"
            ));
        }
        if arg.contains('\0') {
            return Err("argument contains NUL".to_string());
        }
    }
    Ok(())
}

// ─── Wire shapes (in-band structured outcomes, like memory/patch) ───────

/// `Ok` arm: the (now-current) config after a successful setter, or the
/// read result of `session.state`.
#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentConfigOk {
    pub ok: bool,
    pub state: AgentConfig,
}

/// `Err` arm: the setter was refused; `reasons` lists every invariant
/// the requested config would have broken (the store is unchanged).
#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentConfigErr {
    pub ok: bool,
    pub reasons: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum AgentConfigResponse {
    Ok(AgentConfigOk),
    Err(AgentConfigErr),
}

impl AgentConfigResponse {
    pub fn ok(state: AgentConfig) -> Self {
        AgentConfigResponse::Ok(AgentConfigOk { ok: true, state })
    }
    pub fn err(reasons: Vec<String>) -> Self {
        AgentConfigResponse::Err(AgentConfigErr { ok: false, reasons })
    }
}

#[cfg(test)]
#[path = "agent_tests.rs"]
mod tests;
