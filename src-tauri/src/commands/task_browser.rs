//! Session-bound native Browser runtime commands.
//!
//! The main webview supplies only opaque session/tab identities and visible
//! geometry. Session scope is resolved again server-side, activation is
//! compared with the persisted Browser descriptor, and every later mutation
//! must match the one currently selected runtime identity.

use serde::Deserialize;
use tauri::{State, WebviewWindow};

use crate::browser::policy::{loopback_origin, validate_browser_url, BrowserNetworkTarget};
use crate::browser::runtime::{
    BrowserBounds, BrowserRuntimeError, BrowserRuntimeIdentity, BrowserRuntimeManager,
    LiveTabIdentity, TauriBrowserRuntimePort,
};
pub use crate::commands::browser_workspace::SessionIdentity;
use crate::commands::project::AppState;
use crate::commands::sessions::{map_store_err, scope_dir, SessionScope};
use crate::error::{IpcError, IpcRequest};
use crate::sessions;
use crate::sessions::browser_workspace::{
    load_browser_workspace, BrowserWorkspaceLoad, BrowserWorkspaceRecord, BrowserWorkspaceScope,
};

pub(crate) type LiveBrowserRuntime = BrowserRuntimeManager<TauriBrowserRuntimePort>;

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TaskBrowserTabPayload {
    pub tab_id: String,
    pub url: Option<String>,
    pub manual_reopen_required: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TaskBrowserActivatePayload {
    pub identity: SessionIdentity,
    pub tabs: Vec<TaskBrowserTabPayload>,
    pub active_tab_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TaskBrowserIdentityPayload {
    pub identity: SessionIdentity,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TaskBrowserOpenTabPayload {
    pub identity: SessionIdentity,
    pub tab: TaskBrowserTabPayload,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TaskBrowserTabActionPayload {
    pub identity: SessionIdentity,
    pub tab_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TaskBrowserNavigatePayload {
    pub identity: SessionIdentity,
    pub tab_id: String,
    pub url: String,
    pub approved_loopback_origin: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BrowserHostRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub scale_factor: f64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TaskBrowserSetGeometryPayload {
    pub identity: SessionIdentity,
    pub host: BrowserHostRect,
}

#[tauri::command]
pub async fn task_browser_activate(
    req: IpcRequest<TaskBrowserActivatePayload>,
    caller: WebviewWindow,
    state: State<'_, AppState>,
    runtime: State<'_, LiveBrowserRuntime>,
) -> Result<(), IpcError> {
    req.check_version()?;
    task_browser_activate_impl(req.payload, &state, &runtime, caller.label())
}

#[tauri::command]
pub async fn task_browser_deactivate(
    req: IpcRequest<TaskBrowserIdentityPayload>,
    caller: WebviewWindow,
    state: State<'_, AppState>,
    runtime: State<'_, LiveBrowserRuntime>,
) -> Result<(), IpcError> {
    req.check_version()?;
    task_browser_deactivate_impl(req.payload, &state, &runtime, caller.label())
}

#[tauri::command]
pub async fn task_browser_open_tab(
    req: IpcRequest<TaskBrowserOpenTabPayload>,
    caller: WebviewWindow,
    state: State<'_, AppState>,
    runtime: State<'_, LiveBrowserRuntime>,
) -> Result<(), IpcError> {
    req.check_version()?;
    task_browser_open_tab_impl(req.payload, &state, &runtime, caller.label())
}

#[tauri::command]
pub async fn task_browser_close_tab(
    req: IpcRequest<TaskBrowserTabActionPayload>,
    caller: WebviewWindow,
    state: State<'_, AppState>,
    runtime: State<'_, LiveBrowserRuntime>,
) -> Result<Option<String>, IpcError> {
    req.check_version()?;
    task_browser_close_tab_impl(req.payload, &state, &runtime, caller.label())
}

#[tauri::command]
pub async fn task_browser_select_tab(
    req: IpcRequest<TaskBrowserTabActionPayload>,
    caller: WebviewWindow,
    state: State<'_, AppState>,
    runtime: State<'_, LiveBrowserRuntime>,
) -> Result<(), IpcError> {
    req.check_version()?;
    task_browser_select_tab_impl(req.payload, &state, &runtime, caller.label())
}

#[tauri::command]
pub async fn task_browser_navigate(
    req: IpcRequest<TaskBrowserNavigatePayload>,
    caller: WebviewWindow,
    state: State<'_, AppState>,
    runtime: State<'_, LiveBrowserRuntime>,
) -> Result<(), IpcError> {
    req.check_version()?;
    task_browser_navigate_impl(req.payload, &state, &runtime, caller.label())
}

macro_rules! fixed_tab_command {
    ($name:ident, $implementation:ident, $method:ident) => {
        #[tauri::command]
        pub async fn $name(
            req: IpcRequest<TaskBrowserTabActionPayload>,
            caller: WebviewWindow,
            state: State<'_, AppState>,
            runtime: State<'_, LiveBrowserRuntime>,
        ) -> Result<(), IpcError> {
            req.check_version()?;
            $implementation(req.payload, &state, &runtime, caller.label())
        }

        pub(crate) fn $implementation<P: crate::browser::runtime::BrowserRuntimePort>(
            payload: TaskBrowserTabActionPayload,
            state: &AppState,
            runtime: &BrowserRuntimeManager<P>,
            caller_label: &str,
        ) -> Result<(), IpcError> {
            require_main_webview(caller_label)?;
            let identity = require_owned_session(&payload.identity, state)?;
            runtime
                .$method(&identity, &payload.tab_id)
                .map_err(map_runtime_error)
        }
    };
}

fixed_tab_command!(task_browser_back, task_browser_back_impl, back);
fixed_tab_command!(task_browser_forward, task_browser_forward_impl, forward);
fixed_tab_command!(task_browser_reload, task_browser_reload_impl, reload);

#[tauri::command]
pub async fn task_browser_set_geometry(
    req: IpcRequest<TaskBrowserSetGeometryPayload>,
    caller: WebviewWindow,
    state: State<'_, AppState>,
    runtime: State<'_, LiveBrowserRuntime>,
) -> Result<(), IpcError> {
    req.check_version()?;
    task_browser_set_geometry_impl(req.payload, &state, &runtime, caller.label())
}

pub(crate) fn task_browser_activate_impl<P: crate::browser::runtime::BrowserRuntimePort>(
    payload: TaskBrowserActivatePayload,
    state: &AppState,
    runtime: &BrowserRuntimeManager<P>,
    caller_label: &str,
) -> Result<(), IpcError> {
    require_main_webview(caller_label)?;
    let identity = require_owned_session(&payload.identity, state)?;
    let dir = scope_dir(payload.identity.scope, state)?;
    let record = match load_browser_workspace(&dir, &payload.identity.session_id, identity.scope)
        .map_err(map_store_err)?
    {
        BrowserWorkspaceLoad::Ready(record) => record,
        BrowserWorkspaceLoad::Missing => {
            return Err(IpcError::NotFound("browser.workspace".into()))
        }
        BrowserWorkspaceLoad::ResetCorrupt { .. } => {
            return Err(IpcError::BadArgument("browser.workspaceReset".into()))
        }
    };
    let expected_tabs = activation_tabs(&record);
    if payload.tabs != expected_tabs
        || record.active_tab_id.as_deref() != Some(&payload.active_tab_id)
    {
        return Err(IpcError::BadArgument(
            "browser activation does not match persisted workspace".into(),
        ));
    }

    let initial_bounds = BrowserBounds::new(0.0, 0.0, 1.0, 1.0).expect("static bounds are valid");
    let plans = payload
        .tabs
        .into_iter()
        .enumerate()
        .map(|(index, tab)| {
            let url = initial_url(&tab)?;
            Ok(BrowserRuntimeManager::<P>::plan_child(
                LiveTabIdentity {
                    workspace: identity.clone(),
                    tab_id: tab.tab_id,
                    generation: index as u64 + 1,
                },
                url,
                initial_bounds,
            ))
        })
        .collect::<Result<Vec<_>, IpcError>>()?;
    runtime
        .activate(plans, &payload.active_tab_id)
        .map_err(map_runtime_error)
}

pub(crate) fn task_browser_deactivate_impl<P: crate::browser::runtime::BrowserRuntimePort>(
    payload: TaskBrowserIdentityPayload,
    state: &AppState,
    runtime: &BrowserRuntimeManager<P>,
    caller_label: &str,
) -> Result<(), IpcError> {
    require_main_webview(caller_label)?;
    let identity = require_owned_session(&payload.identity, state)?;
    runtime.deactivate(&identity).map_err(map_runtime_error)
}

pub(crate) fn task_browser_open_tab_impl<P: crate::browser::runtime::BrowserRuntimePort>(
    payload: TaskBrowserOpenTabPayload,
    state: &AppState,
    runtime: &BrowserRuntimeManager<P>,
    caller_label: &str,
) -> Result<(), IpcError> {
    require_main_webview(caller_label)?;
    let identity = require_owned_session(&payload.identity, state)?;
    validate_tab_id(&payload.tab.tab_id)?;
    if payload.tab.manual_reopen_required {
        return Err(IpcError::BadArgument(
            "new browser tabs cannot require manual reopen".into(),
        ));
    }
    let url = initial_url(&payload.tab)?;
    let plan = BrowserRuntimeManager::<P>::plan_new_child(
        LiveTabIdentity {
            workspace: identity.clone(),
            tab_id: payload.tab.tab_id,
            generation: 1,
        },
        url,
        BrowserBounds::new(0.0, 0.0, 1.0, 1.0).expect("static bounds are valid"),
    );
    runtime
        .open_tab(&identity, plan, true)
        .map_err(map_runtime_error)
}

pub(crate) fn task_browser_close_tab_impl<P: crate::browser::runtime::BrowserRuntimePort>(
    payload: TaskBrowserTabActionPayload,
    state: &AppState,
    runtime: &BrowserRuntimeManager<P>,
    caller_label: &str,
) -> Result<Option<String>, IpcError> {
    require_main_webview(caller_label)?;
    let identity = require_owned_session(&payload.identity, state)?;
    runtime
        .close_tab(&identity, &payload.tab_id)
        .map_err(map_runtime_error)
}

pub(crate) fn task_browser_select_tab_impl<P: crate::browser::runtime::BrowserRuntimePort>(
    payload: TaskBrowserTabActionPayload,
    state: &AppState,
    runtime: &BrowserRuntimeManager<P>,
    caller_label: &str,
) -> Result<(), IpcError> {
    require_main_webview(caller_label)?;
    let identity = require_owned_session(&payload.identity, state)?;
    runtime
        .select_tab(&identity, &payload.tab_id)
        .map_err(map_runtime_error)
}

pub(crate) fn task_browser_navigate_impl<P: crate::browser::runtime::BrowserRuntimePort>(
    payload: TaskBrowserNavigatePayload,
    state: &AppState,
    runtime: &BrowserRuntimeManager<P>,
    caller_label: &str,
) -> Result<(), IpcError> {
    require_main_webview(caller_label)?;
    let identity = require_owned_session(&payload.identity, state)?;
    let validated = validate_browser_url(&payload.url)
        .map_err(|_| IpcError::BadArgument("browser.invalidUrl".into()))?;
    if validated.target == BrowserNetworkTarget::Loopback {
        let exact = loopback_origin(&validated).expect("loopback URLs have an origin");
        if payload.identity.scope != SessionScope::Project
            || payload.approved_loopback_origin.as_deref() != Some(exact.as_str())
        {
            return Err(IpcError::NeedsApproval);
        }
        runtime
            .approve_loopback_origin(&identity, &payload.tab_id, &exact)
            .map_err(map_runtime_error)?;
    }
    runtime
        .navigate(&identity, &payload.tab_id, validated.url)
        .map_err(map_runtime_error)
}

pub(crate) fn task_browser_set_geometry_impl<P: crate::browser::runtime::BrowserRuntimePort>(
    payload: TaskBrowserSetGeometryPayload,
    state: &AppState,
    runtime: &BrowserRuntimeManager<P>,
    caller_label: &str,
) -> Result<(), IpcError> {
    require_main_webview(caller_label)?;
    let identity = require_owned_session(&payload.identity, state)?;
    if !payload.host.scale_factor.is_finite() || !(0.5..=4.0).contains(&payload.host.scale_factor) {
        return Err(IpcError::BadArgument(
            "browser scale factor is invalid".into(),
        ));
    }
    let bounds = BrowserBounds::new(
        payload.host.x * payload.host.scale_factor,
        payload.host.y * payload.host.scale_factor,
        payload.host.width * payload.host.scale_factor,
        payload.host.height * payload.host.scale_factor,
    )
    .map_err(map_runtime_error)?;
    runtime
        .set_bounds(&identity, bounds)
        .map_err(map_runtime_error)
}

fn require_owned_session(
    identity: &SessionIdentity,
    state: &AppState,
) -> Result<BrowserRuntimeIdentity, IpcError> {
    let dir = scope_dir(identity.scope, state)?;
    if !sessions::session_exists(&dir, &identity.session_id).map_err(map_store_err)? {
        return Err(IpcError::NotFound("browser.task".into()));
    }
    Ok(BrowserRuntimeIdentity {
        scope: workspace_scope(identity.scope),
        session_id: identity.session_id.clone(),
    })
}

fn activation_tabs(record: &BrowserWorkspaceRecord) -> Vec<TaskBrowserTabPayload> {
    record
        .tabs
        .iter()
        .map(|tab| TaskBrowserTabPayload {
            tab_id: tab.id.clone(),
            url: tab
                .current_history_index
                .map(|index| tab.history[index].url.clone()),
            manual_reopen_required: tab.manual_reopen_required,
        })
        .collect()
}

fn initial_url(tab: &TaskBrowserTabPayload) -> Result<tauri::Url, IpcError> {
    if tab.manual_reopen_required || tab.url.is_none() {
        return tauri::Url::parse("about:blank")
            .map_err(|_| IpcError::Internal("browser.blankUrlInvalid".into()));
    }
    validate_browser_url(tab.url.as_deref().expect("checked above"))
        .map(|validated| validated.url)
        .map_err(|_| IpcError::BadArgument("browser.invalidUrl".into()))
}

fn validate_tab_id(tab_id: &str) -> Result<(), IpcError> {
    let tail = tab_id
        .strip_prefix("bt_")
        .ok_or_else(|| IpcError::BadArgument("browser.invalidTabId".into()))?;
    if tail.len() != 32 || !tail.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(IpcError::BadArgument("browser.invalidTabId".into()));
    }
    Ok(())
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
            "task Browser commands are restricted to the main webview".into(),
        ))
    }
}

fn map_runtime_error(error: BrowserRuntimeError) -> IpcError {
    match error {
        BrowserRuntimeError::WorkspaceNotSelected | BrowserRuntimeError::TabNotFound => {
            IpcError::NotFound("browser.task".into())
        }
        BrowserRuntimeError::TabLimit => IpcError::Blocked("browser.tabLimit".into()),
        BrowserRuntimeError::InvalidBounds
        | BrowserRuntimeError::ActiveTabMissing
        | BrowserRuntimeError::WorkspaceMismatch
        | BrowserRuntimeError::TabAlreadyExists => IpcError::BadArgument(error.to_string()),
        BrowserRuntimeError::MainWindowMissing | BrowserRuntimeError::Native(_) => {
            IpcError::Internal(error.to_string())
        }
    }
}
