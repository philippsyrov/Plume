//! D93: agent event dry-run IPC (`agent.dryRun`).
//!
//! Returns the deterministic, dev-only event stream from
//! [`crate::agent::dry_run::scripted_dry_run`] so the frontend can prove
//! the typed D85 event protocol drives the `AgentEventLog` surface end to
//! end. **Nothing real runs**: no model, no shell, no patch, no file
//! writes. Like `tools.*` / `session.*` it is an unprivileged pure read
//! (not trust-gated) — it just hands back a fixed sequence of typed
//! events.

use crate::agent::dry_run::scripted_dry_run;
use crate::agent::events::AgentEventEnvelope;
use crate::error::{IpcError, IpcRequest};

use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EmptyPayload {}

#[derive(Debug, serde::Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentDryRunResponse {
    pub events: Vec<AgentEventEnvelope>,
}

#[tauri::command]
pub async fn agent_dry_run(req: IpcRequest<EmptyPayload>) -> Result<AgentDryRunResponse, IpcError> {
    req.check_version()?;
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    Ok(AgentDryRunResponse {
        events: scripted_dry_run(now_ms),
    })
}

#[cfg(test)]
#[path = "agent_command_tests.rs"]
mod tests;
