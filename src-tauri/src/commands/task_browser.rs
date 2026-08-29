//! Session-bound native Browser runtime commands.
//!
//! The main webview supplies only opaque session/tab identities and visible
//! geometry. Session scope is resolved again server-side, activation is
//! compared with the persisted Browser descriptor, and every later mutation
//! must match the one currently selected runtime identity.

use std::sync::mpsc;
use std::time::Duration;

use tauri::{AppHandle, Manager, State, Webview};

use crate::browser::evidence::store_text_evidence;
use crate::browser::local_evidence::{
    store_local_screenshot_evidence, store_local_text_evidence, LocalEvidenceError,
    LocalEvidenceOwner,
};
#[cfg(target_os = "macos")]
use crate::browser::native_snapshot::request_visible_webview_snapshot;
use crate::browser::policy::{loopback_origin, validate_browser_url, BrowserNetworkTarget};
use crate::browser::runtime::{
    BrowserBounds, BrowserRuntimeError, BrowserRuntimeIdentity, BrowserRuntimeManager,
    LiveTabIdentity, TauriBrowserRuntimePort, MAX_NATIVE_TABS,
};
use crate::browser::screenshot_evidence::{store_screenshot_evidence, CapturedBrowserScreenshot};
pub use crate::commands::browser_workspace::SessionIdentity;
use crate::commands::project::AppState;
use crate::commands::sessions::{map_store_err, scope_dir, SessionScope};
use crate::error::{IpcError, IpcRequest};
use crate::project::OpenProject;
use crate::prompts::ContextSourceRef;
use crate::sessions;
use crate::sessions::browser_workspace::{
    load_browser_workspace, BrowserHistoryNavigation, BrowserWorkspaceLoad, BrowserWorkspaceScope,
};

#[path = "task_browser_activation.rs"]
mod activation;
use activation::{activation_tabs, activation_tabs_match, initial_url};

#[path = "task_browser_types.rs"]
mod types;
pub use types::*;

pub(crate) type LiveBrowserRuntime = BrowserRuntimeManager<TauriBrowserRuntimePort>;

#[tauri::command]
pub async fn task_browser_activate(
    req: IpcRequest<TaskBrowserActivatePayload>,
    caller: Webview,
    state: State<'_, AppState>,
    runtime: State<'_, LiveBrowserRuntime>,
) -> Result<(), IpcError> {
    req.check_version()?;
    task_browser_activate_impl(req.payload, &state, &runtime, caller.label())
}

#[tauri::command]
pub async fn task_browser_deactivate(
    req: IpcRequest<TaskBrowserIdentityPayload>,
    caller: Webview,
    state: State<'_, AppState>,
    runtime: State<'_, LiveBrowserRuntime>,
) -> Result<(), IpcError> {
    req.check_version()?;
    task_browser_deactivate_impl(req.payload, &state, &runtime, caller.label())
}

#[tauri::command]
pub async fn task_browser_set_suspended(
    req: IpcRequest<TaskBrowserSuspensionPayload>,
    caller: Webview,
    state: State<'_, AppState>,
    runtime: State<'_, LiveBrowserRuntime>,
) -> Result<(), IpcError> {
    req.check_version()?;
    task_browser_set_suspended_impl(req.payload, &state, &runtime, caller.label())
}

#[tauri::command]
pub async fn task_browser_open_tab(
    req: IpcRequest<TaskBrowserOpenTabPayload>,
    caller: Webview,
    state: State<'_, AppState>,
    runtime: State<'_, LiveBrowserRuntime>,
) -> Result<(), IpcError> {
    req.check_version()?;
    task_browser_open_tab_impl(req.payload, &state, &runtime, caller.label())
}

#[tauri::command]
pub async fn task_browser_close_tab(
    req: IpcRequest<TaskBrowserTabActionPayload>,
    caller: Webview,
    state: State<'_, AppState>,
    runtime: State<'_, LiveBrowserRuntime>,
) -> Result<Option<String>, IpcError> {
    req.check_version()?;
    task_browser_close_tab_impl(req.payload, &state, &runtime, caller.label())
}

#[tauri::command]
pub async fn task_browser_select_tab(
    req: IpcRequest<TaskBrowserTabActionPayload>,
    caller: Webview,
    state: State<'_, AppState>,
    runtime: State<'_, LiveBrowserRuntime>,
) -> Result<(), IpcError> {
    req.check_version()?;
    task_browser_select_tab_impl(req.payload, &state, &runtime, caller.label())
}

#[tauri::command]
pub async fn task_browser_navigate(
    req: IpcRequest<TaskBrowserNavigatePayload>,
    caller: Webview,
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
            caller: Webview,
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

fixed_tab_command!(task_browser_reload, task_browser_reload_impl, reload);

macro_rules! guarded_history_command {
    ($name:ident, $implementation:ident, $navigation:expr) => {
        #[tauri::command]
        pub async fn $name(
            req: IpcRequest<TaskBrowserTabActionPayload>,
            caller: Webview,
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
            let target = require_history_target(&payload, state, $navigation)?;
            if target.target == BrowserNetworkTarget::Loopback {
                let exact = loopback_origin(&target).expect("loopback URLs have an origin");
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
                .navigate_history(&identity, &payload.tab_id, target.url, $navigation)
                .map_err(map_runtime_error)
        }
    };
}

guarded_history_command!(
    task_browser_back,
    task_browser_back_impl,
    BrowserHistoryNavigation::Back
);
guarded_history_command!(
    task_browser_forward,
    task_browser_forward_impl,
    BrowserHistoryNavigation::Forward
);

#[tauri::command]
pub async fn task_browser_set_geometry(
    req: IpcRequest<TaskBrowserSetGeometryPayload>,
    caller: Webview,
    state: State<'_, AppState>,
    runtime: State<'_, LiveBrowserRuntime>,
) -> Result<(), IpcError> {
    req.check_version()?;
    task_browser_set_geometry_impl(req.payload, &state, &runtime, caller.label())
}

#[tauri::command]
pub async fn task_browser_capture_text(
    req: IpcRequest<TaskBrowserCaptureTextPayload>,
    caller: Webview,
    state: State<'_, AppState>,
    runtime: State<'_, LiveBrowserRuntime>,
) -> Result<TaskBrowserCaptureTextResponse, IpcError> {
    req.check_version()?;
    require_main_webview(caller.label())?;
    let payload = req.payload;
    let project_owner = capture_project_owner(payload.identity.scope, &state)?;
    let identity = require_owned_session(&payload.identity, &state)?;
    let ticket = runtime
        .capture_ticket(&identity, &payload.tab_id)
        .map_err(map_runtime_error)?;
    let (sender, receiver) = mpsc::sync_channel(1);
    runtime
        .evaluate_capture(
            &ticket,
            crate::commands::browser::fixed_capture_script(payload.capture_kind),
            sender,
        )
        .map_err(map_runtime_error)?;
    let raw =
        tauri::async_runtime::spawn_blocking(move || receiver.recv_timeout(Duration::from_secs(3)))
            .await
            .map_err(|_| IpcError::Internal("browser.captureCallbackFailed".into()))?
            .map_err(|_| IpcError::Internal("browser.captureTimedOut".into()))?;
    let capture = crate::commands::browser::parse_capture_observation(&raw, payload.capture_kind)?;
    if capture.source_url != ticket.current_url || !runtime.capture_ticket_is_current(&ticket) {
        return Err(IpcError::Blocked("browser.capturePageChanged".into()));
    }

    let evidence = match payload.identity.scope {
        SessionScope::Local => {
            let sessions_dir = state.local_sessions_dir.clone();
            let owner = LocalEvidenceOwner {
                session_id: payload.identity.session_id.clone(),
            };
            tauri::async_runtime::spawn_blocking(move || {
                store_local_text_evidence(&sessions_dir, &owner, capture)
            })
            .await
            .map_err(|_| IpcError::Internal("browser.evidenceStoreFailed".into()))?
            .map_err(map_local_evidence_error)?
        }
        SessionScope::Project => {
            let project = project_owner.ok_or(IpcError::NeedsApproval)?;
            require_capture_project_current(&project, &state)?;
            tauri::async_runtime::spawn_blocking(move || {
                store_text_evidence(&project.root, capture)
            })
            .await
            .map_err(|_| IpcError::Internal("browser.evidenceStoreFailed".into()))?
            .map_err(|error| {
                if error.is_capacity() {
                    IpcError::Blocked("browser.evidenceCapacityReached".into())
                } else {
                    IpcError::Internal("browser.evidenceStoreFailed".into())
                }
            })?
        }
    };
    if !runtime.capture_ticket_is_current(&ticket) {
        return Err(IpcError::Blocked("browser.capturePageChanged".into()));
    }
    Ok(TaskBrowserCaptureTextResponse {
        source: ContextSourceRef::BrowserTextEvidence {
            evidence_id: evidence.evidence_id.clone(),
        },
        evidence,
    })
}

#[tauri::command]
pub async fn task_browser_capture_screenshot(
    req: IpcRequest<TaskBrowserCaptureScreenshotPayload>,
    caller: Webview,
    app: AppHandle,
    state: State<'_, AppState>,
    runtime: State<'_, LiveBrowserRuntime>,
) -> Result<TaskBrowserCaptureScreenshotResponse, IpcError> {
    req.check_version()?;
    require_main_webview(caller.label())?;
    let payload = req.payload;
    let project_owner = capture_project_owner(payload.identity.scope, &state)?;
    let identity = require_owned_session(&payload.identity, &state)?;
    let ticket = runtime
        .capture_ticket(&identity, &payload.tab_id)
        .map_err(map_runtime_error)?;
    let webview = app
        .get_webview(&ticket.label)
        .ok_or_else(|| IpcError::NotFound("browser.task".into()))?;

    #[cfg(target_os = "macos")]
    let snapshot = {
        let (sender, receiver) = mpsc::sync_channel(1);
        request_visible_webview_snapshot(&webview, sender)
            .map_err(|_| IpcError::Internal("browser.snapshotRequestFailed".into()))?;
        tauri::async_runtime::spawn_blocking(move || receiver.recv_timeout(Duration::from_secs(5)))
            .await
            .map_err(|_| IpcError::Internal("browser.snapshotCallbackFailed".into()))?
            .map_err(|_| IpcError::Internal("browser.snapshotTimedOut".into()))?
            .map_err(|reason| IpcError::Internal(reason.into()))?
    };

    #[cfg(not(target_os = "macos"))]
    let snapshot = {
        let _ = webview;
        return Err(IpcError::Blocked(
            "browser.snapshotUnsupportedPlatform".into(),
        ));
    };

    if !runtime.capture_ticket_is_current(&ticket) {
        return Err(IpcError::Blocked("browser.capturePageChanged".into()));
    }
    let capture = CapturedBrowserScreenshot {
        source_url: ticket.current_url.clone(),
        title: snapshot.title,
        png_bytes: snapshot.png_bytes,
        width: snapshot.width,
        height: snapshot.height,
    };
    let evidence = match payload.identity.scope {
        SessionScope::Local => {
            let sessions_dir = state.local_sessions_dir.clone();
            let owner = LocalEvidenceOwner {
                session_id: payload.identity.session_id.clone(),
            };
            tauri::async_runtime::spawn_blocking(move || {
                store_local_screenshot_evidence(&sessions_dir, &owner, capture)
            })
            .await
            .map_err(|_| IpcError::Internal("browser.screenshotStoreFailed".into()))?
            .map_err(map_local_evidence_error)?
        }
        SessionScope::Project => {
            let project = project_owner.ok_or(IpcError::NeedsApproval)?;
            require_capture_project_current(&project, &state)?;
            tauri::async_runtime::spawn_blocking(move || {
                store_screenshot_evidence(&project.root, capture)
            })
            .await
            .map_err(|_| IpcError::Internal("browser.screenshotStoreFailed".into()))?
            .map_err(|error| {
                if error.is_capacity() {
                    IpcError::Blocked("browser.screenshotCapacityReached".into())
                } else {
                    IpcError::Internal("browser.screenshotStoreFailed".into())
                }
            })?
        }
    };
    if !runtime.capture_ticket_is_current(&ticket) {
        return Err(IpcError::Blocked("browser.capturePageChanged".into()));
    }
    Ok(TaskBrowserCaptureScreenshotResponse {
        source: ContextSourceRef::BrowserScreenshotEvidence {
            evidence_id: evidence.evidence_id.clone(),
        },
        evidence,
    })
}

pub(crate) fn task_browser_activate_impl<P: crate::browser::runtime::BrowserRuntimePort>(
    payload: TaskBrowserActivatePayload,
    state: &AppState,
    runtime: &BrowserRuntimeManager<P>,
    caller_label: &str,
) -> Result<(), IpcError> {
    require_main_webview(caller_label)?;
    // Held across resolution *and* activation. Ownership and the persisted
    // workspace are resolved against the currently open project; without the
    // fence a concurrent project transition could tear Browser children down in
    // the window between those checks and `activate`, leaving this child alive
    // over a project that is no longer open.
    let _fence = state.session.lifecycle_fence();
    let identity = require_owned_session(&payload.identity, state)?;
    let dir = scope_dir(payload.identity.scope, state)?;
    #[cfg(test)]
    activation_test_hooks::after_resolution();
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
    if !activation_tabs_match(&record, &payload.tabs, payload.identity.scope)
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
        .map_err(map_runtime_error)?;
    Ok(())
}

pub(crate) fn task_browser_deactivate_impl<P: crate::browser::runtime::BrowserRuntimePort>(
    payload: TaskBrowserIdentityPayload,
    state: &AppState,
    runtime: &BrowserRuntimeManager<P>,
    caller_label: &str,
) -> Result<(), IpcError> {
    require_main_webview(caller_label)?;
    let identity = require_owned_session(&payload.identity, state)?;
    runtime.deactivate(&identity).map_err(map_runtime_error)?;
    Ok(())
}

pub(crate) fn task_browser_set_suspended_impl<P: crate::browser::runtime::BrowserRuntimePort>(
    payload: TaskBrowserSuspensionPayload,
    state: &AppState,
    runtime: &BrowserRuntimeManager<P>,
    caller_label: &str,
) -> Result<(), IpcError> {
    require_main_webview(caller_label)?;
    let identity = require_owned_session(&payload.identity, state)?;
    runtime
        .set_suspended(&identity, payload.suspended)
        .map_err(map_runtime_error)
}

pub(crate) fn task_browser_open_tab_impl<P: crate::browser::runtime::BrowserRuntimePort>(
    payload: TaskBrowserOpenTabPayload,
    state: &AppState,
    runtime: &BrowserRuntimeManager<P>,
    caller_label: &str,
) -> Result<(), IpcError> {
    require_main_webview(caller_label)?;
    // Held across resolution *and* activation. Ownership and the persisted
    // workspace are resolved against the currently open project; without the
    // fence a concurrent project transition could tear Browser children down in
    // the window between those checks and `activate`, leaving this child alive
    // over a project that is no longer open.
    let _fence = state.session.lifecycle_fence();
    let identity = require_owned_session(&payload.identity, state)?;
    let dir = scope_dir(payload.identity.scope, state)?;
    #[cfg(test)]
    activation_test_hooks::after_resolution();
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
    if record.tabs.len() > MAX_NATIVE_TABS {
        return Err(IpcError::Blocked("browser.tabLimit".into()));
    }
    validate_tab_id(&payload.tab.tab_id)?;
    if payload.tab.manual_reopen_required {
        return Err(IpcError::BadArgument(
            "new browser tabs cannot require manual reopen".into(),
        ));
    }
    let expected = activation_tabs(&record)
        .into_iter()
        .find(|tab| tab.tab_id == payload.tab.tab_id);
    if expected.as_ref() != Some(&payload.tab)
        || record.active_tab_id.as_deref() != Some(payload.tab.tab_id.as_str())
    {
        if record.tabs.len() >= MAX_NATIVE_TABS {
            return Err(IpcError::Blocked("browser.tabLimit".into()));
        }
        return Err(IpcError::BadArgument(
            "browser tab does not match persisted workspace".into(),
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
    if payload.explicit_reopen {
        runtime
            .reopen(&identity, &payload.tab_id, validated.url)
            .map_err(map_runtime_error)
    } else {
        runtime
            .navigate(&identity, &payload.tab_id, validated.url)
            .map_err(map_runtime_error)
    }
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

fn require_history_target(
    payload: &TaskBrowserTabActionPayload,
    state: &AppState,
    navigation: BrowserHistoryNavigation,
) -> Result<crate::browser::policy::ValidatedBrowserUrl, IpcError> {
    let dir = scope_dir(payload.identity.scope, state)?;
    let scope = workspace_scope(payload.identity.scope);
    let BrowserWorkspaceLoad::Ready(workspace) =
        load_browser_workspace(&dir, &payload.identity.session_id, scope).map_err(map_store_err)?
    else {
        return Err(IpcError::NotFound("browser.task".into()));
    };
    let tab = workspace
        .tabs
        .iter()
        .find(|tab| tab.id == payload.tab_id)
        .ok_or_else(|| IpcError::NotFound("browser.task".into()))?;
    let target_index = match (navigation, tab.current_history_index) {
        (BrowserHistoryNavigation::Back, Some(index)) => index.checked_sub(1),
        (BrowserHistoryNavigation::Forward, Some(index)) if index + 1 < tab.history.len() => {
            Some(index + 1)
        }
        _ => None,
    }
    .ok_or_else(|| IpcError::Blocked("browser.historyUnavailable".into()))?;
    validate_browser_url(&tab.history[target_index].url)
        .map_err(|_| IpcError::BadArgument("browser.invalidUrl".into()))
}

pub(crate) fn capture_project_owner(
    scope: SessionScope,
    state: &AppState,
) -> Result<Option<OpenProject>, IpcError> {
    match scope {
        SessionScope::Local => Ok(None),
        SessionScope::Project => crate::commands::browser::trusted_open(state)
            .map(Some)
            .ok_or(IpcError::NeedsApproval),
    }
}

pub(crate) fn require_capture_project_current(
    expected: &OpenProject,
    state: &AppState,
) -> Result<(), IpcError> {
    let current = crate::commands::browser::trusted_open(state).ok_or(IpcError::NeedsApproval)?;
    if current.id == expected.id && current.root == expected.root {
        Ok(())
    } else {
        Err(IpcError::Blocked("browser.captureProjectChanged".into()))
    }
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
    tracing::warn!(error = %error, "task Browser native operation failed");
    match error {
        BrowserRuntimeError::WorkspaceNotSelected | BrowserRuntimeError::TabNotFound => {
            IpcError::NotFound("browser.task".into())
        }
        BrowserRuntimeError::TabLimit => IpcError::Blocked("browser.tabLimit".into()),
        BrowserRuntimeError::InvalidBounds
        | BrowserRuntimeError::ActiveTabMissing
        | BrowserRuntimeError::WorkspaceMismatch
        | BrowserRuntimeError::TabAlreadyExists => IpcError::BadArgument(error.to_string()),
        BrowserRuntimeError::CapturePageChanged => {
            IpcError::Blocked("browser.capturePageChanged".into())
        }
        BrowserRuntimeError::MainWindowMissing | BrowserRuntimeError::Native(_) => {
            IpcError::Internal(error.to_string())
        }
    }
}

fn map_local_evidence_error(error: LocalEvidenceError) -> IpcError {
    match error {
        LocalEvidenceError::OwnerNotFound => {
            IpcError::NotFound("local Browser evidence owner".into())
        }
        LocalEvidenceError::InvalidOwner => {
            IpcError::BadArgument("invalid local Browser evidence owner".into())
        }
        LocalEvidenceError::Refused(message) => IpcError::Blocked(message),
        LocalEvidenceError::Capacity => {
            IpcError::Blocked("local Browser evidence capacity reached".into())
        }
        LocalEvidenceError::Storage(message) => IpcError::Internal(message),
        LocalEvidenceError::Session(error) => map_store_err(error),
    }
}

/// Deterministic interleaving seam for the lifecycle-fence regression test.
///
/// The race this guards is a window between two steps inside one function, so a
/// test cannot reach it without pausing mid-function. Test-only, and never
/// compiled into the shipped binary.
#[cfg(test)]
pub(crate) mod activation_test_hooks {
    use std::sync::{Arc, Mutex};

    type Hook = Arc<dyn Fn() + Send + Sync>;

    static AFTER_RESOLUTION: Mutex<Option<Hook>> = Mutex::new(None);

    pub(crate) fn set_after_resolution(hook: Hook) {
        *AFTER_RESOLUTION.lock().expect("hook mutex poisoned") = Some(hook);
    }

    pub(crate) fn clear() {
        *AFTER_RESOLUTION.lock().expect("hook mutex poisoned") = None;
    }

    pub(crate) fn after_resolution() {
        let hook = AFTER_RESOLUTION
            .lock()
            .expect("hook mutex poisoned")
            .clone();
        if let Some(hook) = hook {
            hook();
        }
    }
}
