use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::agent::protocol::{ExpectedTool, ProtocolErrorCode, ToolArguments};
use crate::chat::{ChatMessage, ChatRole};
use crate::research::budget::{RecoveryReason, ResearchBudget};
use crate::research::context::{PackedTurn, PackingManifest};
use crate::research::model::{
    ModelCapabilities, ModelFinish, ModelTurnResult, ResearchModelError, ResearchModelPort,
};

use super::{execute_tool_turn, HarnessError, ToolTurn};

struct FakeModel {
    replies: Mutex<VecDeque<ModelTurnResult>>,
    seen: Mutex<Vec<Vec<ChatMessage>>>,
    cancel_during_call: bool,
}

impl FakeModel {
    fn new(replies: Vec<ModelTurnResult>) -> Self {
        Self {
            replies: Mutex::new(replies.into()),
            seen: Mutex::new(Vec::new()),
            cancel_during_call: false,
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
        if self.cancel_during_call {
            cancel.store(true, Ordering::SeqCst);
        }
        Ok(self.replies.lock().unwrap().pop_front().unwrap())
    }
}

fn packed(label: &str, recovery: bool) -> PackedTurn {
    PackedTurn {
        messages: vec![ChatMessage {
            role: ChatRole::User,
            content: label.into(),
        }],
        manifest: PackingManifest {
            context_tokens: 4096,
            prompt_tokens: 10,
            reserved_output_tokens: 1024,
            retained_source_bytes: label.len(),
            original_source_bytes: label.len(),
            truncated: false,
            recovery_repack: recovery,
            included_source_ids: vec!["S1".into()],
        },
    }
}

fn result(text: &str, finish: ModelFinish) -> ModelTurnResult {
    ModelTurnResult {
        text: text.into(),
        prompt_tokens: None,
        output_tokens: None,
        finish,
    }
}

fn summary(summary: &str) -> String {
    format!(
        "<plume_tool_call>{{\"callId\":\"c1\",\"tool\":\"research.summary.submit\",\"arguments\":{{\"sourceId\":\"S1\",\"summary\":\"{summary}\"}}}}</plume_tool_call>"
    )
}

#[test]
fn malformed_reply_gets_one_bounded_reask_then_executes_exact_call() {
    let model = FakeModel::new(vec![
        result("plain prose", ModelFinish::Stop),
        result(&summary("bounded"), ModelFinish::Stop),
    ]);
    let mut budget = ResearchBudget::default();
    budget.begin_logical_turn().unwrap();
    let mut recoveries = Vec::new();
    let execution = execute_tool_turn(
        &model,
        Arc::new(AtomicBool::new(false)),
        Instant::now() + Duration::from_secs(1),
        &mut budget,
        ToolTurn {
            expected: ExpectedTool::Summary { source_id: "S1" },
            initial: packed("initial", false),
            overflow_recovery: Some(packed("smaller", true)),
        },
        |reason, _| recoveries.push(reason),
    )
    .unwrap();
    assert_eq!(recoveries, vec![RecoveryReason::MalformedFraming]);
    assert!(matches!(
        execution.call.arguments,
        ToolArguments::Summary { summary, .. } if summary == "bounded"
    ));
    let seen = model.seen.lock().unwrap();
    assert_eq!(seen.len(), 2);
    assert!(seen[1].last().unwrap().content.contains("envelope"));
    assert!(!seen[1].last().unwrap().content.contains("plain prose"));
}

#[test]
fn overflow_uses_smaller_pack_and_blocks_a_second_recovery_reason() {
    let model = FakeModel::new(vec![
        result("partial", ModelFinish::Length),
        result("still malformed", ModelFinish::Stop),
    ]);
    let mut budget = ResearchBudget::default();
    budget.begin_logical_turn().unwrap();
    let error = execute_tool_turn(
        &model,
        Arc::new(AtomicBool::new(false)),
        Instant::now() + Duration::from_secs(1),
        &mut budget,
        ToolTurn {
            expected: ExpectedTool::Summary { source_id: "S1" },
            initial: packed("initial", false),
            overflow_recovery: Some(packed("smaller", true)),
        },
        |_, _| {},
    )
    .unwrap_err();
    assert_eq!(error, HarnessError::Protocol(ProtocolErrorCode::Envelope));
    let seen = model.seen.lock().unwrap();
    assert_eq!(seen.len(), 2);
    assert_eq!(seen[1][0].content, "smaller");
    assert_eq!(budget.snapshot().provider_calls, 2);
}

#[test]
fn second_malformed_reply_fails_closed_without_a_third_call() {
    let model = FakeModel::new(vec![
        result("bad", ModelFinish::Stop),
        result("bad again", ModelFinish::Stop),
    ]);
    let mut budget = ResearchBudget::default();
    budget.begin_logical_turn().unwrap();
    let error = execute_tool_turn(
        &model,
        Arc::new(AtomicBool::new(false)),
        Instant::now() + Duration::from_secs(1),
        &mut budget,
        ToolTurn {
            expected: ExpectedTool::Summary { source_id: "S1" },
            initial: packed("initial", false),
            overflow_recovery: Some(packed("smaller", true)),
        },
        |_, _| {},
    )
    .unwrap_err();
    assert_eq!(error, HarnessError::Protocol(ProtocolErrorCode::Envelope));
    assert_eq!(model.seen.lock().unwrap().len(), 2);
}

#[test]
fn cancellation_is_checked_before_and_after_the_provider_boundary() {
    let cancel = Arc::new(AtomicBool::new(true));
    let model = FakeModel::new(vec![]);
    let mut budget = ResearchBudget::default();
    budget.begin_logical_turn().unwrap();
    let turn = ToolTurn {
        expected: ExpectedTool::Summary { source_id: "S1" },
        initial: packed("initial", false),
        overflow_recovery: Some(packed("smaller", true)),
    };
    assert_eq!(
        execute_tool_turn(
            &model,
            cancel,
            Instant::now() + Duration::from_secs(1),
            &mut budget,
            turn,
            |_, _| {},
        )
        .unwrap_err(),
        HarnessError::Cancelled
    );
    assert!(model.seen.lock().unwrap().is_empty());

    let model = FakeModel {
        replies: Mutex::new(vec![result(&summary("unused"), ModelFinish::Stop)].into()),
        seen: Mutex::new(Vec::new()),
        cancel_during_call: true,
    };
    let cancel = Arc::new(AtomicBool::new(false));
    let mut budget = ResearchBudget::default();
    budget.begin_logical_turn().unwrap();
    assert_eq!(
        execute_tool_turn(
            &model,
            cancel,
            Instant::now() + Duration::from_secs(1),
            &mut budget,
            ToolTurn {
                expected: ExpectedTool::Summary { source_id: "S1" },
                initial: packed("initial", false),
                overflow_recovery: Some(packed("smaller", true)),
            },
            |_, _| {},
        )
        .unwrap_err(),
        HarnessError::Cancelled
    );
}
