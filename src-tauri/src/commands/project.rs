//! Project IPC commands.
//!
//! These commands mirror `docs/IPC_CONTRACT.md` § project. Each one
//! validates the IPC envelope version first, then canonicalizes the
//! supplied path, then touches state.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};

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
pub async fn project_choose_folder(
    req: IpcRequest<EmptyPayload>,
    app: AppHandle,
) -> Result<Option<String>, IpcError> {
    req.check_version()?;
    // The native panel stays open for human input. Keep its blocking channel
    // wait off Tauri's async executor, matching the research export dialog.
    tauri::async_runtime::spawn_blocking(move || choose_native_project_folder(&app))
        .await
        .map_err(|error| IpcError::Internal(format!("join folder chooser: {error}")))?
}

fn choose_native_project_folder(app: &AppHandle) -> Result<Option<String>, IpcError> {
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    app.run_on_main_thread(move || {
        let _ = sender.send(show_native_project_folder_panel());
    })
    .map_err(|error| IpcError::Internal(format!("schedule folder chooser: {error}")))?;
    receiver
        .recv_timeout(std::time::Duration::from_secs(15 * 60))
        .map_err(|error| IpcError::Internal(format!("wait for folder chooser: {error}")))?
}

#[cfg(target_os = "macos")]
fn show_native_project_folder_panel() -> Result<Option<String>, IpcError> {
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSModalResponseCancel, NSModalResponseOK, NSOpenPanel};
    use objc2_foundation::NSString;

    let marker = MainThreadMarker::new()
        .ok_or_else(|| IpcError::Internal("folder chooser was not on the main thread".into()))?;
    let panel = NSOpenPanel::openPanel(marker);
    panel.setCanChooseDirectories(true);
    panel.setCanChooseFiles(false);
    panel.setAllowsMultipleSelection(false);
    panel.setResolvesAliases(true);
    panel.setTitle(Some(&NSString::from_str("Open a project")));
    panel.setMessage(Some(&NSString::from_str("Choose a project folder.")));
    match panel.runModal() {
        response if response == NSModalResponseCancel => Ok(None),
        response if response == NSModalResponseOK => panel
            .URL()
            .and_then(|url| url.path())
            .map(|path| Some(path.to_string()))
            .ok_or_else(|| IpcError::Internal("folder chooser returned no path".into())),
        response => Err(IpcError::Internal(format!(
            "folder chooser returned response {response}"
        ))),
    }
}

#[cfg(not(target_os = "macos"))]
fn show_native_project_folder_panel() -> Result<Option<String>, IpcError> {
    Err(IpcError::Internal(
        "native project folder selection is available on macOS".into(),
    ))
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
    let id = transition_project_identity(
        &state.chat_streams,
        &state.research_runs,
        || {
            browser_runtime
                .deactivate_all()
                .map_err(|error| IpcError::Internal(error.to_string()))
        },
        || state.session.open(root.clone()),
    )?;
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
pub async fn project_close(
    req: IpcRequest<EmptyPayload>,
    state: State<'_, AppState>,
    browser_runtime: State<'_, LiveBrowserRuntime>,
) -> Result<(), IpcError> {
    req.check_version()?;
    transition_project_identity(
        &state.chat_streams,
        &state.research_runs,
        || {
            browser_runtime
                .deactivate_all()
                .map_err(|error| IpcError::Internal(error.to_string()))
        },
        || state.session.close(),
    )?;
    let mut cfg = state.agent_config.lock().expect("agent config poisoned");
    *cfg = crate::agent::AgentConfig::default();
    Ok(())
}

fn transition_project_identity<T, D, M>(
    chat_streams: &ChatStreamRegistry,
    research_runs: &ResearchRunRegistry,
    deactivate_browsers: D,
    mutate_identity: M,
) -> Result<T, IpcError>
where
    D: FnOnce() -> Result<(), IpcError>,
    M: FnOnce() -> T,
{
    deactivate_browsers()?;
    Ok(chat_streams.cancel_all_and_transition(|| {
        research_runs.cancel_all();
        mutate_identity()
    }))
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

#[cfg(test)]
mod tests {
    use std::sync::atomic::Ordering;

    use super::*;

    #[test]
    fn replacing_project_cancels_live_work_before_session_mutation() {
        let session = ProjectSession::default();
        let old_root = PathBuf::from("/project/old");
        let new_root = PathBuf::from("/project/new");
        session.open(old_root.clone());
        let chat_streams = ChatStreamRegistry::default();
        let first_chat = chat_streams
            .register("chat-one".into())
            .expect("first chat");
        let second_chat = chat_streams
            .register("chat-two".into())
            .expect("second chat");
        let research_runs = Arc::new(ResearchRunRegistry::default());
        let research = research_runs
            .register("run_one", "project:old:session")
            .expect("research run");

        let id = transition_project_identity(
            &chat_streams,
            &research_runs,
            || {
                assert_eq!(
                    session.current().expect("old project remains open").root,
                    old_root,
                    "Browser teardown must happen before project identity changes",
                );
                Ok(())
            },
            || {
                assert!(first_chat.load(Ordering::SeqCst));
                assert!(second_chat.load(Ordering::SeqCst));
                assert!(research.cancel_flag().load(Ordering::SeqCst));
                assert_eq!(
                    session.current().expect("old project remains open").root,
                    old_root,
                    "live work must be cancelled before project identity changes",
                );
                session.open(new_root.clone())
            },
        )
        .expect("project replacement");

        assert!(!id.is_empty());
        assert_eq!(session.current().expect("new project open").root, new_root);
    }

    #[test]
    fn closing_project_cancels_live_work_before_clearing_identity() {
        let session = ProjectSession::default();
        let old_root = PathBuf::from("/project/old");
        session.open(old_root.clone());
        let chat_streams = ChatStreamRegistry::default();
        let chat = chat_streams.register("chat-one".into()).expect("chat");
        let research_runs = Arc::new(ResearchRunRegistry::default());
        let research = research_runs
            .register("run_one", "project:old:session")
            .expect("research run");

        transition_project_identity(
            &chat_streams,
            &research_runs,
            || Ok(()),
            || {
                assert!(chat.load(Ordering::SeqCst));
                assert!(research.cancel_flag().load(Ordering::SeqCst));
                assert_eq!(
                    session.current().expect("old project remains open").root,
                    old_root,
                );
                session.close();
            },
        )
        .expect("project close");

        assert!(session.current().is_none());
    }
}
