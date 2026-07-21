//! `providers.list`, `providers.health`, `providers.modelDetails`,
//! `providers.localModels`, `providers.startServer`, `providers.catalogStart`,
//! `providers.stopServer` Tauri command handlers.
//!
//! See `docs/IPC_CONTRACT.md` § providers for the wire shapes and
//! `docs/MODEL_PROVIDERS.md § Runtime categories` for what the
//! `category` field means.
//!
//! Trust posture:
//!
//!   * `providers.list`, `providers.health`, `providers.localModels`,
//!     `providers.modelDetails` — read-only / introspection, NO
//!     trust gate. The registry is global, reachability is global
//!     state about local daemons, model details are
//!     daemon-introspection. None of these spawn a subprocess or
//!     touch the project tree.
//!   * `providers.startServer` — requires a trusted open project
//!     (D40 Codex HIGH fix). The verb spawns `python -m mlx_lm
//!     server …`; shell command execution sits behind the same
//!     trust gate as `memory.remember` / `patch.apply`.
//!   * `providers.catalogStart` — starts only fixed receipt-backed Plume
//!     catalog models with a backend-resolved runtime. It has no project input
//!     and does not read project trust.
//!   * `providers.stopServer` — no trust gate. Stopping a process
//!     Plume already spawned is a cleanup verb; we don't want a
//!     revoked-trust window to strand an orphaned child.

use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard, TryLockError};
use std::time::Duration;

use serde::Deserialize;
use tauri::{Manager, State};

use crate::commands::project::{AppState, EmptyPayload};
use crate::error::{IpcError, IpcRequest};
use crate::project::OpenProject;
use crate::providers::apple_foundation::{
    availability_with, platform_supports_apple_models, AppleAvailability, AppleAvailabilityReason,
    NativeHelperPort,
};
use crate::providers::catalog::{apply_apple_availability, CatalogStore};
use crate::providers::catalog_download::{CatalogDownloadRegistry, CatalogStartReservation};
use crate::providers::local_model_details::{self, LocalModelDetails, LocalModelDetailsError};
use crate::providers::mlx_lm::{
    self, ManagedServerInfo, ServerDiagnostics, ServerHandle, ServerHandleId, StartError, StopError,
};
use crate::providers::{
    default_providers, fit::estimate_fit, local_models, mlx_runtime, ollama, probe_all,
    CatalogEntry, LocalModel, ProviderHealth, ProviderInfo, ProviderModelDetails,
    ProviderModelInfo,
};
use crate::system;

/// Per-provider HTTP probe budget for the model-details fetch. Same
/// envelope as the health module so a stalled daemon never hangs the
/// UI; the call is also routed through `spawn_blocking` to keep the
/// async runtime free.
const MODEL_DETAILS_TIMEOUT: Duration = Duration::from_millis(1500);

/// One fixed catalog model may cross the check-and-spawn boundary at a time.
/// The supervisor owns the durable Starting/Running state after this short
/// command-level gate is released.
static CATALOG_START_GATE: Mutex<()> = Mutex::new(());

/// Connected runtimes the model-details verb knows how to ask. Mirrors
/// the table in `providers::health` — keep them in sync, both are
/// keyed on the static registry's `id`.
const CONNECTED_DETAIL_PROBES: &[(&str, &str, u16)] = &[("ollama", "127.0.0.1", 11434)];

#[path = "providers_catalog_download.rs"]
mod catalog_download;
pub use catalog_download::{
    providers_catalog_download, providers_catalog_download_cancel, providers_catalog_remove,
};

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

/// List Plume's fixed app-level model catalog. It never downloads, selects, or
/// starts a model. It performs one bounded availability check through the
/// bundled Apple helper so the system-model row can report its current state;
/// this app-level introspection does not require project trust.
#[tauri::command]
pub async fn providers_catalog_list(
    req: IpcRequest<EmptyPayload>,
    state: State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<Vec<CatalogEntry>, IpcError> {
    req.check_version()?;
    let store = state.catalog_store.clone();
    let helper = resolve_apple_helper(&app);
    tauri::async_runtime::spawn_blocking(move || {
        let availability = match helper {
            Ok(helper) => availability_with(&helper, true),
            Err(availability) => availability,
        };
        let mut entries = store.list()?;
        apply_apple_availability(&mut entries, &availability);
        Ok::<Vec<CatalogEntry>, crate::providers::catalog::CatalogError>(entries)
    })
    .await
    .map_err(|e| IpcError::Internal(format!("providers.catalogList task join: {e}")))?
    .map_err(|e| IpcError::Internal(e.to_string()))
}

/// Check the Apple system model without requiring a trusted project. The only
/// executable it can resolve is the bundled helper under Tauri resources.
#[tauri::command]
pub async fn providers_apple_availability(
    req: IpcRequest<EmptyPayload>,
    app: tauri::AppHandle,
) -> Result<AppleAvailability, IpcError> {
    req.check_version()?;
    let helper = resolve_apple_helper(&app);
    tauri::async_runtime::spawn_blocking(move || match helper {
        Ok(helper) => availability_with(&helper, true),
        Err(availability) => availability,
    })
    .await
    .map_err(|error| IpcError::Internal(format!("providers.appleAvailability task join: {error}")))
}

fn resolve_apple_helper(app: &tauri::AppHandle) -> Result<NativeHelperPort, AppleAvailability> {
    if !platform_supports_apple_models() {
        return Err(AppleAvailability::unavailable(
            AppleAvailabilityReason::OsUnsupported,
        ));
    }
    app.path()
        .resource_dir()
        .ok()
        .map(|resources| NativeHelperPort::from_resource_dir(&resources))
        .ok_or_else(|| AppleAvailability::unavailable(AppleAvailabilityReason::Failed))
}

#[tauri::command]
pub async fn providers_local_models(
    req: IpcRequest<EmptyPayload>,
) -> Result<Vec<LocalModel>, IpcError> {
    req.check_version()?;
    // D50: scan every configured source (PlumeModelDir + read-only
    // external caches like Locally AI's HF cache + LM Studio). Each
    // entry's `source` field names where it came from; ids are
    // source-prefixed so downstream resolvers can route back without
    // a separate `source` parameter on the wire.
    tauri::async_runtime::spawn_blocking(local_models::scan_all_sources)
        .await
        .map_err(|e| IpcError::Internal(format!("local-model scan task join: {e}")))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LocalModelDetailsPayload {
    /// Project-relative id from `providers.localModels`. The handler
    /// resolves this against `default_model_dir()` and refuses any
    /// shape that escapes the model directory or traverses a
    /// symlink. The frontend should pass the `id` field verbatim
    /// from the inventory row it just rendered.
    pub id: String,
}

/// D41: read on-disk details for a single local-model entry.
/// Backend-only; no daemon HTTP, no model load. Same read-only
/// posture as `providers.localModels`: the verb does not require a
/// trusted project (the local-model directory is global) and never
/// writes.
///
/// **Inventory verification (Codex D41 MEDIUM fix).** The handler
/// runs `scan_model_dir` first and requires `payload.id` to match
/// an entry the scanner surfaced. Without this gate, any regular
/// file under the model dir (a `README.md`, a stray `.txt`) would
/// resolve through `read_local_model_details` and come back as a
/// `weightFileCount: 1` "model" — the resolver's only check was
/// path safety. The inventory is the source of truth for what
/// counts as a model; details are an enrichment on top of an
/// existing row, not a free-form filesystem probe.
///
/// Failure mapping (stable to the frontend):
///   * `payload.id` not in `scan_model_dir` output, or absent on
///     disk → `IpcError::NotFound`. Frontend should refetch
///     `providers.localModels`.
///   * `LocalModelDetailsError::PathEscape` → `IpcError::PathEscape`.
///     A corrupt id or planted symlink — treat as a security failure.
///   * `LocalModelDetailsError::Io(_)` → `IpcError::Internal`. The
///     frontend renders a "couldn't read details" hint.
#[tauri::command]
pub async fn providers_local_model_details(
    req: IpcRequest<LocalModelDetailsPayload>,
) -> Result<LocalModelDetails, IpcError> {
    req.check_version()?;
    let payload = req.payload;
    tauri::async_runtime::spawn_blocking(move || {
        // D50: parse the source prefix off `payload.id` to locate the
        // right root for this entry. An id without a known source tag
        // (older callers, corrupt id, etc.) is a stale id — surface as
        // NotFound the same way an inventory miss is, since the user
        // can recover by refreshing.
        let Some((source, relative)) = local_models::parse_inventory_id(&payload.id) else {
            return Err(LocalModelDetailsError::NotFound);
        };
        let Some(source_root) = local_models::source_root_for(source) else {
            // External source's root dir doesn't exist on this host —
            // the entry can't have come from here, so it's stale.
            return Err(LocalModelDetailsError::NotFound);
        };
        // Codex D41 MEDIUM: verify the id is a real inventory entry
        // before reading details. We re-scan the SAME source the id
        // claims to come from (cheap; one root) and match by full id
        // — the prefix is part of the equality check, so a forged
        // prefix pointing at the wrong source can't match.
        let inventory = local_models::scan_source(&source_root, source);
        if !inventory.iter().any(|m| m.id == payload.id) {
            return Err(LocalModelDetailsError::NotFound);
        }
        local_model_details::read_local_model_details(&source_root, relative)
    })
    .await
    .map_err(|e| IpcError::Internal(format!("local-model details task join: {e}")))?
    .map_err(|err| match err {
        LocalModelDetailsError::NotFound => IpcError::NotFound(
            "local model not found; refresh providers.localModels and retry".into(),
        ),
        LocalModelDetailsError::PathEscape => {
            IpcError::PathEscape("local model id resolved outside the model directory".into())
        }
        LocalModelDetailsError::Io(e) => {
            IpcError::Internal(format!("local-model details read failed: {e}"))
        }
    })
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

// --- D40: providers.startServer / providers.stopServer -------------------

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StartServerPayload {
    /// Today only `"mlx-lm"` is accepted. Other ids reject with
    /// `BadArgument` until their adapter lands.
    pub provider_id: String,
    /// Inventory id from `providers.localModels`. Resolved against
    /// the same model directory the inventory scans. The handler
    /// rejects shapes that escape the model directory or fail the
    /// scanner's symlink defense.
    pub model_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StopServerPayload {
    /// Handle id from a prior `providers.startServer` response.
    pub handle_id: String,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StopServerResponse {
    pub ok: bool,
}

/// D40: spawn a Plume-managed mlx-lm server for the given local
/// model. Backend-only; no chat routing yet. The handler:
///
///   1. Requires a trusted open project (Codex D40 HIGH fix).
///      Spawning `python -m mlx_lm server …` is shell command
///      execution; Plume's safety contract refuses to do that for
///      any unaudited project. The model directory may be a global
///      inventory, but the *act* of running a subprocess on the
///      user's machine is what the trust gate covers. No trust →
///      `NeedsApproval`, same shape as `memory.remember` /
///      `patch.apply`.
///   2. Validates `providerId == "mlx-lm"` (other ids reject).
///   3. Resolves `modelId` against the local-model directory; only
///      `mlx-folder` and `transformer-folder` entries are accepted
///      so callers don't promise a path the runtime can't actually
///      consume.
///   4. Resolves the app runtime, then spawns the non-deprecated
///      `-m mlx_lm server …` launcher with an OS-allocated port.
///   5. Polls `/health` until the overall startup deadline.
///   6. Returns the handle + the bound port + the child PID.
///
/// Errors surface as typed `IpcError` so the frontend can switch
/// on the stable shape: `BadArgument` for input rejection,
/// `NotFound` for stale inventory rows, `Internal` for spawn /
/// health failures (with the captured stderr tail in the message
/// for diagnostics).
#[tauri::command]
pub async fn providers_start_server(
    req: IpcRequest<StartServerPayload>,
    state: State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<ServerHandle, IpcError> {
    req.check_version()?;
    // Trust gate before any other validation. The check has nothing
    // to do with the model directory — it's gating the act of
    // spawning a subprocess on the user's behalf. An untrusted (or
    // closed) project means we refuse to spawn full stop.
    generic_start_gate(trusted_open_project(&state))?;

    let payload = req.payload;
    if payload.provider_id != "mlx-lm" {
        return Err(IpcError::BadArgument(format!(
            "providers.startServer: provider '{}' is not yet supervised; only 'mlx-lm' is supported in D40",
            payload.provider_id
        )));
    }
    let command = resolve_app_mlx_runtime(&app)?;

    tauri::async_runtime::spawn_blocking(move || {
        let model_path = resolve_local_model_path(&payload.model_id)?;
        mlx_lm::start_server_with_command(command, &model_path, &payload.model_id)
            .map_err(|error| start_error_to_ipc("providers.startServer", error))
    })
    .await
    .map_err(|e| IpcError::Internal(format!("providers.startServer task join: {e}")))?
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CatalogStartPayload {
    /// Fixed catalog id. Only receipt-backed Plume catalog models are accepted; callers never
    /// provide a model path, Python interpreter, or launch arguments.
    pub catalog_id: String,
}

/// Start a Plume-installed catalog model without an open project. This
/// is app-level model management, not a project command: its only accepted
/// input is the fixed catalog id, the model path is receipt-validated under
/// app data, and the release interpreter is resolved under app resources.
#[tauri::command]
pub async fn providers_catalog_start(
    req: IpcRequest<CatalogStartPayload>,
    state: State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<ServerHandle, IpcError> {
    req.check_version()?;
    let catalog_id = req.payload.catalog_id;
    if crate::providers::catalog::catalog_revision(&catalog_id).is_none() {
        return Err(IpcError::BadArgument(
            "providers.catalogStart: only fixed Plume catalog models can be started".into(),
        ));
    }
    let command = configure_catalog_command(resolve_app_mlx_runtime(&app)?, &catalog_id);
    let store = state.catalog_store.clone();
    let downloads = state.catalog_downloads.clone();

    tauri::async_runtime::spawn_blocking(move || {
        let _start_guard = try_catalog_start_guard(&CATALOG_START_GATE)?;
        start_catalog_model_with(
            &store,
            &downloads,
            &catalog_id,
            any_catalog_model_is_reserved,
            |model_path, reservation| {
                mlx_lm::start_server_with_command_after_reservation(
                    command,
                    model_path,
                    &catalog_id,
                    move || reservation.release_after_starting_reservation(),
                )
                .map_err(|error| start_error_to_ipc("providers.catalogStart", error))
            },
        )
    })
    .await
    .map_err(|error| IpcError::Internal(format!("providers.catalogStart task join: {error}")))?
}

fn configure_catalog_command(
    mut command: mlx_lm::process::MlxCommand,
    catalog_id: &str,
) -> mlx_lm::process::MlxCommand {
    if catalog_id != crate::providers::catalog::QWEN2_VL_CATALOG_ID {
        return command;
    }
    let module_index = command
        .args_prefix
        .iter()
        .position(|argument| argument == "-m")
        .unwrap_or(command.args_prefix.len());
    command.args_prefix.truncate(module_index);
    command
        .args_prefix
        .extend(["-m".into(), "plume_mlx_vlm_server".into()]);
    command
}

/// Keep catalog path validation and the start reservation under one lifecycle
/// gate. The supplied starter must release `reservation` only after it has
/// recorded its own durable `Starting` state; the production MLX wrapper does
/// that before it begins the potentially slow health poll.
fn start_catalog_model_with<T>(
    store: &CatalogStore,
    downloads: &CatalogDownloadRegistry,
    catalog_id: &str,
    catalog_model_reserved: impl FnOnce() -> bool,
    starter: impl FnOnce(&std::path::Path, CatalogStartReservation) -> Result<T, IpcError>,
) -> Result<T, IpcError> {
    let reservation = downloads
        .begin_catalog_start_for_store(store, catalog_id)
        .map_err(catalog_start_error_to_ipc)?;
    if catalog_model_reserved() {
        return Err(IpcError::BadArgument(
            "providers.catalogStart: stop the running Plume model before starting another".into(),
        ));
    }
    let model_path = store.installed_model_path(catalog_id).ok_or_else(|| {
        IpcError::NotFound(
            "providers.catalogStart: this model is not installed with a valid receipt; download it again before starting"
                .into(),
        )
    })?;
    starter(&model_path, reservation)
}

fn any_catalog_model_is_reserved() -> bool {
    [
        crate::providers::catalog::QWEN_CATALOG_ID,
        crate::providers::catalog::QWEN2_VL_CATALOG_ID,
    ]
    .into_iter()
    .any(mlx_lm::catalog_model_is_reserved)
}

fn try_catalog_start_guard(gate: &Mutex<()>) -> Result<MutexGuard<'_, ()>, IpcError> {
    match gate.try_lock() {
        Ok(guard) => Ok(guard),
        Err(TryLockError::WouldBlock) => Err(IpcError::BadArgument(
            "providers.catalogStart: another Plume model is already starting".into(),
        )),
        Err(TryLockError::Poisoned(_)) => Err(IpcError::Internal(
            "providers.catalogStart lifecycle gate is unavailable".into(),
        )),
    }
}

fn catalog_start_error_to_ipc(
    error: crate::providers::catalog_download::DownloadError,
) -> IpcError {
    match error {
        crate::providers::catalog_download::DownloadError::UnsupportedCatalog(_) => {
            IpcError::BadArgument(
                "providers.catalogStart: only fixed Plume catalog models can be started".into(),
            )
        }
        crate::providers::catalog_download::DownloadError::OperationActive { .. } => {
            IpcError::BadArgument(
                "providers.catalogStart: another catalog lifecycle operation is active".into(),
            )
        }
        other => IpcError::Internal(format!("providers.catalogStart lifecycle gate: {other}")),
    }
}

fn generic_start_gate(project: Option<OpenProject>) -> Result<OpenProject, IpcError> {
    project.ok_or(IpcError::NeedsApproval)
}

fn resolve_app_mlx_runtime(
    app: &tauri::AppHandle,
) -> Result<mlx_lm::process::MlxCommand, IpcError> {
    let resource_dir = app
        .path()
        .resource_dir()
        .map_err(|error| IpcError::Internal(format!("providers runtime resource path: {error}")))?;
    mlx_runtime::resolve_mlx_runtime(&resource_dir, cfg!(debug_assertions))
        .map_err(|error| IpcError::Internal(format!("providers runtime resolution: {error}")))
}

/// Same shape as `memory::trusted_open` and `patch::trusted_open`.
/// Pulled into a local helper so future provider verbs that also
/// need to gate on trust can reuse it without a circular dep on
/// `commands::memory`.
fn trusted_open_project(state: &AppState) -> Option<OpenProject> {
    let open = state.session.current()?;
    let trusted = {
        let store = state.trust.lock().expect("trust mutex poisoned");
        store.is_trusted(&open.root)
    };
    if trusted {
        Some(open)
    } else {
        None
    }
}

/// D40: stop a previously-started Plume-managed server.
///
/// No trust gate: stopping a process Plume already spawned is a
/// cleanup verb. If the user revoked trust mid-session we still
/// want them to be able to shut down what's running rather than
/// leaving an orphaned `python` process. UnknownHandle covers the
/// case where the handle id is bogus.
#[tauri::command]
pub async fn providers_stop_server(
    req: IpcRequest<StopServerPayload>,
) -> Result<StopServerResponse, IpcError> {
    req.check_version()?;
    let handle_id = ServerHandleId(req.payload.handle_id);
    tauri::async_runtime::spawn_blocking(move || mlx_lm::stop_server(&handle_id))
        .await
        .map_err(|e| IpcError::Internal(format!("providers.stopServer task join: {e}")))?
        .map(|_| StopServerResponse { ok: true })
        .map_err(|err| match err {
            StopError::UnknownHandle => IpcError::NotFound(
                "providers.stopServer: no live server with that handle id".into(),
            ),
            StopError::Io(e) => IpcError::Internal(format!("providers.stopServer: {e}")),
        })
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListServersResponse {
    pub servers: Vec<ManagedServerInfo>,
}

/// Thermos I1: list every server THIS Plume process currently
/// manages. The recovery verb for frontend handle loss — a webview
/// reload / remount that skipped the unmount stop can re-key its
/// per-model bookkeeping from the returned `modelId`/`handleId`
/// pairs instead of stranding a running child it can no longer
/// stop. Read-only; never mutates the registry. No trust gate (same
/// posture as `providers.stopServer` / `providers.serverDiagnostics`):
/// every listed process was already trust-gated at start, and a
/// revoked-trust window must not hide a running child from cleanup.
/// The response is bounded by the supervisor's
/// `MAX_MANAGED_SERVERS` cap and claims nothing beyond children this
/// process itself spawned.
#[tauri::command]
pub async fn providers_list_servers(
    req: IpcRequest<EmptyPayload>,
) -> Result<ListServersResponse, IpcError> {
    req.check_version()?;
    tauri::async_runtime::spawn_blocking(|| ListServersResponse {
        servers: mlx_lm::list_managed_servers(),
    })
    .await
    .map_err(|e| IpcError::Internal(format!("providers.listServers task join: {e}")))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ServerDiagnosticsPayload {
    /// Handle id from a prior `providers.startServer` response.
    pub handle_id: String,
}

/// D52: read a diagnostics snapshot for a running Plume-managed
/// server. Returns the current port + pid + model label + uptime + a
/// log tail (last ~16 KiB of mlx-lm's stdout+stderr). Read-only — the
/// verb never mutates the process registry. No trust gate (same
/// posture as `providers.stopServer`): the user already started the
/// process from a trusted session; surfacing its diagnostics is part
/// of the cleanup / inspection contract.
///
/// `NotFound` when the handle id is unknown (never issued, already
/// stopped, belongs to a different Plume instance) so the panel can
/// drop the disclosure without surfacing a confusing error.
#[tauri::command]
pub async fn providers_server_diagnostics(
    req: IpcRequest<ServerDiagnosticsPayload>,
) -> Result<ServerDiagnostics, IpcError> {
    req.check_version()?;
    let handle_id = ServerHandleId(req.payload.handle_id);
    tauri::async_runtime::spawn_blocking(move || mlx_lm::lookup_diagnostics(&handle_id))
        .await
        .map_err(|e| IpcError::Internal(format!("providers.serverDiagnostics task join: {e}")))?
        .ok_or_else(|| {
            IpcError::NotFound(
                "providers.serverDiagnostics: no live server with that handle id".into(),
            )
        })
}

/// Resolve a `LocalModel.id` to the absolute path the supervisor
/// hands to `--model`. D50: scans the SAME source the id claims to
/// come from (parsed off the id prefix); a stale id that references a
/// missing/unknown source surfaces as `NotFound` so the panel can
/// refresh and recover. Rejects ids whose kind is `gguf` /
/// `safetensors` (single-file kinds aren't what mlx_lm.server
/// consumes) so the caller doesn't promise a path mlx-lm can't load.
fn resolve_local_model_path(model_id: &str) -> Result<PathBuf, IpcError> {
    let Some((source, _relative)) = local_models::parse_inventory_id(model_id) else {
        return Err(IpcError::NotFound(format!(
            "providers.startServer: local model '{model_id}' has no known source prefix; refresh providers.localModels and retry"
        )));
    };
    let Some(source_root) = local_models::source_root_for(source) else {
        return Err(IpcError::NotFound(format!(
            "providers.startServer: local model '{model_id}' source root not present on this host; refresh providers.localModels and retry"
        )));
    };
    let inventory = local_models::scan_source(&source_root, source);
    let entry = inventory.into_iter().find(|m| m.id == model_id).ok_or_else(|| {
        IpcError::NotFound(format!(
            "providers.startServer: local model '{model_id}' not in inventory; refresh providers.localModels and retry"
        ))
    })?;
    match entry.kind {
        local_models::LocalModelKind::MlxFolder
        | local_models::LocalModelKind::TransformerFolder => Ok(PathBuf::from(entry.path)),
        local_models::LocalModelKind::Gguf | local_models::LocalModelKind::Safetensors => {
            Err(IpcError::BadArgument(format!(
                "providers.startServer: model '{}' is a single-file kind ({:?}) and cannot be loaded by mlx_lm.server; point at a transformer- or mlx-folder instead",
                model_id, entry.kind
            )))
        }
    }
}

fn start_error_to_ipc(action: &str, err: StartError) -> IpcError {
    match err {
        StartError::InvalidModelPath => {
            IpcError::BadArgument(format!("{action}: model_path is empty"))
        }
        StartError::PortAllocation(e) => {
            IpcError::Internal(format!("{action}: port allocation failed: {e}"))
        }
        StartError::Spawn(e) => IpcError::Internal(format!(
            "{action}: spawn failed (is the MLX runtime available?): {e}"
        )),
        StartError::HealthTimeout { stderr_tail } => IpcError::Internal(format!(
            "{action}: mlx-lm server did not become ready in time. Last output:\n{stderr_tail}"
        )),
        StartError::HealthBadStatus {
            status,
            stderr_tail,
        } => IpcError::Internal(format!(
            "{action}: /health returned status {status}. Last output:\n{stderr_tail}"
        )),
        StartError::RegistryFull => IpcError::BadArgument(format!(
            "{action}: already managing {} servers; stop one before starting another",
            mlx_lm::process::MAX_MANAGED_SERVERS
        )),
        StartError::ShuttingDown => IpcError::Internal(format!(
            "{action}: Plume is shutting down; not starting new servers"
        )),
    }
}

#[cfg(test)]
#[path = "providers_tests.rs"]
mod tests;
