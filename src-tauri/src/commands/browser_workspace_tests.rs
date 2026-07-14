use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde_json::json;

use super::*;
use crate::chat::stream::ChatStreamRegistry;
use crate::commands::project::AppState;
use crate::commands::sessions::SessionScope;
use crate::project::trust::TrustStore;
use crate::project::ProjectSession;
use crate::sessions;
use crate::sessions::browser_workspace::{
    mint_tab_id, BrowserHistoryRecord, BrowserLayoutMode, BrowserRestorationStatus,
    BrowserTabRecord, BrowserWorkspaceRecord, BrowserWorkspaceScope,
};

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
            "plume-browser-workspace-command-{label}-{}-{nonce}",
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

fn workspace(session_id: &str, scope: BrowserWorkspaceScope) -> BrowserWorkspaceRecord {
    let tab_id = mint_tab_id();
    BrowserWorkspaceRecord {
        session_id: session_id.into(),
        scope,
        layout_mode: BrowserLayoutMode::Split,
        split_width_px: 560,
        active_tab_id: Some(tab_id.clone()),
        tabs: vec![BrowserTabRecord {
            id: tab_id,
            position: 0,
            current_history_index: Some(0),
            manual_reopen_required: false,
            restoration_status: BrowserRestorationStatus::Restorable,
            history: vec![BrowserHistoryRecord {
                position: 0,
                url: "https://example.com/".into(),
                recorded_at_ms: 1,
            }],
        }],
        recovery: None,
    }
}

#[test]
fn payloads_are_nested_strict_and_camel_case() {
    let load: BrowserWorkspaceLoadPayload = serde_json::from_value(json!({
        "identity": { "scope": "local", "sessionId": "s123" }
    }))
    .unwrap();
    assert_eq!(load.identity.scope, SessionScope::Local);
    assert_eq!(load.identity.session_id, "s123");
    assert!(
        serde_json::from_value::<BrowserWorkspaceLoadPayload>(json!({
            "identity": { "scope": "local", "sessionId": "s123", "root": "/tmp" }
        }))
        .is_err()
    );
    assert!(
        serde_json::from_value::<BrowserWorkspaceLoadPayload>(json!({
            "identity": { "scope": "elsewhere", "sessionId": "s123" }
        }))
        .is_err()
    );
}

#[test]
fn caller_allowlist_accepts_only_the_main_webview() {
    assert!(require_main_webview("main").is_ok());
    for label in ["browser-sandbox", "task-browser-1", "remote"] {
        assert!(require_main_webview(label).is_err(), "{label}");
    }
}

#[test]
fn local_load_save_reset_and_recovery_wire_shapes_are_exact() {
    let td = TempDir::new("local");
    let state = state(&td.path);
    let session = sessions::create(&state.local_sessions_dir, None).unwrap();
    let id = identity(SessionScope::Local, &session.id);
    let missing = browser_workspace_load_impl(
        BrowserWorkspaceLoadPayload {
            identity: id.clone(),
        },
        &state,
        "main",
    )
    .unwrap();
    assert_eq!(
        serde_json::to_value(missing).unwrap(),
        json!({ "workspace": null, "recoveryNotice": null })
    );

    let record = workspace(&session.id, BrowserWorkspaceScope::Local);
    let saved = browser_workspace_save_impl(
        BrowserWorkspaceSavePayload {
            identity: id.clone(),
            workspace: record.clone(),
        },
        &state,
        "main",
    )
    .unwrap();
    let value = serde_json::to_value(saved).unwrap();
    assert_eq!(value["workspace"]["sessionId"], session.id);
    assert_eq!(value["workspace"]["scope"], "local");
    assert_eq!(value["workspace"]["tabs"][0]["currentHistoryIndex"], 0);

    let reset = browser_workspace_reset_impl(
        BrowserWorkspaceResetPayload { identity: id },
        &state,
        "main",
    )
    .unwrap();
    let value = serde_json::to_value(reset).unwrap();
    assert_eq!(value["workspace"]["tabs"].as_array().unwrap().len(), 1);
    assert_eq!(value["workspace"]["tabs"][0]["restorationStatus"], "blank");
    assert_eq!(
        value["workspace"]["tabs"][0]["currentHistoryIndex"],
        json!(null)
    );
}

#[test]
fn scope_mismatch_is_not_found_and_project_requires_current_trust() {
    let td = TempDir::new("scope");
    let state = state(&td.path);
    let local = sessions::create(&state.local_sessions_dir, None).unwrap();
    assert!(matches!(
        browser_workspace_load_impl(
            BrowserWorkspaceLoadPayload {
                identity: identity(SessionScope::Project, &local.id)
            },
            &state,
            "main"
        ),
        Err(crate::error::IpcError::NeedsApproval)
    ));

    let project = td.path.join("project");
    fs::create_dir_all(&project).unwrap();
    let project = fs::canonicalize(project).unwrap();
    state.session.open(project.clone());
    state.trust.lock().unwrap().mark_trusted(&project).unwrap();
    assert!(matches!(
        browser_workspace_load_impl(
            BrowserWorkspaceLoadPayload {
                identity: identity(SessionScope::Project, &local.id)
            },
            &state,
            "main"
        ),
        Err(crate::error::IpcError::NotFound(_))
    ));
}
