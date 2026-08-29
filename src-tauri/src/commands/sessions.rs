//! D63A: `sessions.*` command handlers — durable chat sessions.
//!
//! Eight verbs: `sessions.list`, `sessions.create`, `sessions.load`,
//! `sessions.rename`, `sessions.archive`, `sessions.delete`,
//! `sessions.saveTranscript`, and `sessions.search` (D66). All storage
//! behavior lives in `crate::sessions`; this file only resolves
//! *which* database a request may touch and maps store errors onto
//! the IPC error model.
//!
//! Scope resolution is the security boundary:
//!
//! * `scope: 'local'` → the app-data sessions directory resolved once
//!   at startup (`AppState::local_sessions_dir`). Available without a
//!   project; never touches project state.
//! * `scope: 'project'` → the currently open **trusted** project's
//!   `.plume/sessions`. No open project, or an untrusted one, is
//!   `NeedsApproval` — the same gate the memory and patch verbs use.
//!
//! No command accepts a filesystem root, and the frontend never sees a
//! database path. Distinct from the D77 `session.*` (singular) family,
//! which is window-scoped agent-autonomy config and touches no disk.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::browser::local_evidence::LocalEvidenceError;
use crate::browser::runtime::BrowserRuntimeIdentity;
use crate::commands::project::{AppState, EmptyPayload};
use crate::commands::task_browser::LiveBrowserRuntime;
use crate::error::{IpcError, IpcRequest};
use crate::project::OpenProject;
use crate::prompts::ContextSourceRef;
use crate::research::bundle::{
    delete_local_session_with_artifacts, delete_project_session_with_artifacts,
    ArtifactDeleteError, ArtifactStoreError,
};
use crate::research::run_registry::{local_owner_key, project_owner_key};
use crate::sessions::owner::SessionOwnerError;
use crate::sessions::{self, SearchHit, SessionRecord, SessionStoreError, SessionSummary};

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SessionScope {
    Local,
    Project,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SessionsListPayload {
    pub scope: SessionScope,
    /// Archived sessions are hidden unless this is `true`.
    pub include_archived: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SessionsCreatePayload {
    pub scope: SessionScope,
    /// Omitted → the backend's default title.
    pub title: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SessionsLoadPayload {
    pub scope: SessionScope,
    pub session_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SessionsForkPayload {
    pub scope: SessionScope,
    pub session_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SessionsRollbackPayload {
    pub scope: SessionScope,
    pub session_id: String,
    pub turn_count: u32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SessionsRenamePayload {
    pub scope: SessionScope,
    pub session_id: String,
    pub title: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SessionsArchivePayload {
    pub scope: SessionScope,
    pub session_id: String,
    /// `true` archives, `false` unarchives.
    pub archived: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SessionsDeletePayload {
    pub scope: SessionScope,
    pub session_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SessionsSaveTranscriptPayload {
    pub scope: SessionScope,
    pub session_id: String,
    /// Raw entry values, parsed and validated in `crate::sessions` so
    /// a malformed entry surfaces as a typed `BadArgument` naming the
    /// entry index instead of an opaque deserialization failure.
    pub entries: Vec<serde_json::Value>,
    #[serde(default)]
    pub context_sources: Vec<ContextSourceRef>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SessionsSearchPayload {
    pub scope: SessionScope,
    /// Literal search text — FTS5 operators in it are searched for,
    /// never interpreted (escaped at the store boundary).
    pub query: String,
    /// Optional result cap; must be 1..=20 when present, default 20.
    pub limit: Option<u32>,
}

#[derive(Debug, Serialize)]
pub struct SessionsListResponse {
    pub sessions: Vec<SessionSummary>,
}

#[derive(Debug, Serialize)]
pub struct SessionsSearchResponse {
    pub hits: Vec<SearchHit>,
}

#[derive(Debug, Serialize)]
pub struct SessionSummaryResponse {
    pub session: SessionSummary,
}

#[derive(Debug, Serialize)]
pub struct SessionRecordResponse {
    pub session: SessionRecord,
}

#[derive(Debug, Serialize)]
pub struct SessionsDeleteResponse {
    pub ok: bool,
}

#[tauri::command]
pub async fn sessions_list(
    req: IpcRequest<SessionsListPayload>,
    state: State<'_, AppState>,
) -> Result<SessionsListResponse, IpcError> {
    req.check_version()?;
    let payload = req.payload;
    let dir = scope_dir(payload.scope, &state)?;
    let sessions =
        sessions::list(&dir, payload.include_archived.unwrap_or(false)).map_err(map_store_err)?;
    Ok(SessionsListResponse { sessions })
}

#[tauri::command]
pub async fn sessions_create(
    req: IpcRequest<SessionsCreatePayload>,
    state: State<'_, AppState>,
) -> Result<SessionSummaryResponse, IpcError> {
    req.check_version()?;
    let payload = req.payload;
    let dir = scope_dir(payload.scope, &state)?;
    let session = sessions::create(&dir, payload.title.as_deref()).map_err(map_store_err)?;
    Ok(SessionSummaryResponse { session })
}

/// Resolve the one app-private Home conversation, creating it on first launch.
///
/// Takes no session id on purpose. Home's identity is backend-owned: a
/// caller-supplied Home id would let the frontend choose which conversation is
/// Home, which is the same mistake as a caller-supplied filesystem root. The
/// frontend learns the id here, every launch, and never stores it.
///
/// Local scope only — Home lives in app-private storage and never in a project.
#[tauri::command]
pub async fn sessions_home(
    req: IpcRequest<EmptyPayload>,
    state: State<'_, AppState>,
) -> Result<SessionSummaryResponse, IpcError> {
    req.check_version()?;
    let session = sessions::home(&state.local_sessions_dir).map_err(map_store_err)?;
    Ok(SessionSummaryResponse { session })
}

#[tauri::command]
pub async fn sessions_load(
    req: IpcRequest<SessionsLoadPayload>,
    state: State<'_, AppState>,
) -> Result<SessionRecordResponse, IpcError> {
    req.check_version()?;
    let payload = req.payload;
    let dir = scope_dir(payload.scope, &state)?;
    let session = sessions::load_for_scope(
        &dir,
        &payload.session_id,
        payload.scope == SessionScope::Project,
    )
    .map_err(map_store_err)?;
    Ok(SessionRecordResponse { session })
}

#[tauri::command]
pub async fn sessions_fork(
    req: IpcRequest<SessionsForkPayload>,
    state: State<'_, AppState>,
) -> Result<SessionRecordResponse, IpcError> {
    req.check_version()?;
    sessions_fork_impl(req.payload, &state)
}

fn sessions_fork_impl(
    payload: SessionsForkPayload,
    state: &AppState,
) -> Result<SessionRecordResponse, IpcError> {
    let dir = scope_dir(payload.scope, state)?;
    let allow_attachments = payload.scope == SessionScope::Project;
    let session =
        sessions::fork(&dir, &payload.session_id, allow_attachments).map_err(map_store_err)?;
    Ok(SessionRecordResponse { session })
}

#[tauri::command]
pub async fn sessions_rollback(
    req: IpcRequest<SessionsRollbackPayload>,
    state: State<'_, AppState>,
) -> Result<SessionRecordResponse, IpcError> {
    req.check_version()?;
    sessions_rollback_impl(req.payload, &state)
}

fn sessions_rollback_impl(
    payload: SessionsRollbackPayload,
    state: &AppState,
) -> Result<SessionRecordResponse, IpcError> {
    let dir = scope_dir(payload.scope, state)?;
    let allow_attachments = payload.scope == SessionScope::Project;
    let session = sessions::rollback(
        &dir,
        &payload.session_id,
        payload.turn_count,
        allow_attachments,
    )
    .map_err(map_store_err)?;
    Ok(SessionRecordResponse { session })
}

#[tauri::command]
pub async fn sessions_rename(
    req: IpcRequest<SessionsRenamePayload>,
    state: State<'_, AppState>,
) -> Result<SessionSummaryResponse, IpcError> {
    req.check_version()?;
    let payload = req.payload;
    let dir = scope_dir(payload.scope, &state)?;
    let session =
        sessions::rename(&dir, &payload.session_id, &payload.title).map_err(map_store_err)?;
    Ok(SessionSummaryResponse { session })
}

#[tauri::command]
pub async fn sessions_archive(
    req: IpcRequest<SessionsArchivePayload>,
    state: State<'_, AppState>,
) -> Result<SessionSummaryResponse, IpcError> {
    req.check_version()?;
    let payload = req.payload;
    let dir = scope_dir(payload.scope, &state)?;
    let session = sessions::set_archived(&dir, &payload.session_id, payload.archived)
        .map_err(map_store_err)?;
    Ok(SessionSummaryResponse { session })
}

#[tauri::command]
pub async fn sessions_delete(
    req: IpcRequest<SessionsDeletePayload>,
    state: State<'_, AppState>,
    browser_runtime: State<'_, LiveBrowserRuntime>,
) -> Result<SessionsDeleteResponse, IpcError> {
    req.check_version()?;
    let runtime_identity = BrowserRuntimeIdentity {
        scope: match req.payload.scope {
            SessionScope::Local => crate::sessions::browser_workspace::BrowserWorkspaceScope::Local,
            SessionScope::Project => {
                crate::sessions::browser_workspace::BrowserWorkspaceScope::Project
            }
        },
        session_id: req.payload.session_id.clone(),
    };
    browser_runtime
        .deactivate_if_selected(&runtime_identity)
        .map_err(|error| IpcError::Internal(error.to_string()))?;
    sessions_delete_impl(req.payload, &state)
}

fn sessions_delete_impl(
    payload: SessionsDeletePayload,
    state: &AppState,
) -> Result<SessionsDeleteResponse, IpcError> {
    match payload.scope {
        SessionScope::Local => {
            state
                .research_runs
                .cancel_owner(&local_owner_key(&payload.session_id));
            delete_local_session_with_artifacts(&state.local_sessions_dir, &payload.session_id)
                .map_err(map_artifact_delete_err)?;
        }
        SessionScope::Project => {
            let project = trusted_open(state).ok_or(IpcError::NeedsApproval)?;
            state
                .research_runs
                .cancel_owner(&project_owner_key(&project.id, &payload.session_id));
            let dir = sessions::project_sessions_dir(&project.root).map_err(map_store_err)?;
            delete_project_session_with_artifacts(&project, &dir, &payload.session_id)
                .map_err(map_artifact_delete_err)?;
        }
    }
    Ok(SessionsDeleteResponse { ok: true })
}

#[tauri::command]
pub async fn sessions_save_transcript(
    req: IpcRequest<SessionsSaveTranscriptPayload>,
    state: State<'_, AppState>,
) -> Result<SessionSummaryResponse, IpcError> {
    req.check_version()?;
    let payload = req.payload;
    let dir = scope_dir(payload.scope, &state)?;
    let entries = sessions::parse_entries(&payload.entries).map_err(map_store_err)?;
    // The scope rule, not a convenience: only project sessions may
    // carry project-file attachment metadata.
    let allow_attachments = payload.scope == SessionScope::Project;
    let session = sessions::save_transcript_with_context(
        &dir,
        &payload.session_id,
        &entries,
        &payload.context_sources,
        allow_attachments,
    )
    .map_err(map_store_err)?;
    Ok(SessionSummaryResponse { session })
}

#[tauri::command]
pub async fn sessions_search(
    req: IpcRequest<SessionsSearchPayload>,
    state: State<'_, AppState>,
) -> Result<SessionsSearchResponse, IpcError> {
    req.check_version()?;
    let payload = req.payload;
    let dir = scope_dir(payload.scope, &state)?;
    let hits = sessions::search(&dir, &payload.query, payload.limit.map(|n| n as usize))
        .map_err(map_store_err)?;
    Ok(SessionsSearchResponse { hits })
}

/// Map `scope` onto the one directory this request may touch. Kept as a
/// plain function over `AppState` (not Tauri `State`) so the gate is
/// directly testable.
pub(crate) fn scope_dir(scope: SessionScope, state: &AppState) -> Result<PathBuf, IpcError> {
    match scope {
        SessionScope::Local => Ok(state.local_sessions_dir.clone()),
        SessionScope::Project => {
            let open = trusted_open(state).ok_or(IpcError::NeedsApproval)?;
            sessions::project_sessions_dir(&open.root).map_err(map_store_err)
        }
    }
}

/// Same trust gate as the memory and patch verbs: an open project that
/// the user has not trusted resolves to `None`.
fn trusted_open(state: &AppState) -> Option<OpenProject> {
    let open = state.session.current()?;
    let trusted = {
        let store = state.trust.lock().expect("trust mutex poisoned");
        store.is_trusted(&open.root)
    };
    if trusted {
        Some(open)
    } else {
        None
    }
}

pub(super) fn map_store_err(err: SessionStoreError) -> IpcError {
    match err {
        SessionStoreError::NotFound(id) => IpcError::NotFound(format!("session {id}")),
        SessionStoreError::Invalid(msg) => IpcError::BadArgument(msg),
        SessionStoreError::Limit(msg) | SessionStoreError::Refused(msg) => IpcError::Blocked(msg),
        SessionStoreError::Corrupt(msg) | SessionStoreError::Storage(msg) => {
            IpcError::Internal(msg)
        }
    }
}

fn map_local_evidence_err(error: LocalEvidenceError) -> IpcError {
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

fn map_artifact_store_err(error: ArtifactStoreError) -> IpcError {
    match error {
        ArtifactStoreError::NotFound => IpcError::NotFound("research artifact".into()),
        ArtifactStoreError::Refused(message) | ArtifactStoreError::Limit(message) => {
            IpcError::Blocked(message)
        }
        ArtifactStoreError::Corrupt(message) | ArtifactStoreError::Storage(message) => {
            IpcError::Internal(message)
        }
    }
}

fn map_owner_err(error: SessionOwnerError) -> IpcError {
    match error {
        SessionOwnerError::ScopeMismatch => IpcError::BadArgument("session owner scope".into()),
        SessionOwnerError::ProjectUnavailable => IpcError::NeedsApproval,
        SessionOwnerError::NotFound => IpcError::NotFound("session owner".into()),
        SessionOwnerError::Store(error) => map_store_err(error),
    }
}

fn map_artifact_delete_err(error: ArtifactDeleteError) -> IpcError {
    match error {
        ArtifactDeleteError::Artifact(error) => map_artifact_store_err(error),
        ArtifactDeleteError::LocalEvidence(error) => map_local_evidence_err(error),
        ArtifactDeleteError::Session(error) => map_store_err(error),
        ArtifactDeleteError::Owner(error) => map_owner_err(error),
    }
}

#[cfg(test)]
#[path = "sessions_tests.rs"]
mod sessions_tests;
