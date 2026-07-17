//! Network adapter, operation registry, and fixed install removal.

use std::collections::BTreeMap;
use std::fs;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use reqwest::blocking::{Client, Response};
use reqwest::header::{CONTENT_RANGE, RANGE};
use reqwest::redirect::Policy;
use serde::Serialize;

use super::*;

/// Blocking HTTP implementation. Tests use a fake fetcher, so test runs never
/// request the 880 MB model or any other network resource.
#[derive(Clone)]
pub(crate) struct ReqwestCatalogFetcher {
    client: Client,
}

impl ReqwestCatalogFetcher {
    pub(crate) fn new() -> Result<Self, DownloadError> {
        let client = Client::builder()
            .redirect(Policy::custom(|attempt| {
                let host = attempt.url().host_str().unwrap_or_default();
                if attempt.previous().len() >= MAX_REDIRECTS || !allowed_download_host(host) {
                    attempt.stop()
                } else {
                    attempt.follow()
                }
            }))
            .build()
            .map_err(|error| DownloadError::Transport(error.to_string()))?;
        Ok(Self { client })
    }
}

impl DownloadFetcher for ReqwestCatalogFetcher {
    fn fetch(&self, request: &DownloadRequest) -> Result<DownloadResponse, DownloadError> {
        let mut builder = self.client.get(&request.url);
        if let Some(start) = request.range_start {
            builder = builder.header(RANGE, format!("bytes={start}-"));
        }
        let response = builder
            .send()
            .map_err(|error| DownloadError::Transport(error.to_string()))?;
        response_to_download_response(response)
    }
}

fn response_to_download_response(response: Response) -> Result<DownloadResponse, DownloadError> {
    let final_host = response
        .url()
        .host_str()
        .ok_or_else(|| DownloadError::RedirectPolicy {
            host: "missing host".into(),
        })?
        .to_string();
    if !allowed_download_host(&final_host) {
        return Err(DownloadError::RedirectPolicy { host: final_host });
    }
    let content_range = response
        .headers()
        .get(CONTENT_RANGE)
        .and_then(|value| value.to_str().ok())
        .map(ToOwned::to_owned);
    Ok(DownloadResponse {
        status: response.status().as_u16(),
        content_range,
        redirect_hosts: vec![final_host],
        body: Box::new(response),
    })
}

#[derive(Clone)]
pub(crate) struct DownloadOperation {
    pub(crate) operation_id: String,
    pub(crate) catalog_id: String,
    pub(crate) cancel: Arc<AtomicBool>,
}

#[derive(Default, Clone)]
pub(crate) struct CatalogDownloadRegistry {
    active: Arc<Mutex<BTreeMap<String, DownloadOperation>>>,
    next_id: Arc<AtomicU64>,
}

impl CatalogDownloadRegistry {
    pub(crate) fn begin_download(
        &self,
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
        let sequence = self.next_id.fetch_add(1, Ordering::Relaxed) + 1;
        let operation = DownloadOperation {
            operation_id: format!("catalog-download-{sequence:016x}"),
            catalog_id: catalog_id.into(),
            cancel: Arc::new(AtomicBool::new(false)),
        };
        active.insert(catalog_id.into(), operation.clone());
        Ok(operation)
    }

    pub(crate) fn cancel_download(&self, operation_id: &str) -> Result<(), DownloadError> {
        let active = self
            .active
            .lock()
            .expect("catalog download registry mutex poisoned");
        let operation = active
            .values()
            .find(|operation| operation.operation_id == operation_id)
            .ok_or_else(|| DownloadError::UnknownOperation(operation_id.into()))?;
        operation.cancel.store(true, Ordering::Release);
        Ok(())
    }

    pub(crate) fn is_active(&self, catalog_id: &str) -> bool {
        self.active
            .lock()
            .expect("catalog download registry mutex poisoned")
            .contains_key(catalog_id)
    }

    pub(crate) fn finish(&self, operation: &DownloadOperation) {
        let mut active = self
            .active
            .lock()
            .expect("catalog download registry mutex poisoned");
        if active
            .get(&operation.catalog_id)
            .is_some_and(|current| current.operation_id == operation.operation_id)
        {
            active.remove(&operation.catalog_id);
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RemoveCatalogResult {
    pub removed: bool,
}

/// Remove only the fixed, receipt-backed Qwen directory. Callers supply the
/// supervisor verdict; this module never starts or stops a runtime to remove.
pub(crate) fn remove_catalog_model(
    store: &CatalogStore,
    catalog_id: &str,
    model_running: bool,
) -> Result<RemoveCatalogResult, DownloadError> {
    if catalog_id != QWEN_CATALOG_ID {
        return Err(DownloadError::UnsupportedCatalog(catalog_id.into()));
    }
    if model_running {
        return Err(DownloadError::ModelRunning {
            catalog_id: catalog_id.into(),
        });
    }
    let install = store.qwen_install_dir();
    match fs::symlink_metadata(&install) {
        Ok(metadata) => {
            validate_directory_metadata(&install, &metadata)?;
            if !store.qwen_install_is_valid() {
                return Err(DownloadError::InstallNotVerified);
            }
            fs::remove_dir_all(&install).map_err(|error| io_error(&install, error))?;
            Ok(RemoveCatalogResult { removed: true })
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Ok(RemoveCatalogResult { removed: false })
        }
        Err(error) => Err(io_error(&install, error)),
    }
}
