//! Production Stage A research workflow and active-run registry.

#![allow(dead_code)] // Task 9 wires the run into managed IPC state.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use crate::agent::controller::{run_loop, LoopOutcome, StepOutcome};
use crate::agent::events::{
    ResearchCitationStatus, ResearchEvent, ResearchEventEnvelope, ResearchPhase,
    ResearchRecoveryReason, ResearchTerminalStatus,
};
use crate::agent::harness::{execute_tool_turn, HarnessError, ToolTurn};
use crate::agent::protocol::{ExpectedTool, ProviderFraming, ToolArguments};

use super::budget::{RecoveryReason, ResearchBudget, ResearchBudgetSnapshot, MAX_LOGICAL_TURNS};
use super::bundle::{
    ArtifactBundleInput, ArtifactCitationStatus, ArtifactOutcome, ArtifactStore, BundleDraft,
    BundleSourceSummary,
};
use super::citations::verify_citations;
use super::context::{
    pack_source_summary_for_request, pack_synthesis_for_request, PackingAttempt,
    SummaryForSynthesis, TokenCounter,
};
use super::evidence::{ResearchEvidenceSource, ResearchScreenshotSource};
use super::model::{estimate_tokens_conservatively, ModelCapabilities, ResearchModelPort};

const MAX_REPAIR_DRAFT_BYTES: usize = 4 * 1024;

#[derive(Debug, Clone)]
pub(crate) struct ResearchRunRequest {
    pub run_id: String,
    pub question: String,
    pub provider_id: String,
    pub model_id: String,
    pub runtime_id: String,
    pub framing: ProviderFraming,
    pub sources: Vec<ResearchEvidenceSource>,
    pub screenshot_sources: Vec<ResearchScreenshotSource>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ArtifactStageRef {
    pub artifact_id: String,
    pub artifact_version: u32,
}

pub(crate) trait ResearchArtifactPort {
    fn stage(&self, input: ArtifactBundleInput) -> Result<ArtifactStageRef, String>;
}

impl ResearchArtifactPort for ArtifactStore {
    fn stage(&self, input: ArtifactBundleInput) -> Result<ArtifactStageRef, String> {
        self.stage_new(input)
            .map(|record| ArtifactStageRef {
                artifact_id: record.artifact_id,
                artifact_version: record.artifact_version,
            })
            .map_err(|error| error.to_string())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResearchRunResult {
    pub status: ResearchTerminalStatus,
    pub artifact: Option<ArtifactStageRef>,
    pub budget: ResearchBudgetSnapshot,
    pub diagnostic: Option<String>,
}

pub(crate) fn run_research(
    request: ResearchRunRequest,
    model: &dyn ResearchModelPort,
    store: &dyn ResearchArtifactPort,
    cancel: Arc<AtomicBool>,
    deadline: Instant,
    owner_is_current: &dyn Fn() -> bool,
    emit: &mut dyn FnMut(ResearchEventEnvelope),
) -> ResearchRunResult {
    let started = Instant::now();
    let mut emitter = ResearchEmitter::new(request.run_id.clone(), emit);
    if cancel.load(Ordering::SeqCst) {
        return finish_early(
            &mut emitter,
            ResearchTerminalStatus::Stopped,
            "Research stopped before it started",
        );
    }
    if !owner_is_current() {
        return finish_early(
            &mut emitter,
            ResearchTerminalStatus::Failed,
            "The chat or project changed before research started",
        );
    }
    let capabilities = match model.capabilities() {
        Ok(capabilities) => capabilities,
        Err(error) => {
            return finish_early(
                &mut emitter,
                ResearchTerminalStatus::Failed,
                &format!("Model capabilities were unavailable: {error}"),
            )
        }
    };
    let mut workflow = Workflow {
        request,
        model,
        store,
        cancel: cancel.clone(),
        deadline,
        owner_is_current,
        emitter: &mut emitter,
        capabilities,
        budget: ResearchBudget::default(),
        summaries: Vec::new(),
        drafts: Vec::new(),
        summary_index: 0,
        repair_count: 0,
        artifact: None,
        citation_status: None,
        diagnostic: None,
        cancelled: false,
        started,
    };
    let report = run_loop(
        MAX_LOGICAL_TURNS,
        || cancel.load(Ordering::SeqCst),
        |_| workflow.step(),
    );
    let (status, diagnostic) = match report.outcome {
        LoopOutcome::Done => match workflow.citation_status {
            Some(ArtifactCitationStatus::Verified) => (ResearchTerminalStatus::Complete, None),
            Some(ArtifactCitationStatus::NeedsReview) => (
                ResearchTerminalStatus::NeedsReview,
                Some("Draft staged; citations need review".to_string()),
            ),
            None => (
                ResearchTerminalStatus::Failed,
                Some("Research ended without a staged artifact".to_string()),
            ),
        },
        LoopOutcome::Aborted if workflow.cancelled || cancel.load(Ordering::SeqCst) => (
            ResearchTerminalStatus::Stopped,
            Some("Research stopped".to_string()),
        ),
        LoopOutcome::Failed { reason } if workflow.cancelled => {
            (ResearchTerminalStatus::Stopped, Some(reason))
        }
        LoopOutcome::Failed { reason } => (ResearchTerminalStatus::Failed, Some(reason)),
        LoopOutcome::BudgetExhausted => (
            ResearchTerminalStatus::Failed,
            Some("Research reached its 13-turn limit".to_string()),
        ),
        LoopOutcome::Paused { reason } => (ResearchTerminalStatus::Failed, Some(reason)),
        LoopOutcome::Aborted => (
            ResearchTerminalStatus::Stopped,
            Some("Research stopped".to_string()),
        ),
    };
    let artifact = workflow.artifact.clone();
    let budget = workflow.budget.snapshot();
    drop(workflow);
    emitter.terminal(status, artifact.as_ref(), diagnostic.clone());
    ResearchRunResult {
        status,
        artifact,
        budget,
        diagnostic,
    }
}

struct Workflow<'a, 'e, 's> {
    request: ResearchRunRequest,
    model: &'a dyn ResearchModelPort,
    store: &'a dyn ResearchArtifactPort,
    cancel: Arc<AtomicBool>,
    deadline: Instant,
    owner_is_current: &'a dyn Fn() -> bool,
    emitter: &'e mut ResearchEmitter<'s>,
    capabilities: ModelCapabilities,
    budget: ResearchBudget,
    summaries: Vec<BundleSourceSummary>,
    drafts: Vec<BundleDraft>,
    summary_index: usize,
    repair_count: usize,
    artifact: Option<ArtifactStageRef>,
    citation_status: Option<ArtifactCitationStatus>,
    diagnostic: Option<String>,
    cancelled: bool,
    started: Instant,
}

impl Workflow<'_, '_, '_> {
    fn step(&mut self) -> StepOutcome {
        if let Err(reason) = self.check_boundary() {
            return self.failed(reason);
        }
        if let Err(error) = self.budget.begin_logical_turn() {
            return self.failed(format!("Research turn budget refused: {error:?}"));
        }
        if self.summary_index < self.request.sources.len() {
            self.summary_step()
        } else {
            self.draft_step()
        }
    }

    fn summary_step(&mut self) -> StepOutcome {
        let source = self.request.sources[self.summary_index].clone();
        let current = self.summary_index as u32 + 1;
        let total = self.request.sources.len() as u32;
        if let Err(reason) = self.emitter.progress(
            ResearchPhase::Summarizing,
            Some("research.summary.submit"),
            current,
            total,
            self.budget.snapshot(),
            format!("Summarizing source {current} of {total}"),
        ) {
            return self.failed(reason);
        }
        let counter = ConservativeCounter;
        let initial = match pack_source_summary_for_request(
            &source,
            &self.request.question,
            self.capabilities,
            self.request.framing,
            &counter,
            PackingAttempt::Initial,
        ) {
            Ok(pack) => pack,
            Err(error) => {
                return self.failed(format!("Source context could not be packed: {error}"))
            }
        };
        let recovery = pack_source_summary_for_request(
            &source,
            &self.request.question,
            self.capabilities,
            self.request.framing,
            &counter,
            PackingAttempt::Recovery,
        )
        .ok();
        let mut recovery_events = Vec::new();
        let execution = execute_tool_turn(
            self.model,
            self.cancel.clone(),
            self.deadline,
            &mut self.budget,
            ToolTurn {
                expected: ExpectedTool::Summary {
                    source_id: &source.source_id,
                },
                initial,
                overflow_recovery: recovery,
            },
            |reason, snapshot| recovery_events.push((reason, snapshot)),
        );
        if let Err(reason) = self.emit_recoveries(ResearchPhase::Summarizing, recovery_events) {
            return self.failed(reason);
        }
        if let Err(reason) = self.check_boundary() {
            return self.failed(reason);
        }
        let execution = match execution {
            Ok(execution) => execution,
            Err(error) => return self.harness_failed(error),
        };
        let ToolArguments::Summary { summary, .. } = execution.call.arguments else {
            return self.failed("Research summary turn returned the wrong tool".into());
        };
        let snapshot = self.budget.snapshot();
        self.summaries.push(BundleSourceSummary {
            source_id: source.source_id.clone(),
            summary,
            logical_turn: snapshot.logical_turns,
            provider_calls: snapshot.provider_calls,
        });
        self.summary_index += 1;
        StepOutcome::Continue
    }

    fn draft_step(&mut self) -> StepOutcome {
        let is_repair = !self.drafts.is_empty();
        let phase = if is_repair {
            ResearchPhase::Revising
        } else {
            ResearchPhase::Writing
        };
        let current = if is_repair {
            self.repair_count as u32 + 1
        } else {
            1
        };
        let total = if is_repair { 2 } else { 1 };
        let summary = if is_repair {
            format!("Revising citations {current} of {total}")
        } else {
            "Writing the research note".into()
        };
        if let Err(reason) = self.emitter.progress(
            phase,
            Some("artifact.markdown.submit"),
            current,
            total,
            self.budget.snapshot(),
            summary,
        ) {
            return self.failed(reason);
        }
        let summaries = self
            .summaries
            .iter()
            .map(|summary| SummaryForSynthesis {
                source_id: summary.source_id.clone(),
                summary: summary.summary.clone(),
            })
            .collect::<Vec<_>>();
        let request = self.repair_request();
        let counter = ConservativeCounter;
        let initial = match pack_synthesis_for_request(
            &summaries,
            &request,
            self.capabilities,
            self.request.framing,
            &counter,
            PackingAttempt::Initial,
        ) {
            Ok(pack) => pack,
            Err(error) => {
                return self.failed(format!("Draft context could not be packed: {error}"))
            }
        };
        let recovery = pack_synthesis_for_request(
            &summaries,
            &request,
            self.capabilities,
            self.request.framing,
            &counter,
            PackingAttempt::Recovery,
        )
        .ok();
        let mut recovery_events = Vec::new();
        let execution = execute_tool_turn(
            self.model,
            self.cancel.clone(),
            self.deadline,
            &mut self.budget,
            ToolTurn {
                expected: ExpectedTool::Markdown,
                initial,
                overflow_recovery: recovery,
            },
            |reason, snapshot| recovery_events.push((reason, snapshot)),
        );
        if let Err(reason) = self.emit_recoveries(phase, recovery_events) {
            return self.failed(reason);
        }
        if let Err(reason) = self.check_boundary() {
            return self.failed(reason);
        }
        let execution = match execution {
            Ok(execution) => execution,
            Err(error) => return self.harness_failed(error),
        };
        let ToolArguments::Markdown { markdown } = execution.call.arguments else {
            return self.failed("Research draft turn returned the wrong tool".into());
        };
        if let Err(reason) = self.emitter.progress(
            ResearchPhase::CheckingCitations,
            None,
            1,
            1,
            self.budget.snapshot(),
            "Checking citation provenance".into(),
        ) {
            return self.failed(reason);
        }
        match verify_citations(&markdown, &self.request.sources) {
            Ok(_) => {
                self.drafts.push(BundleDraft {
                    markdown,
                    citation_status: ArtifactCitationStatus::Verified,
                });
                self.citation_status = Some(ArtifactCitationStatus::Verified);
                self.stage()
            }
            Err(error) => {
                self.drafts.push(BundleDraft {
                    markdown,
                    citation_status: ArtifactCitationStatus::NeedsReview,
                });
                self.diagnostic = Some(error.to_string());
                if is_repair {
                    self.repair_count += 1;
                }
                if self.repair_count < 2 {
                    StepOutcome::Continue
                } else {
                    self.citation_status = Some(ArtifactCitationStatus::NeedsReview);
                    self.stage()
                }
            }
        }
    }

    fn stage(&mut self) -> StepOutcome {
        if let Err(reason) = self.check_boundary() {
            return self.failed(reason);
        }
        if let Err(reason) = self.emitter.progress(
            ResearchPhase::Staging,
            None,
            1,
            1,
            self.budget.snapshot(),
            "Staging the note under this chat".into(),
        ) {
            return self.failed(reason);
        }
        let snapshot = self.budget.snapshot();
        let citation_status = self
            .citation_status
            .unwrap_or(ArtifactCitationStatus::NeedsReview);
        let input = ArtifactBundleInput {
            user_request: self.request.question.clone(),
            provider_id: self.request.provider_id.clone(),
            model_id: self.request.model_id.clone(),
            runtime_id: self.request.runtime_id.clone(),
            sources: self.request.sources.clone(),
            screenshot_sources: self.request.screenshot_sources.clone(),
            summaries: self.summaries.clone(),
            drafts: self.drafts.clone(),
            logical_turns: snapshot.logical_turns,
            provider_calls: snapshot.provider_calls,
            duration_ms: self.started.elapsed().as_millis() as u64,
            outcome: match citation_status {
                ArtifactCitationStatus::Verified => ArtifactOutcome::Complete,
                ArtifactCitationStatus::NeedsReview => ArtifactOutcome::NeedsReview,
            },
        };
        let artifact = match self.store.stage(input) {
            Ok(artifact) => artifact,
            Err(error) => return self.failed(format!("Research artifact was not staged: {error}")),
        };
        if let Err(reason) = self.check_boundary() {
            return self.failed(reason);
        }
        if let Err(reason) = self.emitter.artifact(&artifact, citation_status) {
            return self.failed(reason);
        }
        self.artifact = Some(artifact);
        StepOutcome::Done
    }

    fn repair_request(&self) -> String {
        let Some(last) = self.drafts.last() else {
            return self.request.question.clone();
        };
        let draft = truncate_utf8(&last.markdown, MAX_REPAIR_DRAFT_BYTES);
        format!(
            "{}\n\nRepair the draft so every prose paragraph and list item cites allowed source ids. Diagnostic: {}\n\nPrevious draft:\n{}",
            self.request.question,
            self.diagnostic.as_deref().unwrap_or("citation check failed"),
            draft,
        )
    }

    fn emit_recoveries(
        &mut self,
        phase: ResearchPhase,
        recoveries: Vec<(RecoveryReason, ResearchBudgetSnapshot)>,
    ) -> Result<(), String> {
        for (reason, snapshot) in recoveries {
            self.emitter.recovery(phase, reason, snapshot)?;
        }
        Ok(())
    }

    fn check_boundary(&mut self) -> Result<(), String> {
        if self.cancel.load(Ordering::SeqCst) {
            self.cancelled = true;
            return Err("Research stopped".into());
        }
        if !(self.owner_is_current)() {
            return Err("The chat or project changed during research".into());
        }
        Ok(())
    }

    fn harness_failed(&mut self, error: HarnessError) -> StepOutcome {
        match error {
            HarnessError::Cancelled => {
                self.cancelled = true;
                self.failed("Research stopped".into())
            }
            HarnessError::Budget(error) => {
                self.failed(format!("Research call budget refused: {error:?}"))
            }
            HarnessError::Protocol(code) => {
                self.failed(format!("The model returned invalid tool framing: {code:?}"))
            }
            HarnessError::ContextOverflow => {
                self.failed("The model context remained full after one smaller retry".into())
            }
            HarnessError::Provider(error) => {
                self.failed(format!("The research model failed: {error}"))
            }
        }
    }

    fn failed(&mut self, reason: String) -> StepOutcome {
        self.diagnostic = Some(truncate_utf8(&reason, 2 * 1024).to_string());
        StepOutcome::Failed {
            reason: self.diagnostic.clone().unwrap_or_default(),
        }
    }
}

struct ConservativeCounter;

impl TokenCounter for ConservativeCounter {
    fn count(&self, text: &str) -> u64 {
        estimate_tokens_conservatively(text)
    }
}

struct ResearchEmitter<'a> {
    run_id: String,
    seq: u64,
    terminal: bool,
    emit: &'a mut dyn FnMut(ResearchEventEnvelope),
}

impl<'a> ResearchEmitter<'a> {
    fn new(run_id: String, emit: &'a mut dyn FnMut(ResearchEventEnvelope)) -> Self {
        Self {
            run_id,
            seq: 0,
            terminal: false,
            emit,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn progress(
        &mut self,
        phase: ResearchPhase,
        tool_id: Option<&str>,
        current: u32,
        total: u32,
        budget: ResearchBudgetSnapshot,
        summary: String,
    ) -> Result<(), String> {
        self.push(ResearchEvent::Progress {
            phase,
            tool_id: tool_id.map(str::to_string),
            current,
            total,
            logical_turns: budget.logical_turns,
            provider_calls: budget.provider_calls,
            summary,
        })
    }

    fn recovery(
        &mut self,
        phase: ResearchPhase,
        reason: RecoveryReason,
        budget: ResearchBudgetSnapshot,
    ) -> Result<(), String> {
        let (reason, diagnostic) = match reason {
            RecoveryReason::MalformedFraming => (
                ResearchRecoveryReason::MalformedFraming,
                "Retrying once with the exact tool-call format",
            ),
            RecoveryReason::ContextOverflow => (
                ResearchRecoveryReason::ContextOverflow,
                "Retrying once with a smaller context pack",
            ),
        };
        self.push(ResearchEvent::Recovery {
            phase,
            reason,
            logical_turns: budget.logical_turns,
            provider_calls: budget.provider_calls,
            diagnostic: diagnostic.into(),
        })
    }

    fn artifact(
        &mut self,
        artifact: &ArtifactStageRef,
        status: ArtifactCitationStatus,
    ) -> Result<(), String> {
        self.push(ResearchEvent::Artifact {
            artifact_id: artifact.artifact_id.clone(),
            artifact_version: artifact.artifact_version,
            citation_status: citation_status(status),
        })
    }

    fn terminal(
        &mut self,
        status: ResearchTerminalStatus,
        artifact: Option<&ArtifactStageRef>,
        diagnostic: Option<String>,
    ) {
        if self.terminal {
            return;
        }
        let citation_status = match status {
            ResearchTerminalStatus::Complete => Some(ResearchCitationStatus::Verified),
            ResearchTerminalStatus::NeedsReview => Some(ResearchCitationStatus::NeedsReview),
            ResearchTerminalStatus::Stopped | ResearchTerminalStatus::Failed => None,
        };
        let event = ResearchEvent::Terminal {
            status,
            artifact_id: artifact.map(|value| value.artifact_id.clone()),
            citation_status,
            diagnostic: diagnostic.map(|value| truncate_utf8(&value, 2 * 1024).to_string()),
        };
        if let Ok(envelope) =
            ResearchEventEnvelope::new(self.run_id.clone(), self.seq, now_ms(), event)
        {
            (self.emit)(envelope);
            self.seq = self.seq.saturating_add(1);
        }
        self.terminal = true;
    }

    fn push(&mut self, event: ResearchEvent) -> Result<(), String> {
        if self.terminal {
            return Ok(());
        }
        let envelope = ResearchEventEnvelope::new(self.run_id.clone(), self.seq, now_ms(), event)
            .map_err(|error| format!("Research event was invalid: {error}"))?;
        (self.emit)(envelope);
        self.seq = self.seq.saturating_add(1);
        Ok(())
    }
}

fn finish_early(
    emitter: &mut ResearchEmitter<'_>,
    status: ResearchTerminalStatus,
    diagnostic: &str,
) -> ResearchRunResult {
    emitter.terminal(status, None, Some(diagnostic.into()));
    ResearchRunResult {
        status,
        artifact: None,
        budget: ResearchBudget::default().snapshot(),
        diagnostic: Some(diagnostic.into()),
    }
}

fn citation_status(status: ArtifactCitationStatus) -> ResearchCitationStatus {
    match status {
        ArtifactCitationStatus::Verified => ResearchCitationStatus::Verified,
        ArtifactCitationStatus::NeedsReview => ResearchCitationStatus::NeedsReview,
    }
}

fn truncate_utf8(value: &str, cap: usize) -> &str {
    if value.len() <= cap {
        return value;
    }
    let mut end = cap;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    &value[..end]
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}
