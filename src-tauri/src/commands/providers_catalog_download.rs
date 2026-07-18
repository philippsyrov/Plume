//! Explicit fixed-catalog download IPC, deliberately separate from runtime start.

use serde::Deserialize;
use tauri::{Emitter, State};

use crate::commands::project::AppState;
use crate::error::{IpcError, IpcRequest};
use crate::providers::catalog::QWEN_CATALOG_ID;
use crate::providers::catalog_download::{
    remove_catalog_model, CatalogDownloadEvent, CatalogDownloadManager, DownloadError,
    DownloadEventSink, DownloadManifest, RemoveCatalogResult, ReqwestCatalogFetcher,
    CATALOG_DOWNLOAD_EVENT,
};
use crate::providers::mlx_lm;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CatalogDownloadPayload {
    /// Fixed catalog id from the row the user explicitly chose. The backend
    /// still accepts only Qwen's compiled-in id; no URL/path/revision arrives.
    pub catalog_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CatalogDownloadCancelPayload {
    /// Opaque id from a prior `providers.catalogDownload` response.
    pub operation_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CatalogRemovePayload {
    /// Fixed catalog id. This never accepts an arbitrary on-disk path.
    pub catalog_id: String,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogDownloadStartResponse {
    pub operation_id: String,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogDownloadCancelResponse {
    pub ok: bool,
}

/// Tauri event bridge for a bounded worker. Failed event delivery is not a
/// download failure: listeners are advisory and can disappear on UI reload.
#[derive(Clone)]
struct TauriCatalogDownloadEvents {
    app: tauri::AppHandle,
}

impl DownloadEventSink for TauriCatalogDownloadEvents {
    fn emit(&self, event: CatalogDownloadEvent) {
        if let Err(error) = self.app.emit(CATALOG_DOWNLOAD_EVENT, event) {
            tracing::warn!(%error, "catalog download event delivery failed");
        }
    }
}

/// Start an explicit fixed-manifest Qwen download. This has no project-trust
/// dependency because it only writes beneath the app-owned catalog root, does
/// not start a runtime, and cannot receive a caller-selected network location.
#[tauri::command]
pub async fn providers_catalog_download(
    req: IpcRequest<CatalogDownloadPayload>,
    state: State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<CatalogDownloadStartResponse, IpcError> {
    req.check_version()?;
    let catalog_id = req.payload.catalog_id;
    if catalog_id != QWEN_CATALOG_ID {
        return Err(IpcError::BadArgument(format!(
            "providers.catalogDownload: unsupported catalog '{catalog_id}'"
        )));
    }
    let manifest = DownloadManifest::fixed().map_err(download_error_to_ipc)?;
    let fetcher = ReqwestCatalogFetcher::new().map_err(download_error_to_ipc)?;
    let registry = state.catalog_downloads.clone();
    let operation = registry
        .begin_download_for_store(&state.catalog_store, &catalog_id)
        .map_err(download_error_to_ipc)?;
    let operation_id = operation.operation_id.clone();
    let manager = CatalogDownloadManager::new(
        state.catalog_store.clone(),
        manifest,
        fetcher,
        TauriCatalogDownloadEvents { app },
    );
    let worker_registry = registry.clone();
    let worker_operation = operation.clone();
    if let Err(error) = std::thread::Builder::new()
        .name("plume-catalog-download".into())
        .spawn(move || {
            let result = manager.run(QWEN_CATALOG_ID, &worker_operation);
            if let Err(error) = &result {
                tracing::warn!(%error, "catalog download worker stopped");
            }
            worker_registry.finish(&worker_operation);
            // The registry entry and its cross-process lock are gone before
            // the terminal event, so an immediate retry or remove cannot see
            // a completed operation as still active.
            manager.emit_terminal(&worker_operation, &result);
        })
    {
        registry.finish(&operation);
        return Err(IpcError::Internal(format!(
            "providers.catalogDownload: failed to start bounded worker: {error}"
        )));
    }
    Ok(CatalogDownloadStartResponse { operation_id })
}

/// Cancellation remains available without a trusted project, so a project
/// close/revocation cannot strand a user-requested app-private transfer.
#[tauri::command]
pub async fn providers_catalog_download_cancel(
    req: IpcRequest<CatalogDownloadCancelPayload>,
    state: State<'_, AppState>,
) -> Result<CatalogDownloadCancelResponse, IpcError> {
    req.check_version()?;
    state
        .catalog_downloads
        .cancel_download(&req.payload.operation_id)
        .map_err(download_error_to_ipc)?;
    Ok(CatalogDownloadCancelResponse { ok: true })
}

/// Remove the fixed receipt-backed install, but never while the current Plume
/// supervisor reports the catalog model as live. This command does not stop a
/// runtime and never receives a filesystem path from the frontend.
#[tauri::command]
pub async fn providers_catalog_remove(
    req: IpcRequest<CatalogRemovePayload>,
    state: State<'_, AppState>,
) -> Result<RemoveCatalogResult, IpcError> {
    req.check_version()?;
    let catalog_id = req.payload.catalog_id;
    let store = state.catalog_store.clone();
    let registry = state.catalog_downloads.clone();
    tauri::async_runtime::spawn_blocking(move || {
        remove_catalog_model(&registry, &store, &catalog_id, || {
            mlx_lm::catalog_model_is_reserved(&catalog_id)
        })
    })
    .await
    .map_err(|error| IpcError::Internal(format!("providers.catalogRemove task join: {error}")))?
    .map_err(download_error_to_ipc)
}

fn download_error_to_ipc(error: DownloadError) -> IpcError {
    let message = error.to_string();
    match error {
        DownloadError::UnsupportedCatalog(_) | DownloadError::UnknownOperation(_) => {
            IpcError::NotFound(message)
        }
        DownloadError::OperationActive { .. }
        | DownloadError::ModelRunning { .. }
        | DownloadError::InstallExists
        | DownloadError::InstallNotVerified => IpcError::BadArgument(message),
        DownloadError::Cancelled => IpcError::Cancelled,
        _ => IpcError::Internal(message),
    }
}
