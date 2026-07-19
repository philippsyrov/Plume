//! Thin IPC boundary for session-owned Stage A research notes.

use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::agent::protocol::ProviderFraming;
use crate::commands::project::AppState;
use crate::error::{IpcError, IpcRequest};
use crate::project::OpenProject;
use crate::prompts::ContextSourceRef;
use crate::providers::apple_foundation::{platform_supports_apple_models, NativeHelperPort};
use crate::providers::catalog::QWEN_CATALOG_ID;
use crate::research::bundle::{
    ArtifactBundleRecord, ArtifactCitationStatus, ArtifactOutcome, ArtifactStore,
    ArtifactStoreError,
};
use crate::research::evidence::{
    resolve_browser_evidence, ResearchEvidenceError, ResearchEvidenceSource,
};
use crate::research::markdown::{project_markdown, project_markdown_for_review};
use crate::research::model::{
    select_model, AppleResearchModel, ResearchModelSelectionError, SelectedResearchModel,
};
use crate::research::run::{run_research, ResearchRunRequest};
use crate::research::run_registry::{local_owner_key, project_owner_key, ResearchRunRegistry};
use crate::sessions;
use crate::sessions::owner::{
    resolve_session_owner, ResolvedSessionOwner, SessionOwnerError, SessionOwnerRef,
    SessionOwnerScope,
};

pub const RESEARCH_EVENT_CHANNEL: &str = "research/event";
const RESEARCH_RUN_DEADLINE: Duration = Duration::from_secs(5 * 60);
const MAX_RESEARCH_QUESTION_BYTES: usize = 8 * 1024;

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ResearchOwnerScope {
    Local,
    Project,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResearchOwnerPayload {
    pub scope: ResearchOwnerScope,
    pub session_id: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResearchStartPayload {
    pub run_id: String,
    pub owner: ResearchOwnerPayload,
    pub question: String,
    pub provider_id: String,
    pub model_id: String,
    pub handle_id: Option<String>,
    pub sources: Vec<ContextSourceRef>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResearchCancelPayload {
    pub run_id: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResearchListArtifactsPayload {
    pub owner: ResearchOwnerPayload,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResearchLoadArtifactPayload {
    pub owner: ResearchOwnerPayload,
    pub artifact_id: String,
    pub version: Option<u32>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResearchStartedResponse {
    pub run_id: String,
    pub provider_id: String,
    pub model_id: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResearchCancelResponse {
    pub cancelled: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResearchArtifactSummary {
    pub artifact_id: String,
    pub version: u32,
    pub created_at_ms: u64,
    pub question: String,
    pub provider_id: String,
    pub model_id: String,
    pub citation_status: ArtifactCitationStatus,
    pub outcome: ArtifactOutcome,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResearchListArtifactsResponse {
    pub artifacts: Vec<ResearchArtifactSummary>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResearchSourceView {
    pub source_id: String,
    pub evidence_id: String,
    pub source_url: String,
    pub title: Option<String>,
    pub captured_at_ms: u64,
    pub sha256: String,
    pub bytes: u64,
    pub redaction_count: u64,
    pub truncated: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResearchLoadArtifactResponse {
    pub artifact: ResearchArtifactSummary,
    pub markdown: String,
    pub sources: Vec<ResearchSourceView>,
    pub logical_turns: u32,
    pub provider_calls: u32,
    pub duration_ms: u64,
}

#[derive(Debug)]
struct PreparedResearch {
    owner: ResolvedSessionOwner,
    sources: Vec<ResearchEvidenceSource>,
    store: ArtifactStore,
    framing: ProviderFraming,
}

#[tauri::command]
pub async fn research_start(
    req: IpcRequest<ResearchStartPayload>,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<ResearchStartedResponse, IpcError> {
    req.check_version()?;
    let payload = req.payload;
    let prepared = prepare_research(&payload, &state)?;
    let owner_key = owner_key(&prepared.owner);
    let lease = state
        .research_runs
        .register(&payload.run_id, &owner_key)
        .map_err(|error| IpcError::BadArgument(error.to_string()))?;
    let launch = prepare_model_launch(&payload, &app)?;
    let project_session = state.session.clone();
    let owner = prepared.owner.clone();
    let request = ResearchRunRequest {
        run_id: payload.run_id.clone(),
        question: payload.question.clone(),
        provider_id: payload.provider_id.clone(),
        model_id: payload.model_id.clone(),
        runtime_id: payload
            .handle_id
            .clone()
            .unwrap_or_else(|| "apple-system".into()),
        framing: prepared.framing,
        sources: prepared.sources,
    };
    let app_for_task = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let cancel = lease.cancel_flag();
        let current = || owner_is_current(&owner, &project_session);
        let mut emit = |event| {
            if let Err(error) = app_for_task.emit(RESEARCH_EVENT_CHANNEL, event) {
                tracing::warn!(error = %error, "failed to emit research event");
            }
        };
        match launch {
            ModelLaunch::Apple(helper) => {
                let model = AppleResearchModel::new(&helper, true);
                run_research(
                    request,
                    &model,
                    &prepared.store,
                    cancel,
                    Instant::now() + RESEARCH_RUN_DEADLINE,
                    &current,
                    &mut emit,
                );
            }
            ModelLaunch::Qwen(model) => {
                run_research(
                    request,
                    &model,
                    &prepared.store,
                    cancel,
                    Instant::now() + RESEARCH_RUN_DEADLINE,
                    &current,
                    &mut emit,
                );
            }
        }
    });
    Ok(ResearchStartedResponse {
        run_id: payload.run_id,
        provider_id: payload.provider_id,
        model_id: payload.model_id,
    })
}

#[tauri::command]
pub async fn research_cancel(
    req: IpcRequest<ResearchCancelPayload>,
    state: State<'_, AppState>,
) -> Result<ResearchCancelResponse, IpcError> {
    req.check_version()?;
    validate_identity(&req.payload.run_id, "runId")?;
    Ok(ResearchCancelResponse {
        cancelled: research_cancel_impl(&state.research_runs, &req.payload.run_id),
    })
}

#[tauri::command]
pub async fn research_list_artifacts(
    req: IpcRequest<ResearchListArtifactsPayload>,
    state: State<'_, AppState>,
) -> Result<ResearchListArtifactsResponse, IpcError> {
    req.check_version()?;
    list_artifacts_impl(req.payload, &state)
}

fn list_artifacts_impl(
    payload: ResearchListArtifactsPayload,
    state: &AppState,
) -> Result<ResearchListArtifactsResponse, IpcError> {
    let owner = resolve_owner(&payload.owner, state)?;
    let store = ArtifactStore::from_owner(&owner).map_err(map_store_error)?;
    let artifacts = store
        .list()
        .map_err(map_store_error)?
        .into_iter()
        .map(|record| artifact_summary(&record))
        .collect();
    Ok(ResearchListArtifactsResponse { artifacts })
}

#[tauri::command]
pub async fn research_load_artifact(
    req: IpcRequest<ResearchLoadArtifactPayload>,
    state: State<'_, AppState>,
) -> Result<ResearchLoadArtifactResponse, IpcError> {
    req.check_version()?;
    load_artifact_impl(req.payload, &state)
}

fn load_artifact_impl(
    payload: ResearchLoadArtifactPayload,
    state: &AppState,
) -> Result<ResearchLoadArtifactResponse, IpcError> {
    validate_identity(&payload.artifact_id, "artifactId")?;
    let owner = resolve_owner(&payload.owner, state)?;
    let store = ArtifactStore::from_owner(&owner).map_err(map_store_error)?;
    let record = match payload.version {
        Some(version) => store
            .load_version(&payload.artifact_id, version)
            .map_err(map_store_error)?,
        None => store
            .load_latest(&payload.artifact_id)
            .map_err(map_store_error)?,
    };
    project_record(record)
}

fn validate_start_payload(payload: &ResearchStartPayload) -> Result<(), IpcError> {
    validate_identity(&payload.run_id, "runId")?;
    validate_identity(&payload.owner.session_id, "sessionId")?;
    if payload.question.trim().is_empty() || payload.question.len() > MAX_RESEARCH_QUESTION_BYTES {
        return Err(IpcError::BadArgument(
            "question must be 1..=8192 UTF-8 bytes".into(),
        ));
    }
    match (payload.provider_id.as_str(), payload.model_id.as_str()) {
        ("apple-foundation", "system") if payload.handle_id.is_none() => {}
        ("mlx-lm", QWEN_CATALOG_ID) if payload.handle_id.is_some() => {}
        _ => {
            return Err(IpcError::BadArgument(
                "research supports Apple system or the fixed Plume Qwen model with its handle"
                    .into(),
            ))
        }
    }
    Ok(())
}

fn prepare_research(
    payload: &ResearchStartPayload,
    state: &AppState,
) -> Result<PreparedResearch, IpcError> {
    validate_start_payload(payload)?;
    let owner = resolve_owner(&payload.owner, state)?;
    let sources = resolve_browser_evidence(&owner, &payload.sources, || trusted_open(state))
        .map_err(map_evidence_error)?;
    let store = ArtifactStore::from_owner(&owner).map_err(map_store_error)?;
    let framing = match payload.provider_id.as_str() {
        "apple-foundation" => ProviderFraming::AppleInstructions,
        "mlx-lm" => ProviderFraming::QwenChatMl,
        _ => unreachable!("validated provider"),
    };
    Ok(PreparedResearch {
        owner,
        sources,
        store,
        framing,
    })
}

fn resolve_owner(
    payload: &ResearchOwnerPayload,
    state: &AppState,
) -> Result<ResolvedSessionOwner, IpcError> {
    validate_identity(&payload.session_id, "sessionId")?;
    let scope = match payload.scope {
        ResearchOwnerScope::Local => SessionOwnerScope::Local,
        ResearchOwnerScope::Project => SessionOwnerScope::Project,
    };
    let project = trusted_open(state);
    resolve_session_owner(
        &SessionOwnerRef {
            scope,
            session_id: payload.session_id.clone(),
        },
        scope,
        &state.local_sessions_dir,
        project.as_ref(),
    )
    .map_err(map_owner_error)
}

fn trusted_open(state: &AppState) -> Option<OpenProject> {
    let open = state.session.current()?;
    let trusted = state
        .trust
        .lock()
        .expect("trust mutex poisoned")
        .is_trusted(&open.root);
    trusted.then_some(open)
}

enum ModelLaunch {
    Apple(NativeHelperPort),
    Qwen(SelectedResearchModel<'static>),
}

fn prepare_model_launch(
    payload: &ResearchStartPayload,
    app: &AppHandle,
) -> Result<ModelLaunch, IpcError> {
    match payload.provider_id.as_str() {
        "apple-foundation" => {
            if !platform_supports_apple_models() {
                return Err(IpcError::ProviderDown {
                    provider: payload.provider_id.clone(),
                    reason: "unsupported macOS version".into(),
                });
            }
            let resources = app
                .path()
                .resource_dir()
                .map_err(|error| IpcError::Internal(format!("resolve app resources: {error}")))?;
            let helper = NativeHelperPort::from_resource_dir(&resources);
            select_model(
                &payload.provider_id,
                &payload.model_id,
                payload.handle_id.as_deref(),
                Some(&helper),
                true,
            )
            .map_err(map_model_selection_error)?;
            Ok(ModelLaunch::Apple(helper))
        }
        "mlx-lm" => {
            let model: SelectedResearchModel<'static> = select_model(
                &payload.provider_id,
                &payload.model_id,
                payload.handle_id.as_deref(),
                None,
                true,
            )
            .map_err(map_model_selection_error)?;
            Ok(ModelLaunch::Qwen(model))
        }
        _ => Err(IpcError::BadArgument(
            "unsupported research provider".into(),
        )),
    }
}

fn research_cancel_impl(registry: &Arc<ResearchRunRegistry>, run_id: &str) -> bool {
    registry.cancel(run_id)
}

fn owner_is_current(
    owner: &ResolvedSessionOwner,
    project_session: &Arc<crate::project::ProjectSession>,
) -> bool {
    if !sessions::session_exists(&owner.sessions_dir, &owner.session_id).unwrap_or(false) {
        return false;
    }
    match &owner.project {
        None => owner.scope == SessionOwnerScope::Local,
        Some(expected) => project_session
            .current()
            .is_some_and(|current| current.id == expected.id && current.root == expected.root),
    }
}

fn owner_key(owner: &ResolvedSessionOwner) -> String {
    match &owner.project {
        None => local_owner_key(&owner.session_id),
        Some(project) => project_owner_key(&project.id, &owner.session_id),
    }
}

fn project_record(record: ArtifactBundleRecord) -> Result<ResearchLoadArtifactResponse, IpcError> {
    let draft = record
        .input
        .drafts
        .last()
        .ok_or_else(|| IpcError::Internal("research artifact has no draft".into()))?;
    let markdown = match draft.citation_status {
        ArtifactCitationStatus::Verified => {
            project_markdown(&draft.markdown, &record.input.sources)
        }
        ArtifactCitationStatus::NeedsReview => {
            project_markdown_for_review(&draft.markdown, &record.input.sources)
        }
    }
    .map_err(|error| IpcError::Internal(error.to_string()))?;
    let sources = record.input.sources.iter().map(source_view).collect();
    Ok(ResearchLoadArtifactResponse {
        artifact: artifact_summary(&record),
        markdown,
        sources,
        logical_turns: record.input.logical_turns,
        provider_calls: record.input.provider_calls,
        duration_ms: record.input.duration_ms,
    })
}

fn artifact_summary(record: &ArtifactBundleRecord) -> ResearchArtifactSummary {
    ResearchArtifactSummary {
        artifact_id: record.artifact_id.clone(),
        version: record.artifact_version,
        created_at_ms: record.created_at_ms,
        question: record.input.user_request.clone(),
        provider_id: record.input.provider_id.clone(),
        model_id: record.input.model_id.clone(),
        citation_status: record
            .input
            .drafts
            .last()
            .map(|draft| draft.citation_status)
            .unwrap_or(ArtifactCitationStatus::NeedsReview),
        outcome: record.input.outcome,
    }
}

fn source_view(source: &ResearchEvidenceSource) -> ResearchSourceView {
    ResearchSourceView {
        source_id: source.source_id.clone(),
        evidence_id: source.evidence_id.clone(),
        source_url: source.source_url.clone(),
        title: source.title.clone(),
        captured_at_ms: source.captured_at_ms,
        sha256: source.sha256.clone(),
        bytes: source.bytes,
        redaction_count: source.redaction_count,
        truncated: source.truncated,
    }
}

fn validate_identity(value: &str, label: &str) -> Result<(), IpcError> {
    if value.is_empty()
        || value.len() > 96
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    {
        return Err(IpcError::BadArgument(format!("invalid {label}")));
    }
    Ok(())
}

fn map_owner_error(error: SessionOwnerError) -> IpcError {
    match error {
        SessionOwnerError::ScopeMismatch => IpcError::BadArgument("session owner scope".into()),
        SessionOwnerError::ProjectUnavailable => IpcError::NeedsApproval,
        SessionOwnerError::NotFound => IpcError::NotFound("research session owner".into()),
        SessionOwnerError::Store(error) => IpcError::Internal(error.to_string()),
    }
}

fn map_evidence_error(error: ResearchEvidenceError) -> IpcError {
    match error {
        ResearchEvidenceError::SourceCount
        | ResearchEvidenceError::UnsupportedSourceKind
        | ResearchEvidenceError::DuplicateSource => IpcError::BadArgument(error.to_string()),
        ResearchEvidenceError::NotOnOwnerShelf
        | ResearchEvidenceError::SourceTooLarge
        | ResearchEvidenceError::TotalTooLarge => IpcError::Blocked(error.to_string()),
        ResearchEvidenceError::StaleProjectGeneration => IpcError::NeedsApproval,
        ResearchEvidenceError::EvidenceUnavailable | ResearchEvidenceError::OwnerUnavailable => {
            IpcError::NotFound(error.to_string())
        }
    }
}

fn map_store_error(error: ArtifactStoreError) -> IpcError {
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

fn map_model_selection_error(error: ResearchModelSelectionError) -> IpcError {
    match error {
        ResearchModelSelectionError::HandleNotFound => {
            IpcError::NotFound("research model runtime handle".into())
        }
        ResearchModelSelectionError::HelperUnavailable => IpcError::ProviderDown {
            provider: "apple-foundation".into(),
            reason: error.to_string(),
        },
        _ => IpcError::BadArgument(error.to_string()),
    }
}

#[cfg(test)]
#[path = "research_tests.rs"]
mod tests;
