//! D45 provider routing for `chat.send`. Extracted from `send.rs`
//! (D118): maps the payload's provider id (+ optional `handleId`)
//! onto a `ChatRoute` at the command-handler boundary, before the
//! streaming task spawns.

use crate::error::IpcError;
use crate::providers::mlx_lm::{self as mlx_supervisor, ServerHandleId};

use super::ChatSendPayload;

/// D45: which adapter to route this send through. Resolved at the
/// command-handler boundary so `run_stream` has a single match instead
/// of re-parsing the provider id mid-stream. `Mlx { port }` carries
/// the bound port from the D40 supervisor's registry — the lookup
/// happens at handler time so a stale `handleId` rejects synchronously
/// with `NotFound`, not as a mid-stream transport error.
#[derive(Debug, Clone)]
pub(super) enum ChatRoute {
    Ollama,
    /// D45 Codex HIGH fix: route carries both the bound port AND
    /// the `--model` label the supervisor launched with. The
    /// payload's `modelId` is the inventory id ("gemma-2b") but
    /// mlx-lm was started with an absolute path; the request's
    /// `model` field must match what the server has loaded, so we
    /// echo the supervisor's `model_label` back on the wire.
    Mlx {
        port: u16,
        model_label: String,
    },
}

/// Resolve the provider id (and optional `handleId`) onto a
/// `ChatRoute`. Three honest outcomes:
///
///   * `"ollama"` — legacy path, no `handleId` required.
///   * `"mlx-lm"` — D40-supervised path. Requires a non-empty
///     `handleId` and a live entry in
///     `providers::mlx_lm::lookup_handle_info`. A stale or missing
///     handle returns `IpcError::NotFound` so the frontend can
///     prompt the user to start (or restart) the server. The
///     lookup also pulls the server's launched-model label out
///     of the registry so the chat request can echo it back as
///     the OpenAI `model` field.
///   * anything else — `BadArgument`. LM Studio and llama.cpp share
///     the OpenAI-compatible adapter once their chat path lands;
///     today the rejection is honest about not being wired up.
pub(super) fn resolve_route(payload: &ChatSendPayload) -> Result<ChatRoute, IpcError> {
    match payload.provider_id.as_str() {
        "ollama" => Ok(ChatRoute::Ollama),
        "mlx-lm" => {
            let raw = payload.handle_id.as_deref().unwrap_or("").trim();
            if raw.is_empty() {
                return Err(IpcError::BadArgument(
                    "chat.send: provider 'mlx-lm' requires handleId — call providers.startServer first".into(),
                ));
            }
            let id = ServerHandleId(raw.to_string());
            match mlx_supervisor::lookup_handle_info(&id) {
                Some(info) => Ok(ChatRoute::Mlx {
                    port: info.port,
                    model_label: info.model_label,
                }),
                None => Err(IpcError::NotFound(format!(
                    "chat.send: no live MLX server with handleId '{raw}'; call providers.startServer and pass the returned id"
                ))),
            }
        }
        other => Err(IpcError::BadArgument(format!(
            "provider '{other}' has no chat adapter yet — only 'ollama' and 'mlx-lm' are wired"
        ))),
    }
}
