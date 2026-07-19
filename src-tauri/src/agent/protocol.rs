//! Strict provider-neutral model/tool framing for bounded agent workflows.
//!
//! Providers may format prompts differently, but every text-mode adapter must
//! return exactly one bounded Plume envelope. This parser is the authority:
//! malformed or ambiguous output becomes a typed diagnostic and no partial
//! call escapes to an executor.

#![allow(dead_code)]

use std::fmt;

use serde::Deserialize;

const OPEN_ENVELOPE: &str = "<plume_tool_call>";
const CLOSE_ENVELOPE: &str = "</plume_tool_call>";
const MAX_CALL_ID_BYTES: usize = 64;
const MAX_SUMMARY_BYTES: usize = 16 * 1024;
const MAX_REASK_BYTES: usize = 4096;

pub const MAX_TOOL_REPLY_BYTES: usize = 256 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolId {
    ResearchSummarySubmit,
    ArtifactMarkdownSubmit,
}

impl ToolId {
    fn parse(raw: &str) -> Result<Self, ProtocolError> {
        match raw {
            "research.summary.submit" => Ok(Self::ResearchSummarySubmit),
            "artifact.markdown.submit" => Ok(Self::ArtifactMarkdownSubmit),
            _ => Err(ProtocolError::new(
                ProtocolErrorCode::UnknownTool,
                "the response requested an undisclosed tool",
            )),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolArguments {
    Summary { source_id: String, summary: String },
    Markdown { markdown: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolCall {
    pub call_id: String,
    pub tool: ToolId,
    pub arguments: ToolArguments,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExpectedTool<'a> {
    Summary { source_id: &'a str },
    Markdown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderFraming {
    QwenChatMl,
    AppleInstructions,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolPrompt {
    pub instructions: String,
    pub stop_sequence: Option<&'static str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProtocolErrorCode {
    Oversized,
    Envelope,
    InvalidJson,
    UnknownTool,
    WrongPhase,
    Identity,
    InvalidArguments,
}

impl ProtocolErrorCode {
    fn label(self) -> &'static str {
        match self {
            Self::Oversized => "oversized",
            Self::Envelope => "envelope",
            Self::InvalidJson => "invalid-json",
            Self::UnknownTool => "unknown-tool",
            Self::WrongPhase => "wrong-phase",
            Self::Identity => "identity",
            Self::InvalidArguments => "invalid-arguments",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProtocolError {
    pub code: ProtocolErrorCode,
    detail: &'static str,
}

impl ProtocolError {
    fn new(code: ProtocolErrorCode, detail: &'static str) -> Self {
        Self { code, detail }
    }
}

impl fmt::Display for ProtocolError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.detail)
    }
}

impl std::error::Error for ProtocolError {}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WireCall {
    call_id: String,
    tool: String,
    arguments: serde_json::Value,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WireSummary {
    source_id: String,
    summary: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WireMarkdown {
    markdown: String,
}

pub fn parse_tool_call(raw: &str, expected: ExpectedTool<'_>) -> Result<ToolCall, ProtocolError> {
    if raw.len() > MAX_TOOL_REPLY_BYTES {
        return Err(ProtocolError::new(
            ProtocolErrorCode::Oversized,
            "the model response exceeded the tool-call byte cap",
        ));
    }

    let trimmed = raw.trim();
    if trimmed.matches(OPEN_ENVELOPE).count() != 1 || trimmed.matches(CLOSE_ENVELOPE).count() != 1 {
        return Err(ProtocolError::new(
            ProtocolErrorCode::Envelope,
            "the response must contain exactly one tool-call envelope",
        ));
    }
    let json = trimmed
        .strip_prefix(OPEN_ENVELOPE)
        .and_then(|body| body.strip_suffix(CLOSE_ENVELOPE))
        .ok_or_else(|| {
            ProtocolError::new(
                ProtocolErrorCode::Envelope,
                "only whitespace may appear outside the tool-call envelope",
            )
        })?;
    let wire: WireCall = serde_json::from_str(json).map_err(|_| {
        ProtocolError::new(
            ProtocolErrorCode::InvalidJson,
            "the tool-call envelope did not contain the disclosed JSON shape",
        )
    })?;
    validate_call_id(&wire.call_id)?;

    let tool = ToolId::parse(&wire.tool)?;
    match (expected, tool) {
        (ExpectedTool::Summary { source_id }, ToolId::ResearchSummarySubmit) => {
            let arguments: WireSummary = serde_json::from_value(wire.arguments).map_err(|_| {
                ProtocolError::new(
                    ProtocolErrorCode::InvalidArguments,
                    "the summary arguments did not match the disclosed schema",
                )
            })?;
            if arguments.source_id != source_id {
                return Err(ProtocolError::new(
                    ProtocolErrorCode::Identity,
                    "the summary source did not match the current turn",
                ));
            }
            validate_content(&arguments.summary, MAX_SUMMARY_BYTES, "summary")?;
            Ok(ToolCall {
                call_id: wire.call_id,
                tool,
                arguments: ToolArguments::Summary {
                    source_id: arguments.source_id,
                    summary: arguments.summary,
                },
            })
        }
        (ExpectedTool::Markdown, ToolId::ArtifactMarkdownSubmit) => {
            let arguments: WireMarkdown = serde_json::from_value(wire.arguments).map_err(|_| {
                ProtocolError::new(
                    ProtocolErrorCode::InvalidArguments,
                    "the Markdown arguments did not match the disclosed schema",
                )
            })?;
            validate_content(&arguments.markdown, MAX_TOOL_REPLY_BYTES, "Markdown")?;
            Ok(ToolCall {
                call_id: wire.call_id,
                tool,
                arguments: ToolArguments::Markdown {
                    markdown: arguments.markdown,
                },
            })
        }
        _ => Err(ProtocolError::new(
            ProtocolErrorCode::WrongPhase,
            "the tool was valid but not allowed in the current workflow phase",
        )),
    }
}

pub fn build_reask(error: &ProtocolError, expected: ExpectedTool<'_>) -> String {
    let schema = expected_schema(expected);
    let prompt = format!(
        "Your previous tool response failed with {}. Return exactly one call in this shape and no prose: {schema}",
        error.code.label(),
    );
    truncate_utf8(&prompt, MAX_REASK_BYTES).to_string()
}

pub fn build_tool_prompt(framing: ProviderFraming, expected: ExpectedTool<'_>) -> ToolPrompt {
    let schema = expected_schema(expected);
    let instructions = format!(
        "Return exactly one Plume tool call and no prose, Markdown fence, or second call. Use this exact shape: {schema}"
    );
    let stop_sequence = match framing {
        ProviderFraming::QwenChatMl => Some("<|im_end|>"),
        ProviderFraming::AppleInstructions => None,
    };
    ToolPrompt {
        instructions,
        stop_sequence,
    }
}

fn expected_schema(expected: ExpectedTool<'_>) -> String {
    match expected {
        ExpectedTool::Summary { source_id } => format!(
            "<plume_tool_call>{{\"callId\":\"c1\",\"tool\":\"research.summary.submit\",\"arguments\":{{\"sourceId\":\"{source_id}\",\"summary\":\"...\"}}}}</plume_tool_call>"
        ),
        ExpectedTool::Markdown => concat!(
            "<plume_tool_call>",
            "{\"callId\":\"c1\",\"tool\":\"artifact.markdown.submit\",",
            "\"arguments\":{\"markdown\":\"...\"}}",
            "</plume_tool_call>",
        )
        .to_string(),
    }
}

fn validate_call_id(call_id: &str) -> Result<(), ProtocolError> {
    if call_id.is_empty()
        || call_id.len() > MAX_CALL_ID_BYTES
        || !call_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        return Err(ProtocolError::new(
            ProtocolErrorCode::Identity,
            "the call id was empty, oversized, or contained unsafe characters",
        ));
    }
    Ok(())
}

fn validate_content(
    content: &str,
    byte_cap: usize,
    _label: &'static str,
) -> Result<(), ProtocolError> {
    if content.trim().is_empty() || content.len() > byte_cap {
        return Err(ProtocolError::new(
            ProtocolErrorCode::InvalidArguments,
            "the submitted content was empty or exceeded its byte cap",
        ));
    }
    Ok(())
}

fn truncate_utf8(value: &str, byte_cap: usize) -> &str {
    if value.len() <= byte_cap {
        return value;
    }
    let mut end = byte_cap;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    &value[..end]
}

#[cfg(test)]
#[path = "protocol_tests.rs"]
mod tests;
