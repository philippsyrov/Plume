//! Provider-neutral, authority-free model turns for the research harness.

#![allow(dead_code)] // Task 8 wires the completed port into the run harness.

use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::chat::apple_foundation::{self as apple_chat, StreamOutcome as AppleStreamOutcome};
use crate::chat::mlx_lm::{self as mlx_chat, StreamOutcome as MlxStreamOutcome};
use crate::chat::ChatMessage;
use crate::providers::apple_foundation::{capabilities_with, HelperPort};
use crate::providers::catalog::QWEN_CATALOG_ID;
use crate::providers::mlx_lm::{lookup_handle_info, ServerHandleId};

const QWEN_CONSERVATIVE_CONTEXT_TOKENS: u32 = 8_192;
const MLX_CONNECT_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ModelCapabilities {
    pub context_tokens: u32,
    pub exact_token_count: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ModelFinish {
    Stop,
    Cancelled,
    Length,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ModelTurnResult {
    pub text: String,
    pub prompt_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub finish: ModelFinish,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum ResearchModelError {
    #[error("research model capability query failed: {0}")]
    Capabilities(String),
    #[error("Apple research model turn failed: {0:?}")]
    Apple(apple_chat::AppleChatError),
    #[error("Qwen research model turn failed: {0}")]
    Qwen(mlx_chat::ChatError),
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub(crate) enum ResearchModelSelectionError {
    #[error("the selected research model is unsupported")]
    UnsupportedModel,
    #[error("the selected Qwen model requires its Plume-owned runtime handle")]
    MissingHandle,
    #[error("the selected Apple model must not include a runtime handle")]
    UnexpectedHandle,
    #[error("the selected runtime handle is not active")]
    HandleNotFound,
    #[error("the selected runtime handle belongs to a different model")]
    HandleModelMismatch,
    #[error("the bundled Apple model helper is unavailable")]
    HelperUnavailable,
}

pub(crate) trait ResearchModelPort {
    fn capabilities(&self) -> Result<ModelCapabilities, ResearchModelError>;

    fn complete(
        &self,
        messages: &[ChatMessage],
        cancel: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Result<ModelTurnResult, ResearchModelError>;
}

pub(crate) struct AppleResearchModel<'a> {
    helper: &'a dyn HelperPort,
    os_supported: bool,
}

impl<'a> AppleResearchModel<'a> {
    pub(crate) fn new(helper: &'a dyn HelperPort, os_supported: bool) -> Self {
        Self {
            helper,
            os_supported,
        }
    }
}

impl ResearchModelPort for AppleResearchModel<'_> {
    fn capabilities(&self) -> Result<ModelCapabilities, ResearchModelError> {
        capabilities_with(self.helper)
            .map(|capabilities| ModelCapabilities {
                context_tokens: capabilities.context_tokens,
                exact_token_count: capabilities.exact_token_count,
            })
            .map_err(ResearchModelError::Capabilities)
    }

    fn complete(
        &self,
        messages: &[ChatMessage],
        cancel: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Result<ModelTurnResult, ResearchModelError> {
        let turn = apple_chat::collect_turn_with(
            self.helper,
            messages,
            cancel,
            deadline,
            self.os_supported,
        )
        .map_err(ResearchModelError::Apple)?;
        let finish = match turn.outcome {
            AppleStreamOutcome::Done => ModelFinish::Stop,
            AppleStreamOutcome::Cancelled => ModelFinish::Cancelled,
        };
        Ok(ModelTurnResult {
            output_tokens: Some(estimate_tokens_conservatively(&turn.text)),
            text: turn.text,
            prompt_tokens: turn.prompt_tokens,
            finish,
        })
    }
}

pub(crate) struct QwenMlxResearchModel {
    port: u16,
    model_label: String,
}

impl QwenMlxResearchModel {
    fn from_handle(handle: &str) -> Result<Self, ResearchModelSelectionError> {
        let info = lookup_handle_info(&ServerHandleId(handle.to_string()))
            .ok_or(ResearchModelSelectionError::HandleNotFound)?;
        if info.model_id != QWEN_CATALOG_ID {
            return Err(ResearchModelSelectionError::HandleModelMismatch);
        }
        Ok(Self {
            port: info.port,
            model_label: info.model_label,
        })
    }
}

impl ResearchModelPort for QwenMlxResearchModel {
    fn capabilities(&self) -> Result<ModelCapabilities, ResearchModelError> {
        // The managed runtime exposes no verified per-session context/count
        // API. Use a reviewed window smaller than Qwen's published maximum and
        // the same conservative counter as the Apple fallback path.
        Ok(ModelCapabilities {
            context_tokens: QWEN_CONSERVATIVE_CONTEXT_TOKENS,
            exact_token_count: false,
        })
    }

    fn complete(
        &self,
        messages: &[ChatMessage],
        cancel: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Result<ModelTurnResult, ResearchModelError> {
        let turn = mlx_chat::collect_chat_with_stop_sequences(
            self.port,
            &self.model_label,
            messages,
            &[mlx_chat::QWEN_CHAT_STOP_SEQUENCE],
            cancel,
            MLX_CONNECT_TIMEOUT,
            deadline,
        )
        .map_err(ResearchModelError::Qwen)?;
        let (prompt_tokens, output_tokens, finish) = match turn.outcome {
            MlxStreamOutcome::Done { stats, .. } => (
                stats.prompt_tokens,
                stats.completion_tokens,
                ModelFinish::Stop,
            ),
            MlxStreamOutcome::Cancelled { .. } => (None, None, ModelFinish::Cancelled),
            MlxStreamOutcome::EofBeforeDone { .. } => (None, None, ModelFinish::Length),
        };
        Ok(ModelTurnResult {
            text: turn.text,
            prompt_tokens,
            output_tokens,
            finish,
        })
    }
}

pub(crate) enum SelectedResearchModel<'a> {
    Apple(AppleResearchModel<'a>),
    Qwen(QwenMlxResearchModel),
}

impl ResearchModelPort for SelectedResearchModel<'_> {
    fn capabilities(&self) -> Result<ModelCapabilities, ResearchModelError> {
        match self {
            Self::Apple(model) => model.capabilities(),
            Self::Qwen(model) => model.capabilities(),
        }
    }

    fn complete(
        &self,
        messages: &[ChatMessage],
        cancel: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Result<ModelTurnResult, ResearchModelError> {
        match self {
            Self::Apple(model) => model.complete(messages, cancel, deadline),
            Self::Qwen(model) => model.complete(messages, cancel, deadline),
        }
    }
}

pub(crate) fn select_model<'a>(
    provider_id: &str,
    model_id: &str,
    handle_id: Option<&str>,
    apple_helper: Option<&'a dyn HelperPort>,
    os_supported: bool,
) -> Result<SelectedResearchModel<'a>, ResearchModelSelectionError> {
    match (provider_id, model_id) {
        ("apple-foundation", "system") => {
            if handle_id.is_some() {
                return Err(ResearchModelSelectionError::UnexpectedHandle);
            }
            let helper = apple_helper.ok_or(ResearchModelSelectionError::HelperUnavailable)?;
            Ok(SelectedResearchModel::Apple(AppleResearchModel::new(
                helper,
                os_supported,
            )))
        }
        ("mlx-lm", QWEN_CATALOG_ID) => {
            let handle = handle_id.ok_or(ResearchModelSelectionError::MissingHandle)?;
            Ok(SelectedResearchModel::Qwen(
                QwenMlxResearchModel::from_handle(handle)?,
            ))
        }
        _ => Err(ResearchModelSelectionError::UnsupportedModel),
    }
}

/// No adapter-independent tokenizer is trustworthy here. Counting one token
/// per two UTF-8 bytes deliberately overestimates typical English/Markdown,
/// while remaining deterministic for non-ASCII input.
pub(crate) fn estimate_tokens_conservatively(text: &str) -> u64 {
    (text.len() as u64).div_ceil(2)
}
