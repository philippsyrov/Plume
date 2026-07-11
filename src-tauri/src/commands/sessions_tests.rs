//! Tests for the sessions command layer (D63A). The Tauri handlers are
//! thin; what needs proving here is the scope gate (`scope_dir`), the
//! store-error mapping, payload strictness, and the serialized wire
//! shapes. Storage behavior itself is covered in `sessions::tests`.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde_json::json;

use super::*;
use crate::chat::stream::ChatStreamRegistry;
use crate::project::trust::TrustStore;
use crate::project::ProjectSession;
use crate::sessions::TranscriptEntry;

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(label: &str) -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock before epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "plume-sessions-cmd-test-{}-{}-{}",
            label,
            std::process::id(),
            nanos
        ));
        fs::create_dir_all(&path).expect("create tempdir");
        Self { path }
    }
    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn test_state(base: &Path) -> AppState {
    AppState {
        session: ProjectSession::default(),
        trust: Mutex::new(TrustStore::load(base.join("trusted-projects.json"))),
        chat_streams: Arc::new(ChatStreamRegistry::default()),
        agent_config: Mutex::new(crate::agent::AgentConfig::default()),
        local_sessions_dir: base.join("app-data").join("sessions"),
    }
}

// ---------------------------------------------------------------
// Scope gate
// ---------------------------------------------------------------

#[test]
fn local_scope_resolves_to_app_data_even_with_a_trusted_project_open() {
    let td = TempDir::new("local-vs-project");
    let project = td.path().join("project");
    fs::create_dir_all(&project).unwrap();
    let project = fs::canonicalize(&project).unwrap();

    let state = test_state(td.path());
    state.session.open(project.clone());
    state.trust.lock().unwrap().mark_trusted(&project).unwrap();

    let dir = scope_dir(SessionScope::Local, &state).unwrap();
    assert_eq!(dir, state.local_sessions_dir);
    assert!(
        !dir.starts_with(&project),
        "local sessions must never resolve into a project"
    );
}

#[test]
fn project_scope_requires_an_open_trusted_project() {
    let td = TempDir::new("gate");
    let state = test_state(td.path());

    // No project open at all.
    assert!(matches!(
        scope_dir(SessionScope::Project, &state),
        Err(IpcError::NeedsApproval)
    ));

    // Open but not trusted.
    let project = td.path().join("project");
    fs::create_dir_all(&project).unwrap();
    let project = fs::canonicalize(&project).unwrap();
    state.session.open(project.clone());
    assert!(matches!(
        scope_dir(SessionScope::Project, &state),
        Err(IpcError::NeedsApproval)
    ));

    // Trusted: resolves inside the project's .plume.
    state.trust.lock().unwrap().mark_trusted(&project).unwrap();
    let dir = scope_dir(SessionScope::Project, &state).unwrap();
    assert_eq!(dir, project.join(".plume").join("sessions"));
}

// ---------------------------------------------------------------
// Error mapping
// ---------------------------------------------------------------

#[test]
fn store_errors_map_onto_the_ipc_error_model() {
    let x = || "x".to_string();
    assert!(matches!(
        map_store_err(SessionStoreError::NotFound(x())),
        IpcError::NotFound(_)
    ));
    assert!(matches!(
        map_store_err(SessionStoreError::Invalid(x())),
        IpcError::BadArgument(_)
    ));
    assert!(matches!(
        map_store_err(SessionStoreError::Limit(x())),
        IpcError::Blocked(_)
    ));
    assert!(matches!(
        map_store_err(SessionStoreError::Refused(x())),
        IpcError::Blocked(_)
    ));
    assert!(matches!(
        map_store_err(SessionStoreError::Corrupt(x())),
        IpcError::Internal(_)
    ));
    assert!(matches!(
        map_store_err(SessionStoreError::Storage(x())),
        IpcError::Internal(_)
    ));
}

// ---------------------------------------------------------------
// Payload strictness
// ---------------------------------------------------------------

#[test]
fn payloads_parse_camel_case() {
    let p: SessionsListPayload =
        serde_json::from_value(json!({ "scope": "local", "includeArchived": true })).unwrap();
    assert_eq!(p.scope, SessionScope::Local);
    assert_eq!(p.include_archived, Some(true));

    let p: SessionsSaveTranscriptPayload = serde_json::from_value(json!({
        "scope": "project",
        "sessionId": "s123",
        "entries": [{ "kind": "error", "message": "boom" }]
    }))
    .unwrap();
    assert_eq!(p.scope, SessionScope::Project);
    assert_eq!(p.session_id, "s123");
    assert_eq!(p.entries.len(), 1);
}

#[test]
fn payloads_reject_unknown_fields_and_scopes() {
    // "No command accepts a filesystem root" is a payload-shape fact:
    // a smuggled root is an unknown field, rejected at the boundary.
    assert!(serde_json::from_value::<SessionsListPayload>(
        json!({ "scope": "local", "root": "/etc" })
    )
    .is_err());
    assert!(serde_json::from_value::<SessionsListPayload>(json!({ "scope": "global" })).is_err());
}

// ---------------------------------------------------------------
// Wire-shape pins
// ---------------------------------------------------------------

#[test]
fn summary_response_serializes_camel_case_with_null_archived() {
    let resp = SessionSummaryResponse {
        session: crate::sessions::SessionSummary {
            id: "s1".into(),
            title: "New chat".into(),
            created_at_ms: 1,
            updated_at_ms: 2,
            archived_at_ms: None,
        },
    };
    assert_eq!(
        serde_json::to_value(&resp).unwrap(),
        json!({ "session": {
            "id": "s1",
            "title": "New chat",
            "createdAtMs": 1,
            "updatedAtMs": 2,
            "archivedAtMs": null
        }})
    );
}

#[test]
fn delete_response_is_ok_true() {
    assert_eq!(
        serde_json::to_value(SessionsDeleteResponse { ok: true }).unwrap(),
        json!({ "ok": true })
    );
}

/// Pins the transcript-entry wire form to the frontend's visible
/// `ChatEntry` shape (minus `streaming`): tag values, camelCase field
/// names, nested `message`, object line range, and the bounded stats.
#[test]
fn transcript_entries_serialize_in_visible_chat_shape() {
    let message: TranscriptEntry = serde_json::from_value(json!({
        "kind": "message",
        "message": { "role": "user", "content": "explain this" },
        "durationMs": 5,
        "attachmentRelPath": "src/greet.py",
        "attachmentLineRange": { "startLine": 3, "endLine": 7 },
        "stats": {
            "outputTokens": 42, "evalMs": 900, "tokensPerSecond": 46.5,
            "promptTokens": 101, "promptMs": null
        },
        "sentInMode": "proposeDiff"
    }))
    .unwrap();
    let round = serde_json::to_value(&message).unwrap();
    assert_eq!(round["kind"], "message");
    assert_eq!(round["message"]["role"], "user");
    assert_eq!(round["attachmentLineRange"]["startLine"], 3);
    assert_eq!(round["stats"]["tokensPerSecond"], 46.5);
    assert_eq!(round["sentInMode"], "proposeDiff");

    let cancelled = serde_json::to_value(TranscriptEntry::Cancelled {
        partial: "half".into(),
        model_used: None,
        duration_ms: None,
    })
    .unwrap();
    assert_eq!(cancelled, json!({ "kind": "cancelled", "partial": "half" }));

    let error = serde_json::to_value(TranscriptEntry::Error {
        message: "boom".into(),
    })
    .unwrap();
    assert_eq!(error, json!({ "kind": "error", "message": "boom" }));
}

// ---------------------------------------------------------------
// D66: search payload + wire shape
// ---------------------------------------------------------------

#[test]
fn search_payload_parses_and_rejects_unknown_fields() {
    let p: SessionsSearchPayload = serde_json::from_value(json!({
        "scope": "local",
        "query": "borrow checker",
        "limit": 5
    }))
    .unwrap();
    assert_eq!(p.scope, SessionScope::Local);
    assert_eq!(p.query, "borrow checker");
    assert_eq!(p.limit, Some(5));

    // limit optional
    let p: SessionsSearchPayload =
        serde_json::from_value(json!({ "scope": "project", "query": "x" })).unwrap();
    assert_eq!(p.limit, None);

    // no smuggled roots, no unknown fields
    assert!(serde_json::from_value::<SessionsSearchPayload>(
        json!({ "scope": "local", "query": "x", "root": "/etc" })
    )
    .is_err());
}

#[test]
fn search_response_serializes_camel_case_hits() {
    let resp = SessionsSearchResponse {
        hits: vec![crate::sessions::SearchHit {
            id: "s1".into(),
            title: "gradient descent notes".into(),
            updated_at_ms: 7,
            archived_at_ms: None,
            match_kind: crate::sessions::search::SearchMatchKind::Title,
            snippet: None,
        }],
    };
    assert_eq!(
        serde_json::to_value(&resp).unwrap(),
        json!({ "hits": [{
            "id": "s1",
            "title": "gradient descent notes",
            "updatedAtMs": 7,
            "archivedAtMs": null,
            "matchKind": "title",
            "snippet": null
        }]})
    );
}

#[test]
fn search_goes_through_the_same_scope_gate() {
    // The handler resolves the directory through `scope_dir`; with no
    // trusted project open, project-scope search must be NeedsApproval
    // before any store code runs. (Direct gate check — the handler is
    // a thin async wrapper over exactly this call.)
    let td = TempDir::new("search-gate");
    let state = test_state(td.path());
    assert!(matches!(
        scope_dir(SessionScope::Project, &state),
        Err(IpcError::NeedsApproval)
    ));

    // And the store itself works against the resolved local dir.
    let dir = scope_dir(SessionScope::Local, &state).unwrap();
    crate::sessions::create(&dir, Some("findable title")).unwrap();
    let hits = crate::sessions::search(&dir, "findable", None).unwrap();
    assert_eq!(hits.len(), 1);
}
