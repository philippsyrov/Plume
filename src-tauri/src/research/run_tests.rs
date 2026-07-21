use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};

use crate::agent::events::{ResearchEvent, ResearchRecoveryReason, ResearchTerminalStatus};
use crate::agent::protocol::ProviderFraming;
use crate::browser::evidence::BrowserCaptureKind;
use crate::chat::ChatMessage;
use crate::providers::catalog::QWEN2_VL_CATALOG_ID;
use crate::research::bundle::{ArtifactBundleInput, ArtifactCitationStatus};
use crate::research::evidence::ResearchEvidenceSource;
use crate::research::model::{
    ModelCapabilities, ModelFinish, ModelTurnResult, ResearchModelError, ResearchModelPort,
};

use super::run::{run_research, ArtifactStageRef, ResearchArtifactPort, ResearchRunRequest};
use super::run_registry::ResearchRunRegistry;

enum FakeReply {
    Turn(ModelTurnResult),
    Error,
    ModelError(ResearchModelError),
}

struct FakeModel {
    replies: Mutex<VecDeque<FakeReply>>,
    seen: Mutex<Vec<Vec<ChatMessage>>>,
    cancel_after_call: bool,
}

impl FakeModel {
    fn new(replies: Vec<FakeReply>) -> Self {
        Self {
            replies: Mutex::new(replies.into()),
            seen: Mutex::new(Vec::new()),
            cancel_after_call: false,
        }
    }
}

impl ResearchModelPort for FakeModel {
    fn capabilities(&self) -> Result<ModelCapabilities, ResearchModelError> {
        Ok(ModelCapabilities {
            context_tokens: 4096,
            exact_token_count: false,
        })
    }

    fn complete(
        &self,
        messages: &[ChatMessage],
        cancel: Arc<AtomicBool>,
        _deadline: Instant,
    ) -> Result<ModelTurnResult, ResearchModelError> {
        self.seen.lock().unwrap().push(messages.to_vec());
        let reply = self.replies.lock().unwrap().pop_front().unwrap();
        if self.cancel_after_call {
            cancel.store(true, Ordering::SeqCst);
        }
        match reply {
            FakeReply::Turn(turn) => Ok(turn),
            FakeReply::Error => Err(ResearchModelError::Capabilities("injected".into())),
            FakeReply::ModelError(error) => Err(error),
        }
    }
}

#[test]
fn fixed_catalog_model_not_found_hides_the_receipt_path_from_research_diagnostics() {
    let install_path = "/Users/example/Library/Application Support/Plume/models/catalog/qwen2-vl-2b-instruct-4bit/01af461cdb9574acc09084a0ef94e216e142b085";
    let model = FakeModel::new(vec![FakeReply::ModelError(ResearchModelError::Qwen2Vl(
        crate::chat::mlx_lm::ChatError::ModelNotFound {
            model: install_path.into(),
            message: format!("Model {install_path} was not found"),
        },
    ))]);
    let store = FakeStore::default();
    let mut events = Vec::new();
    let mut run_request = request(ProviderFraming::QwenChatMl, 1);
    run_request.provider_id = "mlx-vlm".into();
    run_request.model_id = QWEN2_VL_CATALOG_ID.into();

    let result = run_research(
        run_request,
        &model,
        &store,
        Arc::new(AtomicBool::new(false)),
        Instant::now() + Duration::from_secs(1),
        &|| true,
        &mut |event| events.push(event),
    );
    let diagnostic = result.diagnostic.expect("safe research diagnostic");
    let terminal_diagnostic = events
        .iter()
        .find_map(|event| match &event.event {
            ResearchEvent::Terminal { diagnostic, .. } => diagnostic.clone(),
            _ => None,
        })
        .expect("safe terminal diagnostic");

    for message in [&diagnostic, &terminal_diagnostic] {
        assert!(
            message.contains(QWEN2_VL_CATALOG_ID),
            "message was: {message}"
        );
        assert!(
            !message.contains("/Users/example"),
            "message was: {message}"
        );
        assert!(
            !message.contains("Application Support"),
            "message was: {message}"
        );
    }
}

#[derive(Default)]
struct FakeStore {
    staged: Mutex<Vec<ArtifactBundleInput>>,
    fail: bool,
}

impl ResearchArtifactPort for FakeStore {
    fn stage(&self, input: ArtifactBundleInput) -> Result<ArtifactStageRef, String> {
        if self.fail {
            return Err("injected store failure".into());
        }
        self.staged.lock().unwrap().push(input);
        Ok(ArtifactStageRef {
            artifact_id: format!("ra_{}", "a".repeat(32)),
            artifact_version: 1,
        })
    }
}

fn turn(text: String) -> FakeReply {
    FakeReply::Turn(ModelTurnResult {
        text,
        prompt_tokens: Some(10),
        output_tokens: Some(5),
        finish: ModelFinish::Stop,
    })
}

fn summary(source_id: &str) -> FakeReply {
    turn(format!(
        "<plume_tool_call>{{\"callId\":\"c-{source_id}\",\"tool\":\"research.summary.submit\",\"arguments\":{{\"sourceId\":\"{source_id}\",\"summary\":\"Summary for {source_id}.\"}}}}</plume_tool_call>"
    ))
}

fn markdown(body: &str) -> FakeReply {
    turn(format!(
        "<plume_tool_call>{{\"callId\":\"c-note\",\"tool\":\"artifact.markdown.submit\",\"arguments\":{{\"markdown\":{}}}}}</plume_tool_call>",
        serde_json::to_string(body).unwrap()
    ))
}

fn source(index: usize) -> ResearchEvidenceSource {
    let content = format!("Evidence body {index}");
    ResearchEvidenceSource {
        source_id: format!("S{index}"),
        evidence_id: format!("be_{index:032x}"),
        capture_kind: BrowserCaptureKind::Page,
        source_url: format!("https://example.com/{index}"),
        title: Some(format!("Source {index}")),
        captured_at_ms: index as u64,
        sha256: format!("{:x}", Sha256::digest(content.as_bytes())),
        bytes: content.len() as u64,
        content,
        redaction_count: 0,
        truncated: false,
    }
}

fn request(framing: ProviderFraming, source_count: usize) -> ResearchRunRequest {
    ResearchRunRequest {
        run_id: "run_stage_a".into(),
        question: "Synthesize the evidence".into(),
        provider_id: match framing {
            ProviderFraming::QwenChatMl => "mlx-lm",
            ProviderFraming::AppleInstructions => "apple-foundation",
        }
        .into(),
        model_id: "test-model".into(),
        runtime_id: "test-runtime".into(),
        framing,
        sources: (1..=source_count).map(source).collect(),
        screenshot_sources: Vec::new(),
    }
}

#[test]
fn qwen_and_apple_drive_the_same_map_reduce_event_order() {
    for framing in [
        ProviderFraming::QwenChatMl,
        ProviderFraming::AppleInstructions,
    ] {
        let model = FakeModel::new(vec![
            summary("S1"),
            summary("S2"),
            markdown("# Note\n\nClaim [[S1]] and detail [[S2]]."),
        ]);
        let store = FakeStore::default();
        let mut events = Vec::new();
        let result = run_research(
            request(framing, 2),
            &model,
            &store,
            Arc::new(AtomicBool::new(false)),
            Instant::now() + Duration::from_secs(1),
            &|| true,
            &mut |event| events.push(event),
        );
        assert_eq!(result.status, ResearchTerminalStatus::Complete);
        assert_eq!(result.budget.logical_turns, 3);
        assert_eq!(result.budget.provider_calls, 3);
        assert_eq!(store.staged.lock().unwrap().len(), 1);
        assert!(matches!(
            events.last().unwrap().event,
            ResearchEvent::Terminal { .. }
        ));
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(event.event, ResearchEvent::Terminal { .. }))
                .count(),
            1
        );
        for (expected, event) in events.iter().enumerate() {
            assert_eq!(event.seq, expected as u64);
        }
    }
}

#[test]
fn malformed_reask_is_typed_and_second_malformed_fails_without_staging() {
    let model = FakeModel::new(vec![turn("bad".into()), turn("bad again".into())]);
    let store = FakeStore::default();
    let mut events = Vec::new();
    let result = run_research(
        request(ProviderFraming::AppleInstructions, 1),
        &model,
        &store,
        Arc::new(AtomicBool::new(false)),
        Instant::now() + Duration::from_secs(1),
        &|| true,
        &mut |event| events.push(event),
    );
    assert_eq!(result.status, ResearchTerminalStatus::Failed);
    assert!(store.staged.lock().unwrap().is_empty());
    assert!(events.iter().any(|event| matches!(
        event.event,
        ResearchEvent::Recovery {
            reason: ResearchRecoveryReason::MalformedFraming,
            ..
        }
    )));
}

#[test]
fn two_failed_citation_repairs_stage_an_ordinary_review_needed_draft() {
    let model = FakeModel::new(vec![
        summary("S1"),
        markdown("# Draft\n\nUncited claim."),
        markdown("# Draft two\n\nStill uncited."),
        markdown("# Draft three\n\nStill uncited."),
    ]);
    let store = FakeStore::default();
    let mut events = Vec::new();
    let result = run_research(
        request(ProviderFraming::QwenChatMl, 1),
        &model,
        &store,
        Arc::new(AtomicBool::new(false)),
        Instant::now() + Duration::from_secs(1),
        &|| true,
        &mut |event| events.push(event),
    );
    assert_eq!(
        result.status,
        ResearchTerminalStatus::NeedsReview,
        "{:?}",
        result.diagnostic
    );
    let staged = store.staged.lock().unwrap();
    assert_eq!(staged.len(), 1);
    assert_eq!(staged[0].drafts.len(), 3);
    assert_eq!(
        staged[0].drafts.last().unwrap().citation_status,
        ArtifactCitationStatus::NeedsReview
    );
    assert!(result.artifact.is_some());
}

#[test]
fn provider_owner_store_and_cancel_fail_closed_at_boundaries() {
    let cases = [
        (
            FakeModel::new(vec![FakeReply::Error]),
            FakeStore::default(),
            Arc::new(AtomicBool::new(false)),
            true,
        ),
        (
            FakeModel::new(vec![summary("S1"), markdown("Claim [[S1]].")]),
            FakeStore {
                staged: Mutex::new(Vec::new()),
                fail: true,
            },
            Arc::new(AtomicBool::new(false)),
            true,
        ),
    ];
    for (model, store, cancel, owner_current) in cases {
        let result = run_research(
            request(ProviderFraming::AppleInstructions, 1),
            &model,
            &store,
            cancel,
            Instant::now() + Duration::from_secs(1),
            &|| owner_current,
            &mut |_| {},
        );
        assert_eq!(result.status, ResearchTerminalStatus::Failed);
    }

    let result = run_research(
        request(ProviderFraming::AppleInstructions, 1),
        &FakeModel::new(vec![]),
        &FakeStore::default(),
        Arc::new(AtomicBool::new(true)),
        Instant::now() + Duration::from_secs(1),
        &|| true,
        &mut |_| {},
    );
    assert_eq!(result.status, ResearchTerminalStatus::Stopped);

    let result = run_research(
        request(ProviderFraming::AppleInstructions, 1),
        &FakeModel::new(vec![]),
        &FakeStore::default(),
        Arc::new(AtomicBool::new(false)),
        Instant::now() + Duration::from_secs(1),
        &|| false,
        &mut |_| {},
    );
    assert_eq!(result.status, ResearchTerminalStatus::Failed);
}

#[test]
fn registry_rejects_duplicates_cancels_idempotently_and_removes_exact_lease() {
    let registry = Arc::new(ResearchRunRegistry::default());
    let lease = registry.register("run_123", "local:s1").unwrap();
    assert!(registry.register("run_123", "local:s1").is_err());
    assert!(registry.cancel("run_123"));
    assert!(registry.cancel("run_123"));
    assert!(lease.cancel_flag().load(Ordering::SeqCst));
    drop(lease);
    assert!(!registry.cancel("run_123"));
    assert!(registry.register("run_123", "project:p1:s1").is_ok());
}

#[test]
fn worst_case_workflow_stops_at_thirteen_logical_and_twenty_six_provider_calls() {
    let mut replies = Vec::new();
    for index in 1..=10 {
        replies.push(turn("malformed".into()));
        replies.push(summary(&format!("S{index}")));
    }
    for draft in ["First uncited.", "Second uncited.", "Third uncited."] {
        replies.push(turn("malformed".into()));
        replies.push(markdown(draft));
    }
    let model = FakeModel::new(replies);
    let store = FakeStore::default();
    let result = run_research(
        request(ProviderFraming::QwenChatMl, 10),
        &model,
        &store,
        Arc::new(AtomicBool::new(false)),
        Instant::now() + Duration::from_secs(1),
        &|| true,
        &mut |_| {},
    );
    assert_eq!(result.status, ResearchTerminalStatus::NeedsReview);
    assert_eq!(result.budget.logical_turns, 13);
    assert_eq!(result.budget.provider_calls, 26);
    assert_eq!(model.seen.lock().unwrap().len(), 26);
}
