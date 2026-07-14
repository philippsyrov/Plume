use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde_json::json;

use super::project::AppState;
use super::sessions::SessionScope;
use super::task_browser::*;
use crate::browser::policy::validate_browser_url;
use crate::browser::runtime::{
    BrowserBounds, BrowserChildPlan, BrowserRuntimeError, BrowserRuntimeManager, BrowserRuntimePort,
};
use crate::chat::stream::ChatStreamRegistry;
use crate::project::trust::TrustStore;
use crate::project::ProjectSession;
use crate::sessions;
use crate::sessions::browser_workspace::{
    mint_tab_id, replace_browser_workspace, BrowserHistoryNavigation, BrowserHistoryRecord,
    BrowserLayoutMode, BrowserRestorationStatus, BrowserTabRecord, BrowserWorkspaceRecord,
    BrowserWorkspaceScope,
};

#[derive(Default)]
struct RecordingPort {
    added: Mutex<Vec<BrowserChildPlan>>,
    bounds: Mutex<Vec<(String, BrowserBounds)>>,
    visibility: Mutex<Vec<(String, bool)>>,
    navigation: Mutex<Vec<String>>,
    evaluated: Mutex<Vec<String>>,
    closed: Mutex<Vec<String>>,
}

impl BrowserRuntimePort for RecordingPort {
    fn add_child(&self, plan: &BrowserChildPlan) -> Result<(), BrowserRuntimeError> {
        self.added.lock().unwrap().push(plan.clone());
        Ok(())
    }

    fn set_bounds(&self, label: &str, bounds: BrowserBounds) -> Result<(), BrowserRuntimeError> {
        self.bounds.lock().unwrap().push((label.into(), bounds));
        Ok(())
    }

    fn set_visible(&self, label: &str, visible: bool) -> Result<(), BrowserRuntimeError> {
        self.visibility
            .lock()
            .unwrap()
            .push((label.into(), visible));
        Ok(())
    }

    fn eval(&self, _label: &str, script: &str) -> Result<(), BrowserRuntimeError> {
        self.evaluated.lock().unwrap().push(script.into());
        Ok(())
    }

    fn reload(&self, _label: &str) -> Result<(), BrowserRuntimeError> {
        Ok(())
    }

    fn navigate(&self, _label: &str, url: &tauri::Url) -> Result<(), BrowserRuntimeError> {
        self.navigation.lock().unwrap().push(url.to_string());
        Ok(())
    }

    fn close(&self, label: &str) -> Result<(), BrowserRuntimeError> {
        self.closed.lock().unwrap().push(label.into());
        Ok(())
    }
}

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(label: &str) -> Self {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "plume-task-browser-command-{label}-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(&path).unwrap();
        Self { path }
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn state(base: &Path) -> AppState {
    AppState {
        session: ProjectSession::default(),
        trust: Mutex::new(TrustStore::load(base.join("trusted-projects.json"))),
        chat_streams: Arc::new(ChatStreamRegistry::default()),
        agent_config: Mutex::new(crate::agent::AgentConfig::default()),
        local_sessions_dir: base.join("app-data/sessions"),
    }
}

fn identity(scope: SessionScope, session_id: &str) -> SessionIdentity {
    SessionIdentity {
        scope,
        session_id: session_id.into(),
    }
}

fn workspace(
    session_id: &str,
    scope: BrowserWorkspaceScope,
    count: usize,
) -> BrowserWorkspaceRecord {
    let tabs = (0..count)
        .map(|position| BrowserTabRecord {
            id: mint_tab_id(),
            position,
            current_history_index: Some(0),
            manual_reopen_required: false,
            restoration_status: BrowserRestorationStatus::Restorable,
            history: vec![BrowserHistoryRecord {
                position: 0,
                url: format!("https://example.com/{position}"),
                recorded_at_ms: position as i64 + 1,
            }],
        })
        .collect::<Vec<_>>();
    BrowserWorkspaceRecord {
        session_id: session_id.into(),
        scope,
        layout_mode: BrowserLayoutMode::Split,
        split_width_px: 560,
        active_tab_id: tabs.first().map(|tab| tab.id.clone()),
        tabs,
        recovery: None,
    }
}

fn activation(record: &BrowserWorkspaceRecord, scope: SessionScope) -> TaskBrowserActivatePayload {
    TaskBrowserActivatePayload {
        identity: identity(scope, &record.session_id),
        tabs: record
            .tabs
            .iter()
            .map(|tab| TaskBrowserTabPayload {
                tab_id: tab.id.clone(),
                url: tab
                    .current_history_index
                    .map(|index| tab.history[index].url.clone()),
                manual_reopen_required: tab.manual_reopen_required,
            })
            .collect(),
        active_tab_id: record.active_tab_id.clone().unwrap(),
    }
}

#[test]
fn payloads_are_nested_strict_and_camel_case() {
    let payload: TaskBrowserSetGeometryPayload = serde_json::from_value(json!({
        "identity": { "scope": "local", "sessionId": "s123" },
        "host": { "x": 10.0, "y": 20.0, "width": 800.0, "height": 600.0, "scaleFactor": 2.0 }
    }))
    .unwrap();
    assert_eq!(payload.host.scale_factor, 2.0);
    assert!(serde_json::from_value::<TaskBrowserSetGeometryPayload>(json!({
        "identity": { "scope": "local", "sessionId": "s123" },
        "host": { "x": 10.0, "y": 20.0, "width": 800.0, "height": 600.0, "scaleFactor": 2.0, "root": "/tmp" }
    }))
    .is_err());
}

#[test]
fn activation_requires_a_real_scope_owned_session_and_exact_persisted_tabs() {
    let td = TempDir::new("activate");
    let app = state(&td.path);
    let session = sessions::create(&app.local_sessions_dir, None).unwrap();
    let record = workspace(&session.id, BrowserWorkspaceScope::Local, 2);
    replace_browser_workspace(
        &app.local_sessions_dir,
        &session.id,
        BrowserWorkspaceScope::Local,
        &record,
    )
    .unwrap();
    let runtime = BrowserRuntimeManager::new(RecordingPort::default());

    task_browser_activate_impl(
        activation(&record, SessionScope::Local),
        &app,
        &runtime,
        "main",
    )
    .unwrap();

    assert_eq!(runtime.port().added.lock().unwrap().len(), 2);
    assert_eq!(runtime.selected_identity().unwrap().session_id, session.id);

    let mut forged = activation(&record, SessionScope::Local);
    forged.tabs[0].url = Some("https://attacker.example/".into());
    assert!(matches!(
        task_browser_activate_impl(forged, &app, &runtime, "main"),
        Err(crate::error::IpcError::BadArgument(_))
    ));
}

#[test]
fn geometry_converts_css_pixels_with_the_reported_scale_and_rejects_stale_scope() {
    let td = TempDir::new("geometry");
    let app = state(&td.path);
    let session = sessions::create(&app.local_sessions_dir, None).unwrap();
    let record = workspace(&session.id, BrowserWorkspaceScope::Local, 1);
    replace_browser_workspace(
        &app.local_sessions_dir,
        &session.id,
        BrowserWorkspaceScope::Local,
        &record,
    )
    .unwrap();
    let runtime = BrowserRuntimeManager::new(RecordingPort::default());
    task_browser_activate_impl(
        activation(&record, SessionScope::Local),
        &app,
        &runtime,
        "main",
    )
    .unwrap();

    task_browser_set_geometry_impl(
        TaskBrowserSetGeometryPayload {
            identity: identity(SessionScope::Local, &session.id),
            host: BrowserHostRect {
                x: 10.0,
                y: 20.0,
                width: 800.0,
                height: 600.0,
                scale_factor: 2.0,
            },
        },
        &app,
        &runtime,
        "main",
    )
    .unwrap();

    assert_eq!(
        runtime.port().bounds.lock().unwrap()[0].1,
        BrowserBounds::new(20.0, 40.0, 1_600.0, 1_200.0).unwrap()
    );
    assert!(matches!(
        task_browser_select_tab_impl(
            TaskBrowserTabActionPayload {
                identity: identity(SessionScope::Project, &session.id),
                tab_id: record.tabs[0].id.clone(),
            },
            &app,
            &runtime,
            "main",
        ),
        Err(crate::error::IpcError::NeedsApproval)
    ));
}

#[test]
fn five_tab_cap_and_stale_tab_ids_fail_before_native_mutation() {
    let td = TempDir::new("tabs");
    let app = state(&td.path);
    let session = sessions::create(&app.local_sessions_dir, None).unwrap();
    let record = workspace(&session.id, BrowserWorkspaceScope::Local, 5);
    replace_browser_workspace(
        &app.local_sessions_dir,
        &session.id,
        BrowserWorkspaceScope::Local,
        &record,
    )
    .unwrap();
    let runtime = BrowserRuntimeManager::new(RecordingPort::default());
    task_browser_activate_impl(
        activation(&record, SessionScope::Local),
        &app,
        &runtime,
        "main",
    )
    .unwrap();

    assert!(matches!(
        task_browser_open_tab_impl(
            TaskBrowserOpenTabPayload {
                identity: identity(SessionScope::Local, &session.id),
                tab: TaskBrowserTabPayload {
                    tab_id: mint_tab_id(),
                    url: None,
                    manual_reopen_required: false,
                },
            },
            &app,
            &runtime,
            "main",
        ),
        Err(crate::error::IpcError::Blocked(_))
    ));
    assert!(matches!(
        task_browser_reload_impl(
            TaskBrowserTabActionPayload {
                identity: identity(SessionScope::Local, &session.id),
                tab_id: mint_tab_id(),
            },
            &app,
            &runtime,
            "main",
        ),
        Err(crate::error::IpcError::NotFound(_))
    ));
}

#[test]
fn unavailable_back_is_rejected_without_poisoning_the_next_page_navigation() {
    let td = TempDir::new("unavailable-back");
    let app = state(&td.path);
    let session = sessions::create(&app.local_sessions_dir, None).unwrap();
    let record = workspace(&session.id, BrowserWorkspaceScope::Local, 1);
    replace_browser_workspace(
        &app.local_sessions_dir,
        &session.id,
        BrowserWorkspaceScope::Local,
        &record,
    )
    .unwrap();
    let runtime = BrowserRuntimeManager::new(RecordingPort::default());
    task_browser_activate_impl(
        activation(&record, SessionScope::Local),
        &app,
        &runtime,
        "main",
    )
    .unwrap();
    let tab_id = record.tabs[0].id.clone();
    let initial_url = record.tabs[0].history[0].url.clone();
    let label = runtime.port().added.lock().unwrap()[0].label.clone();
    let initial = validate_browser_url(&initial_url).unwrap();
    assert!(runtime.admit_page_navigation(&label, &initial));
    runtime.navigation_finished(&label, &initial_url).unwrap();

    assert!(matches!(
        task_browser_back_impl(
            TaskBrowserTabActionPayload {
                identity: identity(SessionScope::Local, &session.id),
                tab_id: tab_id.clone(),
            },
            &app,
            &runtime,
            "main",
        ),
        Err(crate::error::IpcError::Blocked(reason)) if reason == "browser.historyUnavailable"
    ));
    assert!(runtime.port().evaluated.lock().unwrap().is_empty());

    let next = validate_browser_url("https://example.com/next").unwrap();
    assert!(runtime.admit_page_navigation(&label, &next));
    assert_eq!(
        runtime
            .navigation_finished(&label, next.url.as_str())
            .unwrap()
            .navigation,
        BrowserHistoryNavigation::New
    );
}

#[test]
fn deactivate_is_exact_identity_scoped_and_main_webview_only() {
    let td = TempDir::new("deactivate");
    let app = state(&td.path);
    let session = sessions::create(&app.local_sessions_dir, None).unwrap();
    let record = workspace(&session.id, BrowserWorkspaceScope::Local, 1);
    replace_browser_workspace(
        &app.local_sessions_dir,
        &session.id,
        BrowserWorkspaceScope::Local,
        &record,
    )
    .unwrap();
    let runtime = BrowserRuntimeManager::new(RecordingPort::default());
    task_browser_activate_impl(
        activation(&record, SessionScope::Local),
        &app,
        &runtime,
        "main",
    )
    .unwrap();

    assert!(matches!(
        task_browser_deactivate_impl(
            TaskBrowserIdentityPayload {
                identity: identity(SessionScope::Local, &session.id),
            },
            &app,
            &runtime,
            "task-browser-forged",
        ),
        Err(crate::error::IpcError::Blocked(_))
    ));
    task_browser_deactivate_impl(
        TaskBrowserIdentityPayload {
            identity: identity(SessionScope::Local, &session.id),
        },
        &app,
        &runtime,
        "main",
    )
    .unwrap();
    assert!(runtime.selected_identity().is_none());
}

#[test]
fn loopback_navigation_requires_project_scope_and_an_exact_origin_approval() {
    let td = TempDir::new("loopback");
    let app = state(&td.path);
    let local = sessions::create(&app.local_sessions_dir, None).unwrap();
    let local_record = workspace(&local.id, BrowserWorkspaceScope::Local, 1);
    replace_browser_workspace(
        &app.local_sessions_dir,
        &local.id,
        BrowserWorkspaceScope::Local,
        &local_record,
    )
    .unwrap();
    let runtime = BrowserRuntimeManager::new(RecordingPort::default());
    task_browser_activate_impl(
        activation(&local_record, SessionScope::Local),
        &app,
        &runtime,
        "main",
    )
    .unwrap();
    assert!(matches!(
        task_browser_navigate_impl(
            TaskBrowserNavigatePayload {
                identity: identity(SessionScope::Local, &local.id),
                tab_id: local_record.tabs[0].id.clone(),
                url: "http://localhost:3000/".into(),
                approved_loopback_origin: Some("http://localhost:3000".into()),
            },
            &app,
            &runtime,
            "main",
        ),
        Err(crate::error::IpcError::NeedsApproval)
    ));

    let project = td.path.join("project");
    fs::create_dir_all(&project).unwrap();
    let project = fs::canonicalize(project).unwrap();
    app.session.open(project.clone());
    app.trust.lock().unwrap().mark_trusted(&project).unwrap();
    let project_dir = sessions::project_sessions_dir(&project).unwrap();
    let project_session = sessions::create(&project_dir, None).unwrap();
    let project_record = workspace(&project_session.id, BrowserWorkspaceScope::Project, 1);
    replace_browser_workspace(
        &project_dir,
        &project_session.id,
        BrowserWorkspaceScope::Project,
        &project_record,
    )
    .unwrap();
    task_browser_activate_impl(
        activation(&project_record, SessionScope::Project),
        &app,
        &runtime,
        "main",
    )
    .unwrap();

    assert!(matches!(
        task_browser_navigate_impl(
            TaskBrowserNavigatePayload {
                identity: identity(SessionScope::Project, &project_session.id),
                tab_id: project_record.tabs[0].id.clone(),
                url: "http://localhost:3000/path".into(),
                approved_loopback_origin: Some("http://localhost:4000".into()),
            },
            &app,
            &runtime,
            "main",
        ),
        Err(crate::error::IpcError::NeedsApproval)
    ));
    task_browser_navigate_impl(
        TaskBrowserNavigatePayload {
            identity: identity(SessionScope::Project, &project_session.id),
            tab_id: project_record.tabs[0].id.clone(),
            url: "http://localhost:3000/path".into(),
            approved_loopback_origin: Some("http://localhost:3000".into()),
        },
        &app,
        &runtime,
        "main",
    )
    .unwrap();
    assert_eq!(
        runtime.port().navigation.lock().unwrap().as_slice(),
        ["http://localhost:3000/path"]
    );
}

#[test]
fn project_capture_owner_is_snapshotted_and_rejects_a_project_switch() {
    let td = TempDir::new("capture-project-switch");
    let app = state(&td.path);
    let first = fs::canonicalize({
        let path = td.path.join("first-project");
        fs::create_dir_all(&path).unwrap();
        path
    })
    .unwrap();
    app.session.open(first.clone());
    app.trust.lock().unwrap().mark_trusted(&first).unwrap();
    let owner = capture_project_owner(SessionScope::Project, &app)
        .unwrap()
        .unwrap();

    let second = fs::canonicalize({
        let path = td.path.join("second-project");
        fs::create_dir_all(&path).unwrap();
        path
    })
    .unwrap();
    app.session.open(second.clone());
    app.trust.lock().unwrap().mark_trusted(&second).unwrap();

    assert!(matches!(
        require_capture_project_current(&owner, &app),
        Err(crate::error::IpcError::Blocked(reason))
            if reason == "browser.captureProjectChanged"
    ));
    assert_eq!(owner.root, first);
}
