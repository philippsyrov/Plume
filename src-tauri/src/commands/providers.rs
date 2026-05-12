//! `providers.list`, `providers.health`, and `providers.modelDetails`
//! Tauri command handlers.
//!
//! See `docs/IPC_CONTRACT.md` § providers for the wire shapes and
//! `docs/MODEL_PROVIDERS.md § Runtime categories` for what the
//! `category` field means.
//!
//! None of these verbs require an open project. The registry is
//! global, reachability is global state about local daemons, and
//! `providers.modelDetails` reads model-info from those daemons —
//! all without touching the project tree. UI surfaces them inside
//! the project view, but the backend doesn't gate them on trust.

use std::time::Duration;

use serde::Deserialize;

use crate::commands::project::EmptyPayload;
use crate::error::{IpcError, IpcRequest};
use crate::providers::{
    default_providers, fit::estimate_fit, ollama, probe_all, ProviderHealth, ProviderInfo,
    ProviderModelDetails, ProviderModelInfo,
};
use crate::system;

/// Per-provider HTTP probe budget for the model-details fetch. Same
/// envelope as the health module so a stalled daemon never hangs the
/// UI; the call is also routed through `spawn_blocking` to keep the
/// async runtime free.
const MODEL_DETAILS_TIMEOUT: Duration = Duration::from_millis(1500);

/// Connected runtimes the model-details verb knows how to ask. Mirrors
/// the table in `providers::health` — keep them in sync, both are
/// keyed on the static registry's `id`.
const CONNECTED_DETAIL_PROBES: &[(&str, &str, u16)] = &[("ollama", "127.0.0.1", 11434)];

#[tauri::command]
pub async fn providers_list(req: IpcRequest<EmptyPayload>) -> Result<Vec<ProviderInfo>, IpcError> {
    req.check_version()?;
    Ok(default_providers())
}

#[tauri::command]
pub async fn providers_health(
    req: IpcRequest<EmptyPayload>,
) -> Result<Vec<ProviderHealth>, IpcError> {
    req.check_version()?;
    Ok(probe_all().await)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelDetailsPayload {
    pub provider_id: String,
    pub model_id: String,
}

/// Lazy model-truth probe. The frontend calls this when the user
/// expands a model row in the provider panel; we deliberately do not
/// pre-fetch every model on `providers.health` so the panel stays
/// cheap on refresh.
///
/// Behavior:
///   - Unknown provider id ⇒ `BadArgument`. The frontend should not
///     be calling this verb for a runtime Plume has not registered.
///   - Provider we know but cannot HTTP-probe today (MLX-LM,
///     llama.cpp, LM Studio) ⇒ `BadArgument` for now; their probes
///     land with their adapters.
///   - HTTP probe fails ⇒ details = None, fit = Unknown, but the
///     verb still succeeds — the UI shows a "couldn't read details"
///     message and the fit estimate is honest about why.
#[tauri::command]
pub async fn providers_model_details(
    req: IpcRequest<ModelDetailsPayload>,
) -> Result<ProviderModelDetails, IpcError> {
    req.check_version()?;
    let payload = req.payload;

    let Some((_, host, port)) = CONNECTED_DETAIL_PROBES
        .iter()
        .find(|(id, _, _)| *id == payload.provider_id)
    else {
        return Err(IpcError::BadArgument(format!(
            "provider '{}' does not have a model-details probe yet",
            payload.provider_id
        )));
    };
    let host = host.to_string();
    let port = *port;
    let model_id = payload.model_id.clone();

    // Run the HTTP probe on the blocking pool; std::net is sync.
    let probe = tauri::async_runtime::spawn_blocking(move || {
        ollama::probe_model_details(&host, port, &model_id, MODEL_DETAILS_TIMEOUT)
    })
    .await
    .map_err(|e| IpcError::Internal(format!("model-details task join: {e}")))?;

    let machine_ram_bytes = system::physical_memory_bytes();

    match probe {
        Ok(raw) => {
            let fit = estimate_fit(
                raw.parameter_count,
                raw.quantization.as_deref(),
                machine_ram_bytes,
            );
            Ok(ProviderModelDetails {
                provider_id: payload.provider_id.clone(),
                model_id: payload.model_id,
                details: Some(ProviderModelInfo {
                    format: raw.format,
                    family: raw.family,
                    parameter_size: raw.parameter_size,
                    parameter_count: raw.parameter_count,
                    quantization: raw.quantization,
                    context_length: raw.context_length,
                    capabilities: raw.capabilities,
                }),
                fit,
                runtime_path: runtime_path_for(&payload.provider_id),
            })
        }
        Err(err) => {
            tracing::debug!(provider = %payload.provider_id, model = %payload.model_id, error = %err, "model-details probe failed");
            // Still return a snapshot; the UI shows "couldn't read"
            // and the fit estimate reflects that we lack inputs.
            let fit = estimate_fit(None, None, machine_ram_bytes);
            Ok(ProviderModelDetails {
                provider_id: payload.provider_id.clone(),
                model_id: payload.model_id,
                details: None,
                fit,
                runtime_path: runtime_path_for(&payload.provider_id),
            })
        }
    }
}

/// Hand-written runtime-path label per provider. These come from
/// `docs/MODEL_PROVIDERS.md` — Ollama on Mac is GGUF/Metal today, not
/// MLX, and the UI must say so. Adding a new label here is part of
/// landing a new adapter.
fn runtime_path_for(provider_id: &str) -> Option<String> {
    match provider_id {
        "ollama" => {
            // Match the runtime-honesty wording in
            // `docs/MODEL_PROVIDERS.md § Ollama`. macOS today serves
            // GGUF through Metal; if Ollama's MLX preview becomes
            // default we revise this label.
            #[cfg(target_os = "macos")]
            {
                Some("GGUF / Metal (Ollama)".into())
            }
            #[cfg(not(target_os = "macos"))]
            {
                Some("GGUF (Ollama)".into())
            }
        }
        _ => None,
    }
}
