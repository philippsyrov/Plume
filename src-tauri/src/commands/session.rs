//! D77: session agent-autonomy config handlers.
//!
//! The four `session.*` verbs the IPC roadmap reserves:
//!
//! - `session.setMode` — flip `agentMode`.
//! - `session.setApprovalPolicy` — flip `approvalPolicy`.
//! - `session.setAllowlist` — replace `fileAllowlist` /
//!   `commandAllowlist` / `iterationCap`.
//! - `session.state` — read the current config.
//!
//! These are window-scoped session state and touch no disk, so unlike
//! the memory / patch verbs they are **not** trust-gated — they only
//! declare intent. The actions the config gates (writes, commands, the
//! agent loop) are trust- and approval-gated when they actually run, in
//! later slices.
//!
//! Each setter does a locked read-modify-validate-write on the shared
//! `AgentConfig`: it builds a candidate, runs `AgentConfig::validate`,
//! and commits only if the candidate is valid. An invalid request leaves
//! the stored config untouched and returns the list of broken
//! invariants in-band (`AgentConfigResponse::Err`), so the session can
//! never be left half-configured into autonomy.

use serde::Deserialize;
use tauri::State;

use crate::agent::{AgentConfig, AgentConfigResponse, AgentMode, ApprovalPolicy};
use crate::commands::project::AppState;
use crate::error::{IpcError, IpcRequest};

/// Pure transition: clone `current`, apply `mutate`, validate the
/// candidate. Returns the new config on success or the broken-invariant
/// reasons on failure. The handlers wrap this with the lock + commit;
/// tests exercise it directly without a Tauri `AppState`.
fn apply_change(
    current: &AgentConfig,
    mutate: impl FnOnce(&mut AgentConfig),
) -> Result<AgentConfig, Vec<String>> {
    let mut candidate = current.clone();
    mutate(&mut candidate);
    let reasons = candidate.validate();
    if reasons.is_empty() {
        Ok(candidate)
    } else {
        Err(reasons)
    }
}

/// Hold the config lock for the whole read-modify-validate-write so two
/// concurrent setters can't race on a stale baseline.
fn with_config<F>(state: &AppState, mutate: F) -> AgentConfigResponse
where
    F: FnOnce(&mut AgentConfig),
{
    let mut guard = state.agent_config.lock().expect("agent config poisoned");
    match apply_change(&guard, mutate) {
        Ok(next) => {
            *guard = next.clone();
            AgentConfigResponse::ok(next)
        }
        Err(reasons) => AgentConfigResponse::err(reasons),
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SetModePayload {
    pub mode: AgentMode,
}

#[tauri::command]
pub async fn session_set_mode(
    req: IpcRequest<SetModePayload>,
    state: State<'_, AppState>,
) -> Result<AgentConfigResponse, IpcError> {
    req.check_version()?;
    let mode = req.payload.mode;
    Ok(with_config(&state, |c| c.mode = mode))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SetApprovalPolicyPayload {
    pub approval_policy: ApprovalPolicy,
}

#[tauri::command]
pub async fn session_set_approval_policy(
    req: IpcRequest<SetApprovalPolicyPayload>,
    state: State<'_, AppState>,
) -> Result<AgentConfigResponse, IpcError> {
    req.check_version()?;
    let policy = req.payload.approval_policy;
    Ok(with_config(&state, |c| c.approval_policy = policy))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SetAllowlistPayload {
    pub file_allowlist: Vec<String>,
    pub command_allowlist: Vec<Vec<String>>,
    /// `None` clears the iteration cap. Required (non-`None`) before
    /// `agent-loop` mode is valid.
    pub iteration_cap: Option<u32>,
}

#[tauri::command]
pub async fn session_set_allowlist(
    req: IpcRequest<SetAllowlistPayload>,
    state: State<'_, AppState>,
) -> Result<AgentConfigResponse, IpcError> {
    req.check_version()?;
    let SetAllowlistPayload {
        file_allowlist,
        command_allowlist,
        iteration_cap,
    } = req.payload;
    Ok(with_config(&state, move |c| {
        c.file_allowlist = file_allowlist;
        c.command_allowlist = command_allowlist;
        c.iteration_cap = iteration_cap;
    }))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EmptyPayload {}

#[tauri::command]
pub async fn session_state(
    req: IpcRequest<EmptyPayload>,
    state: State<'_, AppState>,
) -> Result<AgentConfig, IpcError> {
    req.check_version()?;
    let guard = state.agent_config.lock().expect("agent config poisoned");
    Ok(guard.clone())
}

#[cfg(test)]
#[path = "session_tests.rs"]
mod tests;
