use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex};

use serde_json::json;

use super::*;
use crate::agent::AgentConfig;
use crate::browser::evidence::{BrowserCaptureKind, CapturedBrowserText};
use crate::browser::local_evidence::{store_local_text_evidence, LocalEvidenceOwner};
use crate::chat::stream::ChatStreamRegistry;
use crate::commands::project::AppState;
use crate::project::trust::TrustStore;
use crate::project::ProjectSession;
use crate::prompts::ContextSourceRef;
use crate::providers::catalog::CatalogStore;
use crate::providers::catalog_download::CatalogDownloadRegistry;
use crate::research::bundle::{
    ArtifactBundleInput, ArtifactCitationStatus, ArtifactOutcome, ArtifactStore, BundleDraft,
    BundleSourceSummary,
};
use crate::research::evidence::ResearchEvidenceSource;
use crate::research::run_registry::ResearchRunRegistry;
use crate::sessions;

fn state(base: &Path) -> AppState {
    AppState {
        session: Arc::new(ProjectSession::default()),
        trust: Mutex::new(TrustStore::load(base.join("trust.json"))),
        chat_streams: Arc::new(ChatStreamRegistry::default()),
        research_runs: Arc::new(ResearchRunRegistry::default()),
        agent_config: Mutex::new(AgentConfig::default()),
        local_sessions_dir: base.join("app-data/sessions"),
        user_memory_dir: base.join("app-data/memory"),
        catalog_store: Arc::new(CatalogStore::new(base.join("app-data"))),
        catalog_downloads: Arc::new(CatalogDownloadRegistry::default()),
    }
}

fn start_payload(session_id: String, evidence_id: String) -> ResearchStartPayload {
    ResearchStartPayload {
        run_id: "run_123".into(),
        owner: ResearchOwnerPayload {
            scope: ResearchOwnerScope::Local,
            session_id,
        },
        question: "Write a short note".into(),
        provider_id: "apple-foundation".into(),
        model_id: "system".into(),
        handle_id: None,
        sources: vec![ContextSourceRef::BrowserTextEvidence { evidence_id }],
    }
}

#[test]
fn start_payload_is_strict_and_pins_the_listener_before_start_contract() {
    let parsed: ResearchStartPayload = serde_json::from_value(json!({
        "runId": "run_123",
        "owner": { "scope": "local", "sessionId": "s1" },
        "question": "Write a note",
        "providerId": "apple-foundation",
        "modelId": "system",
        "handleId": null,
        "sources": [{ "kind": "browserTextEvidence", "evidenceId": "be_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa" }]
    }))
    .unwrap();
    validate_start_payload(&parsed).unwrap();
    assert!(matches!(
        IpcRequest {
            ipc_version: crate::error::IPC_VERSION + 1,
            payload: parsed.clone(),
        }
        .check_version(),
        Err(IpcError::Version { .. })
    ));
    assert_eq!(RESEARCH_EVENT_CHANNEL, "research/event");
    assert!(serde_json::from_value::<ResearchStartPayload>(json!({
        "runId": "run_123", "owner": { "scope": "local", "sessionId": "s1" },
        "question": "Write a note", "providerId": "apple-foundation", "modelId": "system",
        "handleId": null, "sources": [], "root": "/tmp"
    }))
    .is_err());
    let mut invalid = parsed.clone();
    invalid.handle_id = Some("unexpected".into());
    assert!(matches!(
        validate_start_payload(&invalid),
        Err(IpcError::BadArgument(_))
    ));
    invalid.provider_id = "mlx-lm".into();
    invalid.model_id = QWEN_CATALOG_ID.into();
    invalid.handle_id = None;
    assert!(matches!(
        validate_start_payload(&invalid),
        Err(IpcError::BadArgument(_))
    ));
    invalid.handle_id = Some("handle_123".into());
    invalid.question = "x".repeat(MAX_RESEARCH_QUESTION_BYTES + 1);
    assert!(matches!(
        validate_start_payload(&invalid),
        Err(IpcError::BadArgument(_))
    ));
    assert!(serde_json::from_value::<ResearchCancelPayload>(json!({
        "runId": "run_123", "extra": true
    }))
    .is_err());
    assert!(
        serde_json::from_value::<ResearchListArtifactsPayload>(json!({
            "owner": { "scope": "local", "sessionId": "s1" }, "extra": true
        }))
        .is_err()
    );
    assert!(
        serde_json::from_value::<ResearchLoadArtifactPayload>(json!({
            "owner": { "scope": "local", "sessionId": "s1" },
            "artifactId": "ra_123", "version": null, "extra": true
        }))
        .is_err()
    );
}

#[test]
fn preflight_resolves_only_the_exact_local_session_shelf() {
    let temp = tempfile::tempdir().unwrap();
    let state = state(temp.path());
    let session = sessions::create(&state.local_sessions_dir, None).unwrap();
    let evidence = store_local_text_evidence(
        &state.local_sessions_dir,
        &LocalEvidenceOwner {
            session_id: session.id.clone(),
        },
        CapturedBrowserText {
            capture_kind: BrowserCaptureKind::Page,
            source_url: "https://example.com".into(),
            title: Some("Example".into()),
            content: "evidence".into(),
            source_truncated: false,
        },
    )
    .unwrap();
    let refs = vec![ContextSourceRef::BrowserTextEvidence {
        evidence_id: evidence.evidence_id.clone(),
    }];
    sessions::save_transcript_with_context(
        &state.local_sessions_dir,
        &session.id,
        &[],
        &refs,
        false,
    )
    .unwrap();

    let prepared = prepare_research(
        &start_payload(session.id.clone(), evidence.evidence_id),
        &state,
    )
    .unwrap();
    assert_eq!(prepared.owner.session_id, session.id);
    assert_eq!(prepared.sources[0].source_id, "S1");

    let other = sessions::create(&state.local_sessions_dir, None).unwrap();
    let error = prepare_research(
        &start_payload(other.id, prepared.sources[0].evidence_id.clone()),
        &state,
    )
    .unwrap_err();
    assert!(matches!(error, crate::error::IpcError::Blocked(_)));
}

#[test]
fn project_preflight_requires_current_trust_and_exact_generation() {
    let temp = tempfile::tempdir().unwrap();
    let state = state(temp.path());
    let project = temp.path().join("project");
    fs::create_dir_all(&project).unwrap();
    let project = fs::canonicalize(project).unwrap();
    state.session.open(project.clone());
    let payload = ResearchStartPayload {
        owner: ResearchOwnerPayload {
            scope: ResearchOwnerScope::Project,
            session_id: "missing".into(),
        },
        ..start_payload("missing".into(), format!("be_{}", "a".repeat(32)))
    };
    assert!(matches!(
        prepare_research(&payload, &state),
        Err(crate::error::IpcError::NeedsApproval)
    ));
}

#[test]
fn cancel_is_idempotent_and_unknown_runs_are_honest_noops() {
    let registry = Arc::new(ResearchRunRegistry::default());
    assert!(!research_cancel_impl(&registry, "run_123"));
    let lease = registry.register("run_123", "local:s1").unwrap();
    assert!(research_cancel_impl(&registry, "run_123"));
    assert!(research_cancel_impl(&registry, "run_123"));
    assert!(lease
        .cancel_flag()
        .load(std::sync::atomic::Ordering::SeqCst));
}

#[test]
fn list_and_load_are_session_scoped_and_never_return_source_bodies() {
    let temp = tempfile::tempdir().unwrap();
    let state = state(temp.path());
    let first = sessions::create(&state.local_sessions_dir, None).unwrap();
    let second = sessions::create(&state.local_sessions_dir, None).unwrap();
    let owner = resolve_owner(
        &ResearchOwnerPayload {
            scope: ResearchOwnerScope::Local,
            session_id: first.id.clone(),
        },
        &state,
    )
    .unwrap();
    let store = ArtifactStore::from_owner(&owner).unwrap();
    let record = store.stage_new(artifact_input()).unwrap();

    let listed = list_artifacts_impl(
        ResearchListArtifactsPayload {
            owner: ResearchOwnerPayload {
                scope: ResearchOwnerScope::Local,
                session_id: first.id.clone(),
            },
        },
        &state,
    )
    .unwrap();
    assert_eq!(listed.artifacts.len(), 1);
    let other = list_artifacts_impl(
        ResearchListArtifactsPayload {
            owner: ResearchOwnerPayload {
                scope: ResearchOwnerScope::Local,
                session_id: second.id.clone(),
            },
        },
        &state,
    )
    .unwrap();
    assert!(other.artifacts.is_empty());
    assert!(matches!(
        load_artifact_impl(
            ResearchLoadArtifactPayload {
                owner: ResearchOwnerPayload {
                    scope: ResearchOwnerScope::Local,
                    session_id: second.id,
                },
                artifact_id: record.artifact_id.clone(),
                version: None,
            },
            &state,
        ),
        Err(IpcError::NotFound(_))
    ));

    let loaded = load_artifact_impl(
        ResearchLoadArtifactPayload {
            owner: ResearchOwnerPayload {
                scope: ResearchOwnerScope::Local,
                session_id: first.id,
            },
            artifact_id: record.artifact_id,
            version: None,
        },
        &state,
    )
    .unwrap();
    let wire = serde_json::to_string(&loaded).unwrap();
    assert!(loaded.markdown.contains("[^S1]"));
    assert!(!wire.contains("secret source body"));
    assert!(!wire.contains("\"content\""));
}

fn artifact_input() -> ArtifactBundleInput {
    use sha2::Digest;

    let content = "secret source body".to_string();
    ArtifactBundleInput {
        user_request: "Write a note".into(),
        provider_id: "apple-foundation".into(),
        model_id: "system".into(),
        runtime_id: "apple-system".into(),
        sources: vec![ResearchEvidenceSource {
            source_id: "S1".into(),
            evidence_id: format!("be_{}", "b".repeat(32)),
            capture_kind: BrowserCaptureKind::Page,
            source_url: "https://example.com/research".into(),
            title: Some("Research source".into()),
            captured_at_ms: 1,
            sha256: format!("{:x}", sha2::Sha256::digest(content.as_bytes())),
            bytes: content.len() as u64,
            content,
            redaction_count: 0,
            truncated: false,
        }],
        summaries: vec![BundleSourceSummary {
            source_id: "S1".into(),
            summary: "summary".into(),
            logical_turn: 1,
            provider_calls: 1,
        }],
        drafts: vec![BundleDraft {
            markdown: "A cited note [[S1]].".into(),
            citation_status: ArtifactCitationStatus::Verified,
        }],
        logical_turns: 2,
        provider_calls: 2,
        duration_ms: 5,
        outcome: ArtifactOutcome::Complete,
    }
}
