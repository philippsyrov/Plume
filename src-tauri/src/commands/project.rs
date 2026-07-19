//! Project IPC commands.
//!
//! All four mirror `docs/IPC_CONTRACT.md` § project. Each one
//! validates the IPC envelope version first, then canonicalizes the
//! supplied path, then touches state.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::chat::stream::ChatStreamRegistry;
use crate::commands::task_browser::LiveBrowserRuntime;
use crate::error::{IpcError, IpcRequest};
use crate::project::trust::TrustStore;
use crate::project::{self, ProjectMeta, ProjectSession, TrustState};
use crate::providers::catalog::CatalogStore;
use crate::providers::catalog_download::CatalogDownloadRegistry;
use crate::research::run_registry::ResearchRunRegistry;
use crate::safety::path::canonicalize_root;

/// Process-wide state managed by Tauri. One instance, set up at app
/// launch.
pub struct AppState {
    pub session: Arc<ProjectSession>,
    pub trust: Mutex<TrustStore>,
    /// D7.1: in-flight chat streams indexed by stream id. Wrapped in
    /// `Arc` so the background streaming task can hold a handle
    /// across `spawn_blocking` without borrowing `AppState`.
    pub chat_streams: Arc<ChatStreamRegistry>,
    /// Active bounded research runs. Project changes cancel every captured
    /// generation before replacing the window's project identity.
    pub research_runs: Arc<ResearchRunRegistry>,
    /// D77: the session's agent-autonomy config (mode / approval policy /
    /// allowlists / iteration cap). Window-scoped; reset to default on
    /// every `project.open` so one project's allowlists never carry into
    /// another. See `crate::agent`.
    pub agent_config: Mutex<crate::agent::AgentConfig>,
    /// D63A: absolute directory of the LOCAL chat-session database
    /// (`<app-data>/sessions`). Resolved once at startup and never
    /// derived from any open project, so opening or closing a project
    /// cannot change which database backs local chat.
    pub local_sessions_dir: PathBuf,
    /// App-private user-memory directory (`<app-data>/memory`). The backend
    /// owns this path; IPC callers cannot select or override it.
    pub user_memory_dir: PathBuf,
    /// Fixed app-owned catalog. Its receipt path remains under the Tauri
    /// app-data root and does not depend on any open project's trust state.
    pub catalog_store: Arc<CatalogStore>,
    /// Bounded app-owned download operations. This never depends on project
    /// trust because its fixed target is under the app-data root, not a project.
    pub catalog_downloads: Arc<CatalogDownloadRegistry>,
}

#[derive(Debug, Deserialize)]
pub struct PathPayload {
    pub path: String,
}

/// `project.refresh` payload is empty. Defining a struct (rather than
/// `()`) keeps the wire form `{ "payload": {} }` which is what the TS
/// envelope produces.
#[derive(Debug, Deserialize)]
pub struct EmptyPayload {}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrustStateResponse {
    pub trusted: bool,
}

#[tauri::command]
pub async fn project_open(
    req: IpcRequest<PathPayload>,
    state: State<'_, AppState>,
    browser_runtime: State<'_, LiveBrowserRuntime>,
) -> Result<ProjectMeta, IpcError> {
    req.check_version()?;
    let raw = PathBuf::from(req.payload.path);
    if raw.as_os_str().is_empty() {
        return Err(IpcError::BadArgument("path is empty".into()));
    }
    let root = canonicalize_root(&raw)?;
    let trust_state = {
        let store = state.trust.lock().expect("trust mutex poisoned");
        if store.is_trusted(&root) {
            TrustState::Trusted
        } else {
            TrustState::Unknown
        }
    };
    browser_runtime
        .deactivate_all()
        .map_err(|error| IpcError::Internal(error.to_string()))?;
    state.research_runs.cancel_all();
    let id = state.session.open(root.clone());
    // D77: a fresh project is a fresh session — reset agent autonomy to
    // the least-privilege default so a prior project's allowlists (which
    // are project-relative) can't leak into this one.
    {
        let mut cfg = state.agent_config.lock().expect("agent config poisoned");
        *cfg = crate::agent::AgentConfig::default();
    }
    Ok(project::build_meta(&id, &root, trust_state))
}

#[tauri::command]
pub async fn project_refresh(
    req: IpcRequest<EmptyPayload>,
    state: State<'_, AppState>,
) -> Result<ProjectMeta, IpcError> {
    req.check_version()?;
    let open = state
        .session
        .current()
        .ok_or_else(|| IpcError::BadArgument("no project is open".into()))?;
    let trust_state = {
        let store = state.trust.lock().expect("trust mutex poisoned");
        if store.is_trusted(&open.root) {
            TrustState::Trusted
        } else {
            TrustState::Unknown
        }
    };
    Ok(project::build_meta(&open.id, &open.root, trust_state))
}

#[tauri::command]
pub async fn project_trust(
    req: IpcRequest<PathPayload>,
    state: State<'_, AppState>,
) -> Result<ProjectMeta, IpcError> {
    req.check_version()?;
    let raw = PathBuf::from(req.payload.path);
    if raw.as_os_str().is_empty() {
        return Err(IpcError::BadArgument("path is empty".into()));
    }
    let root = canonicalize_root(&raw)?;
    // Enforce the documented flow: open the project, see the trust
    // prompt, click Trust on *that* project. Without this check the
    // verb becomes a broad "mark any folder trusted" primitive — and
    // would also let the caller flip trust on a path that has never
    // been audited, then immediately spawn git against it via the
    // returned trusted `ProjectMeta`.
    let open = state
        .session
        .current()
        .ok_or_else(|| IpcError::BadArgument("no project is open".into()))?;
    if open.root != root {
        return Err(IpcError::BadArgument(
            "project.trust only accepts the currently-open project root".into(),
        ));
    }
    {
        let mut store = state.trust.lock().expect("trust mutex poisoned");
        store
            .mark_trusted(&root)
            .map_err(|err| IpcError::Internal(format!("failed to persist trust: {err}")))?;
    }
    Ok(project::build_meta(&open.id, &root, TrustState::Trusted))
}

#[tauri::command]
pub async fn project_trust_state(
    req: IpcRequest<PathPayload>,
    state: State<'_, AppState>,
) -> Result<TrustStateResponse, IpcError> {
    req.check_version()?;
    let raw = PathBuf::from(req.payload.path);
    if raw.as_os_str().is_empty() {
        return Err(IpcError::BadArgument("path is empty".into()));
    }
    let root = canonicalize_root(&raw)?;
    let trusted = {
        let store = state.trust.lock().expect("trust mutex poisoned");
        store.is_trusted(&root)
    };
    Ok(TrustStateResponse { trusted })
}
