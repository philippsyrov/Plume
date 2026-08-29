//! Provider-neutral, authority-free model turns for the research harness.

use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::{Duration, Instant};
use std::{error::Error, fmt};

use crate::chat::apple_foundation::{self as apple_chat, StreamOutcome as AppleStreamOutcome};
use crate::chat::mlx_lm::{self as mlx_chat, StreamOutcome as MlxStreamOutcome};
use crate::chat::ChatMessage;
use crate::providers::apple_foundation::{capabilities_with, HelperPort};
use crate::providers::catalog::{QWEN2_VL_CATALOG_ID, QWEN_CATALOG_ID};
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

#[derive(Debug)]
pub(crate) enum ResearchModelError {
    Capabilities(String),
    Apple(apple_chat::AppleChatError),
    Qwen(mlx_chat::ChatError),
    Qwen2Vl(mlx_chat::ChatError),
}

impl fmt::Display for ResearchModelError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Capabilities(message) => {
                write!(
                    formatter,
                    "research model capability query failed: {message}"
                )
            }
            Self::Apple(error) => write!(formatter, "Apple research model turn failed: {error:?}"),
            Self::Qwen(error) => write!(
                formatter,
                "Qwen research model turn failed: {}",
                mlx_chat::format_fixed_catalog_chat_error(error, QWEN_CATALOG_ID)
            ),
            Self::Qwen2Vl(error) => write!(
                formatter,
                "Qwen2-VL research model turn failed: {}",
                mlx_chat::format_fixed_catalog_chat_error(error, QWEN2_VL_CATALOG_ID)
            ),
        }
    }
}

impl Error for ResearchModelError {}

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

pub(crate) struct Qwen2VlMlxResearchModel {
    port: u16,
    model_label: String,
    images: Vec<Vec<u8>>,
}

impl Qwen2VlMlxResearchModel {
    fn from_handle(
        handle: &str,
        images: Vec<Vec<u8>>,
    ) -> Result<Self, ResearchModelSelectionError> {
        let info = lookup_handle_info(&ServerHandleId(handle.to_string()))
            .ok_or(ResearchModelSelectionError::HandleNotFound)?;
        if info.model_id != QWEN2_VL_CATALOG_ID {
            return Err(ResearchModelSelectionError::HandleModelMismatch);
        }
        Ok(Self {
            port: info.port,
            model_label: info.model_label,
            images,
        })
    }
}

impl ResearchModelPort for Qwen2VlMlxResearchModel {
    fn capabilities(&self) -> Result<ModelCapabilities, ResearchModelError> {
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
        let mut text = String::new();
        let outcome = mlx_chat::stream_chat_with_stop_sequences_and_images(
            self.port,
            &self.model_label,
            messages,
            &[mlx_chat::QWEN_CHAT_STOP_SEQUENCE],
            &self.images,
            true,
            cancel,
            |delta| text.push_str(delta),
            MLX_CONNECT_TIMEOUT,
            deadline,
        )
        .map_err(ResearchModelError::Qwen2Vl)?;
        let (prompt_tokens, output_tokens, finish) = match outcome {
            MlxStreamOutcome::Done { stats, .. } => (
                stats.prompt_tokens,
                stats.completion_tokens,
                ModelFinish::Stop,
            ),
            MlxStreamOutcome::Cancelled { .. } => (None, None, ModelFinish::Cancelled),
            MlxStreamOutcome::EofBeforeDone { .. } => (None, None, ModelFinish::Length),
        };
        Ok(ModelTurnResult {
            text,
            prompt_tokens,
            output_tokens,
            finish,
        })
    }
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
    Qwen2Vl(Qwen2VlMlxResearchModel),
}

impl ResearchModelPort for SelectedResearchModel<'_> {
    fn capabilities(&self) -> Result<ModelCapabilities, ResearchModelError> {
        match self {
            Self::Apple(model) => model.capabilities(),
            Self::Qwen(model) => model.capabilities(),
            Self::Qwen2Vl(model) => model.capabilities(),
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
            Self::Qwen2Vl(model) => model.complete(messages, cancel, deadline),
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
    select_model_with_images(
        provider_id,
        model_id,
        handle_id,
        apple_helper,
        os_supported,
        Vec::new(),
    )
}

pub(crate) fn select_model_with_images<'a>(
    provider_id: &str,
    model_id: &str,
    handle_id: Option<&str>,
    apple_helper: Option<&'a dyn HelperPort>,
    os_supported: bool,
    images: Vec<Vec<u8>>,
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
        ("mlx-vlm", QWEN2_VL_CATALOG_ID) => {
            let handle = handle_id.ok_or(ResearchModelSelectionError::MissingHandle)?;
            Ok(SelectedResearchModel::Qwen2Vl(
                Qwen2VlMlxResearchModel::from_handle(handle, images)?,
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
