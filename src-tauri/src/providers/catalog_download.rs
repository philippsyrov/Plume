//! Fixed-manifest catalog download policy and transfer orchestration.
//!
//! Filesystem mutation is deliberately isolated in `catalog_download_fs`; this
//! module owns the immutable manifest, HTTPS/redirect policy, transfer checks,
//! and ordered events. It never accepts caller-controlled model locations.

use std::collections::BTreeSet;
#[cfg(test)]
use std::io::Cursor;
use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use super::catalog::{CatalogStore, InstallReceipt, QWEN_CATALOG_ID, QWEN_REVISION};

pub const CATALOG_DOWNLOAD_EVENT: &str = "providers/catalog-download";
pub(crate) const MAX_REDIRECTS: usize = 5;
pub(crate) const DOWNLOAD_SLACK_BYTES: u64 = 1024 * 1024;
pub(crate) const COPY_BUFFER_BYTES: usize = 64 * 1024;
/// Each HTTPS response is bounded so the client timeout applies to a finite
/// slice rather than the whole 880 MB artifact.
pub(crate) const MAX_RESPONSE_BYTES: u64 = 4 * 1024 * 1024;
const FIXED_MANIFEST_BYTES: &[u8] = include_bytes!("catalog_download_manifest.json");

#[path = "catalog_download_fs.rs"]
mod filesystem;
#[path = "catalog_download_runtime.rs"]
mod runtime;
#[cfg(test)]
pub(crate) use filesystem::with_publication_hook_for_test;
#[cfg(test)]
pub(crate) use runtime::redirect_is_allowed;
pub(crate) use runtime::{
    remove_catalog_model, CatalogDownloadRegistry, CatalogStartReservation, DownloadOperation,
    RemoveCatalogResult, ReqwestCatalogFetcher,
};

/// Every host that the reviewed immutable Hugging Face source may use.
pub(crate) fn allowed_download_host(host: &str) -> bool {
    matches!(
        host.to_ascii_lowercase().as_str(),
        "huggingface.co"
            | "cdn-lfs.huggingface.co"
            | "cdn-lfs-us-1.huggingface.co"
            | "cas-bridge.xethub.hf.co"
            | "transfer.xethub.hf.co"
    )
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct DownloadManifest {
    pub catalog_id: String,
    repo: String,
    pub revision: String,
    license: String,
    weight_bytes: u64,
    pub total_bytes: u64,
    pub files: Vec<ManifestFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ManifestFile {
    pub path: String,
    pub size: u64,
    pub(crate) sha256: String,
}

impl DownloadManifest {
    pub(crate) fn fixed() -> Result<Self, DownloadError> {
        Self::parse_bytes(FIXED_MANIFEST_BYTES)
    }

    #[cfg(test)]
    pub(crate) fn parse_json(json: &str) -> Result<Self, DownloadError> {
        Self::parse_bytes(json.as_bytes())
    }

    fn parse_bytes(bytes: &[u8]) -> Result<Self, DownloadError> {
        let manifest: Self = serde_json::from_slice(bytes)
            .map_err(|error| DownloadError::Manifest(error.to_string()))?;
        manifest.validate()?;
        Ok(manifest)
    }

    fn validate(&self) -> Result<(), DownloadError> {
        if self.catalog_id != QWEN_CATALOG_ID {
            return Err(DownloadError::Manifest(
                "catalog id is not the fixed Qwen id".into(),
            ));
        }
        if self.repo != "mlx-community/Qwen2.5-Coder-1.5B-Instruct-4bit" {
            return Err(DownloadError::Manifest(
                "repository is not the fixed Qwen repository".into(),
            ));
        }
        if self.revision != QWEN_REVISION {
            return Err(DownloadError::Manifest(
                "revision is not the pinned commit".into(),
            ));
        }
        if self.license != "Apache-2.0" {
            return Err(DownloadError::Manifest("license is not Apache-2.0".into()));
        }
        if self.files.is_empty() {
            return Err(DownloadError::Manifest("file list is empty".into()));
        }
        let mut paths = BTreeSet::new();
        let mut total = 0u64;
        let mut weight_size = None;
        for file in &self.files {
            if !is_safe_manifest_name(&file.path) || file.size == 0 {
                return Err(DownloadError::Manifest(format!(
                    "unsafe manifest file '{}'",
                    file.path
                )));
            }
            if !paths.insert(file.path.as_str()) {
                return Err(DownloadError::Manifest(format!(
                    "duplicate manifest file '{}'",
                    file.path
                )));
            }
            if file.sha256.len() != 64 || !file.sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
            {
                return Err(DownloadError::Manifest(format!(
                    "invalid SHA-256 for '{}'",
                    file.path
                )));
            }
            total = total
                .checked_add(file.size)
                .ok_or_else(|| DownloadError::Manifest("manifest total overflows u64".into()))?;
            if file.path == "model.safetensors" {
                weight_size = Some(file.size);
            }
        }
        if total != self.total_bytes || weight_size != Some(self.weight_bytes) {
            return Err(DownloadError::Manifest(
                "manifest byte totals do not match fixed weights".into(),
            ));
        }
        Ok(())
    }

    fn download_url(&self, file: &ManifestFile) -> String {
        format!(
            "https://huggingface.co/{}/resolve/{}/{}",
            self.repo, self.revision, file.path
        )
    }
}

fn is_safe_manifest_name(path: &str) -> bool {
    !path.is_empty() && path != "." && path != ".." && !path.contains(['/', '\\', '\0'])
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum DownloadError {
    #[error("catalog download manifest is invalid: {0}")]
    Manifest(String),
    #[error("catalog download only supports '{0}'")]
    UnsupportedCatalog(String),
    #[error("a catalog download is already active for '{catalog_id}'")]
    OperationActive { catalog_id: String },
    #[error("catalog download operation '{0}' is not active")]
    UnknownOperation(String),
    #[error("catalog download was cancelled")]
    Cancelled,
    #[error("download transport failed: {0}")]
    Transport(String),
    #[error("invalid Content-Range for '{path}': {actual:?}")]
    InvalidContentRange {
        path: String,
        actual: Option<String>,
    },
    #[error("download size mismatch for '{path}': expected {expected}, got {actual}")]
    SizeMismatch {
        path: String,
        expected: u64,
        actual: u64,
    },
    #[error("downloaded bytes exceed the bounded manifest ceiling")]
    ByteCeiling,
    #[error("download hash mismatch for '{path}'")]
    HashMismatch { path: String },
    #[error("download redirect policy rejected '{host}'")]
    RedirectPolicy { host: String },
    #[error("refusing symlinked catalog path '{path}'")]
    SymlinkPath { path: String },
    #[error("catalog path '{path}' is not a directory")]
    NotDirectory { path: String },
    #[error("catalog staging contains unexpected path '{path}'")]
    UnexpectedStagingPath { path: String },
    #[error("catalog path was swapped while its descriptor was open: '{path}'")]
    PathSwap { path: String },
    #[error("catalog path is absent")]
    MissingPath,
    #[error("catalog install directory already exists")]
    InstallExists,
    #[error("catalog path already exists: '{path}'")]
    AlreadyExists { path: String },
    #[error("catalog install directory is not backed by a valid receipt")]
    InstallNotVerified,
    #[error("cannot remove a catalog model while it is running")]
    ModelRunning { catalog_id: String },
    #[error("catalog I/O failed at '{path}': {reason}")]
    Io { path: String, reason: String },
}

#[derive(Debug, Clone)]
pub(crate) struct DownloadRequest {
    #[cfg(test)]
    pub path: String,
    pub range_start: Option<u64>,
    pub range_end: Option<u64>,
    pub(crate) url: String,
}

pub(crate) struct DownloadResponse {
    pub status: u16,
    pub content_range: Option<String>,
    pub redirect_urls: Vec<String>,
    pub(crate) body: Box<dyn Read + Send>,
}

impl DownloadResponse {
    #[cfg(test)]
    pub(crate) fn from_bytes(
        status: u16,
        content_range: Option<String>,
        redirect_urls: Vec<String>,
        bytes: Vec<u8>,
    ) -> Self {
        Self {
            status,
            content_range,
            redirect_urls,
            body: Box::new(Cursor::new(bytes)),
        }
    }
}

pub(crate) trait DownloadFetcher: Send + Sync {
    fn fetch(&self, request: &DownloadRequest) -> Result<DownloadResponse, DownloadError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum DownloadPhase {
    Started,
    Downloading,
    Verifying,
    Installed,
    Cancelled,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CatalogDownloadEvent {
    pub operation_id: String,
    pub seq: u64,
    pub catalog_id: String,
    pub phase: DownloadPhase,
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
    pub error: Option<String>,
}

pub(crate) trait DownloadEventSink: Send + Sync {
    fn emit(&self, event: CatalogDownloadEvent);
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DownloadResult {
    pub installed_bytes: u64,
}

#[derive(Default)]
pub(crate) struct OperationProgress {
    next_seq: u64,
    downloaded_bytes: u64,
}

#[derive(Clone)]
pub(crate) struct CatalogDownloadManager<F, E> {
    store: Arc<CatalogStore>,
    manifest: DownloadManifest,
    fetcher: F,
    events: E,
}

impl<F, E> CatalogDownloadManager<F, E>
where
    F: DownloadFetcher,
    E: DownloadEventSink,
{
    pub(crate) fn new(
        store: Arc<CatalogStore>,
        manifest: DownloadManifest,
        fetcher: F,
        events: E,
    ) -> Self {
        Self {
            store,
            manifest,
            fetcher,
            events,
        }
    }

    /// Runs transfer work but deliberately leaves terminal event emission to
    /// the caller, which must release registry + filesystem ownership first.
    pub(crate) fn run(
        &self,
        catalog_id: &str,
        operation: &DownloadOperation,
    ) -> Result<DownloadResult, DownloadError> {
        if catalog_id != self.manifest.catalog_id || operation.catalog_id != catalog_id {
            return Err(DownloadError::UnsupportedCatalog(catalog_id.into()));
        }
        let mut reporter = EventReporter::new(&self.events, operation, self.manifest.total_bytes);
        reporter.emit(DownloadPhase::Started, 0, None);
        self.run_inner(operation, &mut reporter)
    }

    pub(crate) fn emit_terminal(
        &self,
        operation: &DownloadOperation,
        result: &Result<DownloadResult, DownloadError>,
    ) {
        let mut reporter = EventReporter::new(&self.events, operation, self.manifest.total_bytes);
        match result {
            Ok(result) => reporter.emit(DownloadPhase::Installed, result.installed_bytes, None),
            Err(DownloadError::Cancelled) => {
                reporter.emit(DownloadPhase::Cancelled, reporter.downloaded(), None)
            }
            Err(error) => reporter.emit(
                DownloadPhase::Failed,
                reporter.downloaded(),
                Some(error.to_string()),
            ),
        }
    }

    fn run_inner(
        &self,
        operation: &DownloadOperation,
        reporter: &mut EventReporter<'_, E>,
    ) -> Result<DownloadResult, DownloadError> {
        check_cancelled(&operation.cancel)?;
        let root = filesystem::CatalogRoot::open(&self.store)?;
        let mut staging = root.open_staging()?;
        let initial = staging.preflight(&self.manifest)?;
        reporter.emit(DownloadPhase::Downloading, initial, None);

        for file in &self.manifest.files {
            download_file(
                &self.fetcher,
                &mut staging,
                file,
                &self.manifest,
                &operation.cancel,
                reporter,
            )?;
        }
        check_cancelled(&operation.cancel)?;
        reporter.emit(DownloadPhase::Verifying, reporter.downloaded(), None);
        let receipt = InstallReceipt {
            catalog_id: QWEN_CATALOG_ID.into(),
            revision: QWEN_REVISION.into(),
            manifest_sha256: self.store.expected_manifest_sha256(),
            installed_bytes: self.manifest.total_bytes,
            completed_at_ms: now_unix_ms(),
        };
        root.finalize(&mut staging, &self.manifest, &receipt, &operation.cancel)?;
        Ok(DownloadResult {
            installed_bytes: self.manifest.total_bytes,
        })
    }
}

struct EventReporter<'a, E> {
    sink: &'a E,
    operation: &'a DownloadOperation,
    total_bytes: u64,
}

impl<'a, E: DownloadEventSink> EventReporter<'a, E> {
    fn new(sink: &'a E, operation: &'a DownloadOperation, total_bytes: u64) -> Self {
        Self {
            sink,
            operation,
            total_bytes,
        }
    }

    fn downloaded(&self) -> u64 {
        self.operation
            .progress
            .lock()
            .expect("catalog operation progress mutex poisoned")
            .downloaded_bytes
    }

    fn emit(&mut self, phase: DownloadPhase, downloaded_bytes: u64, error: Option<String>) {
        let (seq, downloaded_bytes) = {
            let mut progress = self
                .operation
                .progress
                .lock()
                .expect("catalog operation progress mutex poisoned");
            progress.downloaded_bytes = downloaded_bytes;
            progress.next_seq = progress.next_seq.saturating_add(1);
            (progress.next_seq, progress.downloaded_bytes)
        };
        self.sink.emit(CatalogDownloadEvent {
            operation_id: self.operation.operation_id.clone(),
            seq,
            catalog_id: self.operation.catalog_id.clone(),
            phase,
            downloaded_bytes,
            total_bytes: self.total_bytes,
            error,
        });
    }

    fn advance(&mut self, amount: u64) -> Result<(), DownloadError> {
        let next = self
            .downloaded()
            .checked_add(amount)
            .ok_or(DownloadError::ByteCeiling)?;
        if next > self.total_bytes.saturating_add(DOWNLOAD_SLACK_BYTES) {
            return Err(DownloadError::ByteCeiling);
        }
        self.emit(DownloadPhase::Downloading, next, None);
        Ok(())
    }

    fn rewind(&mut self, amount: u64) -> Result<(), DownloadError> {
        let next = self
            .downloaded()
            .checked_sub(amount)
            .ok_or(DownloadError::ByteCeiling)?;
        self.emit(DownloadPhase::Downloading, next, None);
        Ok(())
    }
}

fn download_file<F, E>(
    fetcher: &F,
    staging: &mut filesystem::StagingDir,
    file: &ManifestFile,
    manifest: &DownloadManifest,
    cancel: &AtomicBool,
    reporter: &mut EventReporter<'_, E>,
) -> Result<(), DownloadError>
where
    F: DownloadFetcher,
    E: DownloadEventSink,
{
    if staging.verified(&file.path) {
        return Ok(());
    }
    let (mut part, current) = staging.open_part_for_resume(file)?;
    if current == file.size {
        // Preflight never counts a corrupt complete part. Seeing one here means
        // a concurrent replacement occurred after preflight; fail safely and
        // leave the next run to discard it before publishing progress.
        return Err(DownloadError::HashMismatch {
            path: file.path.clone(),
        });
    }
    check_cancelled(cancel)?;
    let mut buffer = [0u8; COPY_BUFFER_BYTES];
    let mut written = current;
    let mut restarted_after_ignored_range = false;
    while written < file.size {
        check_cancelled(cancel)?;
        let range_end = written
            .saturating_add(MAX_RESPONSE_BYTES.saturating_sub(1))
            .min(file.size.saturating_sub(1));
        let request = DownloadRequest {
            #[cfg(test)]
            path: file.path.clone(),
            range_start: Some(written),
            range_end: Some(range_end),
            url: manifest.download_url(file),
        };
        validate_https_url(&request.url)?;
        let mut response = match fetcher.fetch(&request) {
            Ok(response) => response,
            Err(_error) if cancel.load(Ordering::Acquire) => return Err(DownloadError::Cancelled),
            Err(error) => return Err(error),
        };
        validate_redirects(&response.redirect_urls)?;
        if response.status == 200 && response.content_range.is_none() && written > 0 {
            if restarted_after_ignored_range {
                return Err(DownloadError::InvalidContentRange {
                    path: file.path.clone(),
                    actual: response.content_range,
                });
            }
            restarted_after_ignored_range = true;
            drop(response);
            staging.truncate_part_for_restart(&mut part, file)?;
            reporter.rewind(written)?;
            written = 0;
            continue;
        }
        if response.status != 206
            || !matches_content_range(
                response.content_range.as_deref(),
                written,
                range_end,
                file.size,
            )
        {
            return Err(DownloadError::InvalidContentRange {
                path: file.path.clone(),
                actual: response.content_range,
            });
        }
        loop {
            check_cancelled(cancel)?;
            let count = match response.body.read(&mut buffer) {
                Ok(count) => count,
                Err(_error) if cancel.load(Ordering::Acquire) => {
                    return Err(DownloadError::Cancelled)
                }
                Err(error) => {
                    return Err(DownloadError::Io {
                        path: file.path.clone(),
                        reason: error.to_string(),
                    })
                }
            };
            if count == 0 {
                break;
            }
            written = written
                .checked_add(count as u64)
                .ok_or(DownloadError::ByteCeiling)?;
            if written > file.size || written > range_end.saturating_add(1) {
                return Err(DownloadError::SizeMismatch {
                    path: file.path.clone(),
                    expected: file.size,
                    actual: written,
                });
            }
            part.write_all(&buffer[..count])
                .map_err(|error| DownloadError::Io {
                    path: file.path.clone(),
                    reason: error.to_string(),
                })?;
            reporter.advance(count as u64)?;
        }
        if written != range_end.saturating_add(1) {
            return Err(DownloadError::SizeMismatch {
                path: file.path.clone(),
                expected: range_end.saturating_add(1),
                actual: written,
            });
        }
    }
    part.sync_all().map_err(|error| DownloadError::Io {
        path: file.path.clone(),
        reason: error.to_string(),
    })?;
    if written != file.size {
        return Err(DownloadError::SizeMismatch {
            path: file.path.clone(),
            expected: file.size,
            actual: written,
        });
    }
    let digest = filesystem::hash_verified_part(&mut part, &file.path)?;
    if digest != file.sha256 {
        return Err(DownloadError::HashMismatch {
            path: file.path.clone(),
        });
    }
    staging.remember_verified(file, part)
}

fn check_cancelled(cancel: &AtomicBool) -> Result<(), DownloadError> {
    if cancel.load(Ordering::Acquire) {
        Err(DownloadError::Cancelled)
    } else {
        Ok(())
    }
}

pub(crate) fn validate_https_url(value: &str) -> Result<(), DownloadError> {
    let url = reqwest::Url::parse(value)
        .map_err(|_| DownloadError::RedirectPolicy { host: value.into() })?;
    let host = url
        .host_str()
        .ok_or_else(|| DownloadError::RedirectPolicy { host: value.into() })?;
    if url.scheme() != "https" || !allowed_download_host(host) {
        return Err(DownloadError::RedirectPolicy { host: value.into() });
    }
    Ok(())
}

fn validate_redirects(urls: &[String]) -> Result<(), DownloadError> {
    if urls.len() > MAX_REDIRECTS {
        return Err(DownloadError::RedirectPolicy {
            host: "redirect limit exceeded".into(),
        });
    }
    for url in urls {
        validate_https_url(url)?;
    }
    Ok(())
}

fn matches_content_range(value: Option<&str>, start: u64, end: u64, total: u64) -> bool {
    start <= end && end < total && value == Some(format!("bytes {start}-{end}/{total}").as_str())
}

fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().try_into().unwrap_or(u64::MAX))
        .unwrap_or(0)
}

#[cfg(test)]
pub(crate) fn remove_verified_install_with_parent_swap_for_test<F>(
    store: &CatalogStore,
    hook: F,
) -> Result<bool, DownloadError>
where
    F: FnOnce(),
{
    filesystem::CatalogRoot::open(store)?.remove_verified_install_with_hook(store, hook)
}
