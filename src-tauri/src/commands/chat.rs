//! `chat.send` Tauri command handler — non-streaming subset.
//!
//! D7 ships the sync version of the chat verb documented in
//! `docs/IPC_CONTRACT.md § chat`. The streaming variant
//! (`ChatStreamId` + `chat.token` events + `chat.cancel`) is deferred
//! to D7.1 — see `docs/IPC_ROADMAP.md § Chat streaming`.
//!
//! Validation order (matches the rest of the IPC surface):
//!   1. version
//!   2. payload shape (non-empty model, non-empty messages, no `Tool`
//!      role in v1, last message is from the user)
//!   3. provider id (Ollama-only today)
//!   4. transport — connect, send, parse
//!
//! What this handler deliberately does NOT do:
//!   - It does not validate the model id against the live
//!     `/api/tags` snapshot. The runtime is the source of truth;
//!     pre-checking would just add an extra round-trip and let the
//!     real call race the cache. A bad id returns 404 from Ollama,
//!     which we map onto a typed error here.
//!   - It does not read files, assemble prompts from attachments,
//!     or run the secret redactor. D7 is one-shot text-only chat.
//!     `prompts::assemble` lands when the propose-diff slice does.
//!   - It does not auto-start `ollama serve`. Reachability is the
//!     user's responsibility.

use std::time::{Duration, Instant};

use serde::Deserialize;

use crate::chat::ollama::{self, ChatError};
use crate::chat::{ChatMessage, ChatResponse, ChatRole};
use crate::error::{IpcError, IpcRequest};

/// Default localhost endpoint for Ollama. Same constant as the
/// reachability probe + model-details probe; centralizing port
/// overrides is roadmap (`docs/IPC_ROADMAP.md § Provider health`).
const OLLAMA_HOST: &str = "127.0.0.1";
const OLLAMA_PORT: u16 = 11434;

/// 5-minute timeout on the chat HTTP read. Generation can take a long
/// time on modest hardware — the model-details probe's 1.5 s budget
/// is too short here. Once streaming lands the per-token timeout
/// replaces this whole-call cap.
const CHAT_TIMEOUT: Duration = Duration::from_secs(300);

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatSendPayload {
    pub provider_id: String,
    pub model_id: String,
    pub messages: Vec<ChatMessage>,
}

#[tauri::command]
pub async fn chat_send(req: IpcRequest<ChatSendPayload>) -> Result<ChatResponse, IpcError> {
    req.check_version()?;
    let payload = req.payload;

    validate_payload(&payload)?;

    if payload.provider_id != "ollama" {
        // LM Studio and llama.cpp will share an OpenAI-compatible
        // adapter when their chat path lands; today an attempt to
        // chat against them is honest about not being wired up.
        return Err(IpcError::BadArgument(format!(
            "provider '{}' has no chat adapter yet — only 'ollama' is wired in D7",
            payload.provider_id
        )));
    }

    let model_id = payload.model_id.clone();
    let provider_id = payload.provider_id.clone();
    let messages = payload.messages.clone();

    let started = Instant::now();
    let probe = tauri::async_runtime::spawn_blocking(move || {
        ollama::send_chat(OLLAMA_HOST, OLLAMA_PORT, &model_id, &messages, CHAT_TIMEOUT)
    })
    .await
    .map_err(|e| IpcError::Internal(format!("chat.send task join: {e}")))?;

    let mut response = match probe {
        Ok(resp) => resp,
        Err(ChatError::Transport { host, port, source }) => {
            tracing::debug!(host = %host, port = port, error = %source, "ollama chat transport error");
            return Err(IpcError::ProviderDown {
                provider: "ollama".to_string(),
                reason: format!("could not reach ollama at {host}:{port} ({source})"),
            });
        }
        Err(ChatError::ModelNotFound { model, message }) => {
            return Err(IpcError::BadArgument(format!(
                "model '{model}' not found at ollama: {message}"
            )));
        }
        Err(ChatError::BadStatus { status, message }) => {
            // 4xx → BadArgument (client problem), 5xx → ProviderDown
            // (server problem). Both keep the upstream message so the
            // UI can surface what ollama actually said.
            if (400..500).contains(&status) {
                return Err(IpcError::BadArgument(format!(
                    "ollama rejected the chat (HTTP {status}): {message}"
                )));
            }
            return Err(IpcError::ProviderDown {
                provider: "ollama".to_string(),
                reason: format!("ollama returned HTTP {status}: {message}"),
            });
        }
        Err(ChatError::Parse(msg)) => {
            return Err(IpcError::Internal(format!(
                "ollama chat response did not parse: {msg}"
            )));
        }
    };

    // Adapter leaves duration_ms = 0 because it doesn't time itself;
    // fill it here so the frontend can render "model X · N ms".
    response.duration_ms = started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64;
    response.provider_id = provider_id;
    Ok(response)
}

/// Reject obviously malformed payloads with `BadArgument` before any
/// network call. Each branch is its own clause so the error string
/// names the failing field — the frontend renders the message
/// verbatim in the chat error row.
fn validate_payload(payload: &ChatSendPayload) -> Result<(), IpcError> {
    if payload.provider_id.trim().is_empty() {
        return Err(IpcError::BadArgument(
            "chat.send: providerId is empty".to_string(),
        ));
    }
    if payload.model_id.trim().is_empty() {
        return Err(IpcError::BadArgument(
            "chat.send: modelId is empty — pick a model in the provider panel first".to_string(),
        ));
    }
    if payload.messages.is_empty() {
        return Err(IpcError::BadArgument(
            "chat.send: messages array is empty".to_string(),
        ));
    }
    for (i, m) in payload.messages.iter().enumerate() {
        if m.content.is_empty() {
            return Err(IpcError::BadArgument(format!(
                "chat.send: messages[{i}] has empty content"
            )));
        }
        if matches!(m.role, ChatRole::Tool) {
            // The `Tool` role exists on the wire so a future tool-call
            // loop can carry tool results, but D7 has no tool runtime
            // and shouldn't be talked into one through a hand-rolled
            // payload.
            return Err(IpcError::BadArgument(format!(
                "chat.send: messages[{i}] uses the 'tool' role, which D7 does not support"
            )));
        }
    }
    // The last message must be from the user — otherwise the model
    // would be asked to continue from its own assistant turn, which
    // is rarely what the UI intends. Cheap guard against a frontend
    // bug that forgets to append the new user turn.
    let last = payload.messages.last().expect("non-empty checked above");
    if !matches!(last.role, ChatRole::User) {
        return Err(IpcError::BadArgument(
            "chat.send: last message must have role='user'".to_string(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::{ChatMessage, ChatRole};

    fn user_msg(content: &str) -> ChatMessage {
        ChatMessage {
            role: ChatRole::User,
            content: content.to_string(),
        }
    }

    fn assistant_msg(content: &str) -> ChatMessage {
        ChatMessage {
            role: ChatRole::Assistant,
            content: content.to_string(),
        }
    }

    #[test]
    fn rejects_empty_model_id() {
        let p = ChatSendPayload {
            provider_id: "ollama".into(),
            model_id: "   ".into(),
            messages: vec![user_msg("hi")],
        };
        let err = validate_payload(&p).expect_err("blank model rejected");
        match err {
            IpcError::BadArgument(s) => assert!(s.contains("modelId")),
            other => panic!("expected BadArgument, got {other:?}"),
        }
    }

    #[test]
    fn rejects_empty_messages() {
        let p = ChatSendPayload {
            provider_id: "ollama".into(),
            model_id: "llama3".into(),
            messages: vec![],
        };
        let err = validate_payload(&p).expect_err("empty messages rejected");
        match err {
            IpcError::BadArgument(s) => assert!(s.contains("messages")),
            other => panic!("expected BadArgument, got {other:?}"),
        }
    }

    #[test]
    fn rejects_tool_role_in_v1() {
        let p = ChatSendPayload {
            provider_id: "ollama".into(),
            model_id: "llama3".into(),
            messages: vec![ChatMessage {
                role: ChatRole::Tool,
                content: "tool result".into(),
            }],
        };
        let err = validate_payload(&p).expect_err("tool rejected");
        match err {
            IpcError::BadArgument(s) => assert!(s.contains("tool")),
            other => panic!("expected BadArgument, got {other:?}"),
        }
    }

    #[test]
    fn rejects_when_last_message_is_assistant() {
        // A frontend bug that forwards stale history without appending
        // the new user turn would let the model "complete its own"
        // response. Cheap guard.
        let p = ChatSendPayload {
            provider_id: "ollama".into(),
            model_id: "llama3".into(),
            messages: vec![user_msg("hi"), assistant_msg("hey")],
        };
        let err = validate_payload(&p).expect_err("trailing assistant rejected");
        match err {
            IpcError::BadArgument(s) => assert!(s.contains("user")),
            other => panic!("expected BadArgument, got {other:?}"),
        }
    }

    #[test]
    fn accepts_well_formed_history() {
        let p = ChatSendPayload {
            provider_id: "ollama".into(),
            model_id: "llama3".into(),
            messages: vec![user_msg("hi"), assistant_msg("hey"), user_msg("more")],
        };
        validate_payload(&p).expect("should pass");
    }

    #[test]
    fn rejects_empty_content() {
        let p = ChatSendPayload {
            provider_id: "ollama".into(),
            model_id: "llama3".into(),
            messages: vec![user_msg("")],
        };
        let err = validate_payload(&p).expect_err("empty content rejected");
        match err {
            IpcError::BadArgument(s) => assert!(s.contains("content")),
            other => panic!("expected BadArgument, got {other:?}"),
        }
    }
}
