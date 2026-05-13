//! `chat.cancel` Tauri command handler.
//!
//! Idempotent flip of the per-stream cooperative cancel flag. The
//! streaming loop in `super::send::run_stream` checks the flag
//! between reads; cancelling an unknown or already-terminal stream
//! is a successful no-op so the frontend can fire-and-forget without
//! tracking lifecycle state.

use serde::Deserialize;
use tauri::State;

use crate::commands::project::AppState;
use crate::error::{IpcError, IpcRequest};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatCancelPayload {
    pub stream_id: String,
}

#[tauri::command]
pub async fn chat_cancel(
    req: IpcRequest<ChatCancelPayload>,
    state: State<'_, AppState>,
) -> Result<(), IpcError> {
    req.check_version()?;
    // Idempotent per the contract: cancelling a finished or unknown
    // stream is a successful no-op. The `cancel` return value is
    // only used for tracing here.
    let was_live = state.chat_streams.cancel(&req.payload.stream_id);
    if !was_live {
        tracing::debug!(
            stream = %req.payload.stream_id,
            "chat.cancel: stream id is unknown or already terminal (idempotent no-op)"
        );
    }
    Ok(())
}
