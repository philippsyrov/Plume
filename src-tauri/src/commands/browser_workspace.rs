//! Session-owned Browser workspace IPC.
//!
//! These commands expose only bounded restoration descriptors. They never
//! accept filesystem roots and are callable only by Plume's main webview;
//! local/project separation is resolved through the existing session scope
//! boundary before the store is touched.

use serde::{Deserialize, Serialize};
use tauri::{State, Webview};

use crate::commands::project::AppState;
use crate::commands::sessions::{map_store_err, scope_dir, SessionScope};
use crate::error::{IpcError, IpcRequest};
use crate::sessions::browser_workspace::{
    load_browser_workspace, replace_browser_workspace, reset_browser_workspace,
    BrowserWorkspaceLoad, BrowserWorkspaceRecord, BrowserWorkspaceRecovery, BrowserWorkspaceScope,
};

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SessionIdentity {
    pub scope: SessionScope,
    pub session_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BrowserWorkspaceLoadPayload {
    pub identity: SessionIdentity,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BrowserWorkspaceSavePayload {
    pub identity: SessionIdentity,
    pub workspace: BrowserWorkspaceRecord,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BrowserWorkspaceResetPayload {
    pub identity: SessionIdentity,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserWorkspaceLoadResponse {
    pub workspace: Option<BrowserWorkspaceRecord>,
    pub recovery_notice: Option<BrowserWorkspaceRecovery>,
}

#[derive(Debug, Serialize)]
pub struct BrowserWorkspaceResponse {
    pub workspace: BrowserWorkspaceRecord,
}

#[tauri::command]
pub async fn browser_workspace_load(
    req: IpcRequest<BrowserWorkspaceLoadPayload>,
    caller: Webview,
    state: State<'_, AppState>,
) -> Result<BrowserWorkspaceLoadResponse, IpcError> {
    req.check_version()?;
    browser_workspace_load_impl(req.payload, &state, caller.label())
}

#[tauri::command]
pub async fn browser_workspace_save(
    req: IpcRequest<BrowserWorkspaceSavePayload>,
    caller: Webview,
    state: State<'_, AppState>,
) -> Result<BrowserWorkspaceResponse, IpcError> {
    req.check_version()?;
    browser_workspace_save_impl(req.payload, &state, caller.label())
}

#[tauri::command]
pub async fn browser_workspace_reset(
    req: IpcRequest<BrowserWorkspaceResetPayload>,
    caller: Webview,
    state: State<'_, AppState>,
) -> Result<BrowserWorkspaceResponse, IpcError> {
    req.check_version()?;
    browser_workspace_reset_impl(req.payload, &state, caller.label())
}

fn browser_workspace_load_impl(
    payload: BrowserWorkspaceLoadPayload,
    state: &AppState,
    caller_label: &str,
) -> Result<BrowserWorkspaceLoadResponse, IpcError> {
    require_main_webview(caller_label)?;
    let dir = scope_dir(payload.identity.scope, state)?;
    let scope = workspace_scope(payload.identity.scope);
    match load_browser_workspace(&dir, &payload.identity.session_id, scope)
        .map_err(map_store_err)?
    {
        BrowserWorkspaceLoad::Missing => Ok(BrowserWorkspaceLoadResponse {
            workspace: None,
            recovery_notice: None,
        }),
        BrowserWorkspaceLoad::Ready(workspace) => Ok(BrowserWorkspaceLoadResponse {
            workspace: Some(workspace),
            recovery_notice: None,
        }),
        BrowserWorkspaceLoad::ResetCorrupt { .. } => Ok(BrowserWorkspaceLoadResponse {
            workspace: None,
            recovery_notice: Some(BrowserWorkspaceRecovery::BrowserStateReset),
        }),
    }
}

fn browser_workspace_save_impl(
    payload: BrowserWorkspaceSavePayload,
    state: &AppState,
    caller_label: &str,
) -> Result<BrowserWorkspaceResponse, IpcError> {
    require_main_webview(caller_label)?;
    let dir = scope_dir(payload.identity.scope, state)?;
    let scope = workspace_scope(payload.identity.scope);
    let workspace = replace_browser_workspace(
        &dir,
        &payload.identity.session_id,
        scope,
        &payload.workspace,
    )
    .map_err(map_store_err)?;
    Ok(BrowserWorkspaceResponse { workspace })
}

fn browser_workspace_reset_impl(
    payload: BrowserWorkspaceResetPayload,
    state: &AppState,
    caller_label: &str,
) -> Result<BrowserWorkspaceResponse, IpcError> {
    require_main_webview(caller_label)?;
    let dir = scope_dir(payload.identity.scope, state)?;
    let scope = workspace_scope(payload.identity.scope);
    let workspace = reset_browser_workspace(&dir, &payload.identity.session_id, scope)
        .map_err(map_store_err)?;
    Ok(BrowserWorkspaceResponse { workspace })
}

fn workspace_scope(scope: SessionScope) -> BrowserWorkspaceScope {
    match scope {
        SessionScope::Local => BrowserWorkspaceScope::Local,
        SessionScope::Project => BrowserWorkspaceScope::Project,
    }
}

fn require_main_webview(label: &str) -> Result<(), IpcError> {
    if label == "main" {
        Ok(())
    } else {
        Err(IpcError::Blocked(
            "browser workspace commands are restricted to the main webview".into(),
        ))
    }
}

#[cfg(test)]
#[path = "browser_workspace_tests.rs"]
mod browser_workspace_tests;
