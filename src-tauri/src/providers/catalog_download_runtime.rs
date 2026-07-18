//! Network adapter and lifecycle ownership for fixed catalog downloads.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use reqwest::blocking::{Client, Response};
use reqwest::header::{CONTENT_RANGE, RANGE};
use reqwest::redirect::Policy;
use serde::Serialize;

use super::filesystem::{self, CatalogFilesystemLock, CatalogRoot};
use super::*;

/// A stalled peer cannot hold the downloader forever. Requests are capped at
/// 4 MiB by the transfer layer, so this response deadline bounds one connect
/// and body read without putting a whole-model deadline on a healthy transfer.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(2);
const RESPONSE_TIMEOUT: Duration = Duration::from_secs(20);

/// Blocking HTTP implementation. Tests use a fake fetcher, so test runs never
/// request the 880 MB model or any other network resource.
#[derive(Clone)]
pub(crate) struct ReqwestCatalogFetcher {
    client: Client,
}

impl ReqwestCatalogFetcher {
    pub(crate) fn new() -> Result<Self, DownloadError> {
        let client = Client::builder()
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(RESPONSE_TIMEOUT)
            .redirect(Policy::custom(|attempt| {
                if redirect_is_allowed(attempt.previous().len(), attempt.url()) {
                    attempt.follow()
                } else {
                    attempt.stop()
                }
            }))
            .build()
            .map_err(|error| DownloadError::Transport(error.to_string()))?;
        Ok(Self { client })
    }
}

/// `reqwest` counts the initial URL in `previous()`. Therefore a fifth real
/// redirect arrives with five prior URLs; the sixth is denied. Keeping this
/// predicate separate makes the production closure's hop semantics testable.
pub(crate) fn redirect_is_allowed(previous_urls: usize, next: &reqwest::Url) -> bool {
    previous_urls <= MAX_REDIRECTS
        && next.scheme() == "https"
        && next.host_str().is_some_and(allowed_download_host)
}

impl DownloadFetcher for ReqwestCatalogFetcher {
    fn fetch(&self, request: &DownloadRequest) -> Result<DownloadResponse, DownloadError> {
        let mut builder = self.client.get(&request.url);
        if let (Some(start), Some(end)) = (request.range_start, request.range_end) {
            builder = builder.header(RANGE, format!("bytes={start}-{end}"));
        }
        let response = builder
            .send()
            .map_err(|error| DownloadError::Transport(error.to_string()))?;
        response_to_download_response(response)
    }
}

fn response_to_download_response(response: Response) -> Result<DownloadResponse, DownloadError> {
    let final_url = response.url().to_string();
    validate_https_url(&final_url)?;
    let content_range = response
        .headers()
        .get(CONTENT_RANGE)
        .and_then(|value| value.to_str().ok())
        .map(ToOwned::to_owned);
    Ok(DownloadResponse {
        status: response.status().as_u16(),
        content_range,
        // The production redirect policy validates every hop. Re-checking the
        // final URL here makes a policy regression fail closed even if reqwest
        // changes when it invokes the callback.
        redirect_urls: vec![final_url],
        body: Box::new(response),
    })
}

#[derive(Clone)]
pub(crate) struct DownloadOperation {
    pub(crate) operation_id: String,
    pub(crate) catalog_id: String,
    pub(crate) cancel: Arc<AtomicBool>,
    pub(crate) progress: Arc<Mutex<OperationProgress>>,
}

struct ActiveDownload {
    operation: DownloadOperation,
    /// This descriptor is the cross-process half of the lifecycle gate. It is
    /// owned by the registry entry rather than the worker clone so `finish`
    /// releases it before the terminal event is emitted.
    _filesystem_lock: CatalogFilesystemLock,
}

#[derive(Default, Clone)]
pub(crate) struct CatalogDownloadRegistry {
    active: Arc<Mutex<BTreeMap<String, ActiveDownload>>>,
    next_id: Arc<AtomicU64>,
}

/// Short-lived catalog-start ownership. It keeps the same in-process and
/// cross-process gates as a download until the MLX supervisor has recorded a
/// `Starting` slot. Dropping it before that point releases the gate on every
/// validation or spawn failure; releasing it after the slot lands avoids
/// blocking removal for the slower health poll.
pub(crate) struct CatalogStartReservation {
    registry: CatalogDownloadRegistry,
    operation: Option<DownloadOperation>,
}

impl CatalogStartReservation {
    pub(crate) fn release_after_starting_reservation(mut self) {
        if let Some(operation) = self.operation.take() {
            self.registry.finish(&operation);
        }
    }
}

impl Drop for CatalogStartReservation {
    fn drop(&mut self) {
        if let Some(operation) = self.operation.take() {
            self.registry.finish(&operation);
        }
    }
}

impl CatalogDownloadRegistry {
    pub(crate) fn begin_download_for_store(
        &self,
        store: &CatalogStore,
        catalog_id: &str,
    ) -> Result<DownloadOperation, DownloadError> {
        if catalog_id != QWEN_CATALOG_ID {
            return Err(DownloadError::UnsupportedCatalog(catalog_id.into()));
        }
        let mut active = self
            .active
            .lock()
            .expect("catalog download registry mutex poisoned");
        if active.contains_key(catalog_id) {
            return Err(DownloadError::OperationActive {
                catalog_id: catalog_id.into(),
            });
        }
        // The local mutex stays held while taking the advisory filesystem lock:
        // begin and remove therefore cannot pass each other between their local
        // active check and cross-process ownership acquisition.
        let filesystem_lock = filesystem::acquire_catalog_lock(store)?;
        let sequence = self.next_id.fetch_add(1, Ordering::Relaxed) + 1;
        let operation = DownloadOperation {
            operation_id: format!("catalog-download-{sequence:016x}"),
            catalog_id: catalog_id.into(),
            cancel: Arc::new(AtomicBool::new(false)),
            progress: Arc::new(Mutex::new(OperationProgress::default())),
        };
        active.insert(
            catalog_id.into(),
            ActiveDownload {
                operation: operation.clone(),
                _filesystem_lock: filesystem_lock,
            },
        );
        Ok(operation)
    }

    /// Reserve the catalog lifecycle gate for a start attempt. The caller must
    /// validate the receipt-backed path while this guard is alive, then hand
    /// the guard to the supervisor callback that runs after its `Starting`
    /// slot is installed.
    pub(crate) fn begin_catalog_start_for_store(
        &self,
        store: &CatalogStore,
        catalog_id: &str,
    ) -> Result<CatalogStartReservation, DownloadError> {
        if catalog_id != QWEN_CATALOG_ID {
            return Err(DownloadError::UnsupportedCatalog(catalog_id.into()));
        }
        let mut active = self
            .active
            .lock()
            .expect("catalog download registry mutex poisoned");
        if active.contains_key(catalog_id) {
            return Err(DownloadError::OperationActive {
                catalog_id: catalog_id.into(),
            });
        }
        let filesystem_lock = filesystem::acquire_catalog_lock(store)?;
        let sequence = self.next_id.fetch_add(1, Ordering::Relaxed) + 1;
        let operation = DownloadOperation {
            operation_id: format!("catalog-start-{sequence:016x}"),
            catalog_id: catalog_id.into(),
            cancel: Arc::new(AtomicBool::new(false)),
            progress: Arc::new(Mutex::new(OperationProgress::default())),
        };
        active.insert(
            catalog_id.into(),
            ActiveDownload {
                operation: operation.clone(),
                _filesystem_lock: filesystem_lock,
            },
        );
        Ok(CatalogStartReservation {
            registry: self.clone(),
            operation: Some(operation),
        })
    }

    pub(crate) fn cancel_download(&self, operation_id: &str) -> Result<(), DownloadError> {
        let active = self
            .active
            .lock()
            .expect("catalog download registry mutex poisoned");
        let operation = active
            .values()
            .find(|active| active.operation.operation_id == operation_id)
            .map(|active| &active.operation)
            .ok_or_else(|| DownloadError::UnknownOperation(operation_id.into()))?;
        operation.cancel.store(true, Ordering::Release);
        Ok(())
    }

    pub(crate) fn finish(&self, operation: &DownloadOperation) {
        let mut active = self
            .active
            .lock()
            .expect("catalog download registry mutex poisoned");
        if active
            .get(&operation.catalog_id)
            .is_some_and(|current| current.operation.operation_id == operation.operation_id)
        {
            active.remove(&operation.catalog_id);
        }
    }

    /// Hold the same local and cross-process ownership gates used by begin
    /// while removal validates and unlinks the receipt-backed install.
    pub(crate) fn with_removal_lock<T>(
        &self,
        store: &CatalogStore,
        catalog_id: &str,
        remove: impl FnOnce() -> Result<T, DownloadError>,
    ) -> Result<T, DownloadError> {
        if catalog_id != QWEN_CATALOG_ID {
            return Err(DownloadError::UnsupportedCatalog(catalog_id.into()));
        }
        let active = self
            .active
            .lock()
            .expect("catalog download registry mutex poisoned");
        if active.contains_key(catalog_id) {
            return Err(DownloadError::OperationActive {
                catalog_id: catalog_id.into(),
            });
        }
        let _filesystem_lock = filesystem::acquire_catalog_lock(store)?;
        // Keep both guards in scope through the full descriptor-relative remove.
        let result = remove();
        drop(_filesystem_lock);
        drop(active);
        result
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RemoveCatalogResult {
    pub removed: bool,
}

/// Remove only the fixed receipt-backed Qwen directory. The supervisor query
/// runs inside the download/remove gate so a local transfer cannot slip in
/// between the liveness check and descriptor-relative deletion.
pub(crate) fn remove_catalog_model(
    registry: &CatalogDownloadRegistry,
    store: &CatalogStore,
    catalog_id: &str,
    model_reserved: impl FnOnce() -> bool,
) -> Result<RemoveCatalogResult, DownloadError> {
    registry.with_removal_lock(store, catalog_id, || {
        if model_reserved() {
            return Err(DownloadError::ModelRunning {
                catalog_id: catalog_id.into(),
            });
        }
        let root = CatalogRoot::open(store)?;
        let removed = root.remove_verified_install(store)?;
        Ok(RemoveCatalogResult { removed })
    })
}
