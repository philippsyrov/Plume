//! Bounded provider-neutral model/tool turns for artifact workflows.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

use crate::agent::protocol::{
    build_reask, parse_tool_call, ExpectedTool, ProtocolErrorCode, ToolCall,
};
use crate::chat::{ChatMessage, ChatRole};
use crate::research::budget::{
    BudgetRefusal, RecoveryReason, ResearchBudget, ResearchBudgetSnapshot,
};
use crate::research::context::{PackedTurn, PackingManifest};
use crate::research::model::{ModelFinish, ResearchModelPort};

pub(crate) struct ToolTurn<'a> {
    pub expected: ExpectedTool<'a>,
    pub initial: PackedTurn,
    pub overflow_recovery: Option<PackedTurn>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ToolTurnExecution {
    pub call: ToolCall,
    pub manifest: PackingManifest,
    pub recovery: Option<RecoveryReason>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum HarnessError {
    Cancelled,
    Budget(BudgetRefusal),
    Protocol(ProtocolErrorCode),
    ContextOverflow,
    Provider(String),
}

pub(crate) fn execute_tool_turn(
    model: &dyn ResearchModelPort,
    cancel: Arc<AtomicBool>,
    deadline: Instant,
    budget: &mut ResearchBudget,
    turn: ToolTurn<'_>,
    mut on_recovery: impl FnMut(RecoveryReason, ResearchBudgetSnapshot),
) -> Result<ToolTurnExecution, HarnessError> {
    let first = call_provider(
        model,
        &turn.initial.messages,
        cancel.clone(),
        deadline,
        budget,
    )?;
    match first.finish {
        ModelFinish::Cancelled => return Err(HarnessError::Cancelled),
        ModelFinish::Length => {
            let overflow_recovery = turn
                .overflow_recovery
                .ok_or(HarnessError::ContextOverflow)?;
            reserve_recovery(budget, RecoveryReason::ContextOverflow, &mut on_recovery)?;
            let second =
                call_provider(model, &overflow_recovery.messages, cancel, deadline, budget)?;
            if second.finish != ModelFinish::Stop {
                return Err(match second.finish {
                    ModelFinish::Cancelled => HarnessError::Cancelled,
                    ModelFinish::Length => HarnessError::ContextOverflow,
                    ModelFinish::Stop => unreachable!(),
                });
            }
            let call = parse_tool_call(&second.text, turn.expected)
                .map_err(|error| HarnessError::Protocol(error.code))?;
            return Ok(ToolTurnExecution {
                call,
                manifest: overflow_recovery.manifest,
                recovery: Some(RecoveryReason::ContextOverflow),
            });
        }
        ModelFinish::Stop => {}
    }

    match parse_tool_call(&first.text, turn.expected) {
        Ok(call) => Ok(ToolTurnExecution {
            call,
            manifest: turn.initial.manifest,
            recovery: None,
        }),
        Err(error) => {
            if reserve_recovery(budget, RecoveryReason::MalformedFraming, &mut on_recovery).is_err()
            {
                return Err(HarnessError::Protocol(error.code));
            }
            let mut messages = turn.initial.messages;
            messages.push(ChatMessage {
                role: ChatRole::User,
                content: build_reask(&error, turn.expected),
            });
            let second = call_provider(model, &messages, cancel, deadline, budget)?;
            match second.finish {
                ModelFinish::Cancelled => Err(HarnessError::Cancelled),
                ModelFinish::Length => Err(HarnessError::ContextOverflow),
                ModelFinish::Stop => {
                    let call = parse_tool_call(&second.text, turn.expected)
                        .map_err(|error| HarnessError::Protocol(error.code))?;
                    Ok(ToolTurnExecution {
                        call,
                        manifest: turn.initial.manifest,
                        recovery: Some(RecoveryReason::MalformedFraming),
                    })
                }
            }
        }
    }
}

fn call_provider(
    model: &dyn ResearchModelPort,
    messages: &[ChatMessage],
    cancel: Arc<AtomicBool>,
    deadline: Instant,
    budget: &mut ResearchBudget,
) -> Result<crate::research::model::ModelTurnResult, HarnessError> {
    if cancel.load(Ordering::SeqCst) {
        return Err(HarnessError::Cancelled);
    }
    budget
        .reserve_provider_call()
        .map_err(HarnessError::Budget)?;
    let result = model
        .complete(messages, cancel.clone(), deadline)
        .map_err(|error| HarnessError::Provider(error.to_string()))?;
    if cancel.load(Ordering::SeqCst) {
        return Err(HarnessError::Cancelled);
    }
    Ok(result)
}

fn reserve_recovery(
    budget: &mut ResearchBudget,
    reason: RecoveryReason,
    on_recovery: &mut impl FnMut(RecoveryReason, ResearchBudgetSnapshot),
) -> Result<(), HarnessError> {
    budget
        .reserve_recovery(reason)
        .map_err(HarnessError::Budget)?;
    on_recovery(reason, budget.snapshot());
    Ok(())
}

#[cfg(test)]
#[path = "harness_tests.rs"]
mod tests;
