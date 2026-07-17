//! Fixed-manifest, resumable catalog downloads.
//!
//! This module owns only the explicit Qwen catalog download path. It never
//! selects, starts, or launches a model, and it never accepts a URL, revision,
//! install path, or project path from IPC.

use std::collections::BTreeSet;
use std::fs::{self, File, OpenOptions};
#[cfg(test)]
use std::io::Cursor;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::catalog::{CatalogStore, InstallReceipt, QWEN_CATALOG_ID, QWEN_REVISION};

pub const CATALOG_DOWNLOAD_EVENT: &str = "providers/catalog-download";
const MAX_REDIRECTS: usize = 5;
const DOWNLOAD_SLACK_BYTES: u64 = 1024 * 1024;
const COPY_BUFFER_BYTES: usize = 64 * 1024;
const FIXED_MANIFEST_BYTES: &[u8] = include_bytes!("catalog_download_manifest.json");

#[path = "catalog_download_runtime.rs"]
mod runtime;
pub(crate) use runtime::{
    remove_catalog_model, CatalogDownloadRegistry, DownloadOperation, RemoveCatalogResult,
    ReqwestCatalogFetcher,
};

/// Every host that the fixed Hugging Face source is allowed to redirect to.
/// Adding a delivery host requires a source and policy review.
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
    sha256: String,
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
            if !is_safe_manifest_name(&file.path) {
                return Err(DownloadError::Manifest(format!(
                    "file path '{}' is not a safe single filename",
                    file.path
                )));
            }
            if file.size == 0 {
                return Err(DownloadError::Manifest(format!(
                    "file '{}' has zero size",
                    file.path
                )));
            }
            if !paths.insert(file.path.as_str()) {
                return Err(DownloadError::Manifest(format!(
                    "file path '{}' appears more than once",
                    file.path
                )));
            }
            if file.sha256.len() != 64 || !file.sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
            {
                return Err(DownloadError::Manifest(format!(
                    "file '{}' does not have a SHA-256 digest",
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
        if total != self.total_bytes {
            return Err(DownloadError::Manifest(format!(
                "totalBytes {} does not equal file total {total}",
                self.total_bytes
            )));
        }
        if weight_size != Some(self.weight_bytes) {
            return Err(DownloadError::Manifest(
                "weightBytes does not match model.safetensors".into(),
            ));
        }
        Ok(())
    }

    fn download_url(&self, file: &ManifestFile) -> String {
        // `validate` limits file names to plain single components, and both the
        // repository and revision are hard-coded above. Nothing here is IPC input.
        format!(
            "https://huggingface.co/{}/resolve/{}/{}",
            self.repo, self.revision, file.path
        )
    }
}

fn is_safe_manifest_name(path: &str) -> bool {
    !path.is_empty()
        && path != "."
        && path != ".."
        && !path.contains('/')
        && !path.contains('\\')
        && !path.contains('\0')
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
    #[error("download returned HTTP status {status} for '{path}'")]
    HttpStatus { path: String, status: u16 },
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
    #[error("catalog install directory already exists")]
    InstallExists,
    #[error("catalog install directory is not backed by a valid receipt")]
    InstallNotVerified,
    #[error("cannot remove a catalog model while it is running")]
    ModelRunning { catalog_id: String },
    #[error("catalog I/O failed at '{path}': {reason}")]
    Io { path: String, reason: String },
}

#[derive(Debug, Clone)]
pub(crate) struct DownloadRequest {
    #[allow(dead_code)] // The fake fetcher records the fixed filename in tests.
    pub path: String,
    pub range_start: Option<u64>,
    url: String,
}

pub(crate) struct DownloadResponse {
    pub status: u16,
    pub content_range: Option<String>,
    pub redirect_hosts: Vec<String>,
    body: Box<dyn Read + Send>,
}

impl DownloadResponse {
    #[cfg(test)]
    pub(crate) fn from_bytes(
        status: u16,
        content_range: Option<String>,
        redirect_hosts: Vec<String>,
        bytes: Vec<u8>,
    ) -> Self {
        Self {
            status,
            content_range,
            redirect_hosts,
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
        let result = self.run_inner(operation, &mut reporter);
        match &result {
            Ok(result) => reporter.emit(DownloadPhase::Installed, result.installed_bytes, None),
            Err(DownloadError::Cancelled) => {
                reporter.emit(DownloadPhase::Cancelled, reporter.downloaded_bytes, None)
            }
            Err(error) => reporter.emit(
                DownloadPhase::Failed,
                reporter.downloaded_bytes,
                Some(error.to_string()),
            ),
        }
        result
    }

    fn run_inner(
        &self,
        operation: &DownloadOperation,
        reporter: &mut EventReporter<'_, E>,
    ) -> Result<DownloadResult, DownloadError> {
        check_cancelled(&operation.cancel)?;
        let catalog_dir = ensure_catalog_dir(&self.store)?;
        let staging = self.store.staging_dir();
        ensure_directory(&staging)?;
        validate_staging_contents(&staging, &self.manifest)?;

        let mut initial = 0u64;
        for file in &self.manifest.files {
            initial = initial
                .checked_add(existing_part_len(&staging, file)?)
                .ok_or(DownloadError::ByteCeiling)?;
        }
        if initial > self.manifest.total_bytes {
            return Err(DownloadError::ByteCeiling);
        }
        reporter.downloaded_bytes = initial;
        reporter.emit(DownloadPhase::Downloading, initial, None);

        for file in &self.manifest.files {
            download_file(
                &self.fetcher,
                &staging,
                file,
                self.manifest.total_bytes,
                &operation.cancel,
                reporter,
                &self.manifest,
            )?;
        }

        check_cancelled(&operation.cancel)?;
        reporter.emit(DownloadPhase::Verifying, reporter.downloaded_bytes, None);
        finalize_install(&self.store, &catalog_dir, &staging, &self.manifest)?;
        Ok(DownloadResult {
            installed_bytes: self.manifest.total_bytes,
        })
    }
}

struct EventReporter<'a, E> {
    sink: &'a E,
    operation: &'a DownloadOperation,
    total_bytes: u64,
    next_seq: u64,
    downloaded_bytes: u64,
}

impl<'a, E: DownloadEventSink> EventReporter<'a, E> {
    fn new(sink: &'a E, operation: &'a DownloadOperation, total_bytes: u64) -> Self {
        Self {
            sink,
            operation,
            total_bytes,
            next_seq: 1,
            downloaded_bytes: 0,
        }
    }

    fn emit(&mut self, phase: DownloadPhase, downloaded_bytes: u64, error: Option<String>) {
        self.downloaded_bytes = downloaded_bytes;
        self.sink.emit(CatalogDownloadEvent {
            operation_id: self.operation.operation_id.clone(),
            seq: self.next_seq,
            catalog_id: self.operation.catalog_id.clone(),
            phase,
            downloaded_bytes,
            total_bytes: self.total_bytes,
            error,
        });
        self.next_seq = self.next_seq.saturating_add(1);
    }
}

fn check_cancelled(cancel: &AtomicBool) -> Result<(), DownloadError> {
    if cancel.load(Ordering::Acquire) {
        Err(DownloadError::Cancelled)
    } else {
        Ok(())
    }
}

fn ensure_catalog_dir(store: &CatalogStore) -> Result<PathBuf, DownloadError> {
    let app_data = store.app_data_dir();
    ensure_directory(app_data)?;
    let models = app_data.join("models");
    ensure_directory(&models)?;
    let catalog = models.join("catalog");
    ensure_directory(&catalog)?;
    let catalog_id = catalog.join(QWEN_CATALOG_ID);
    ensure_directory(&catalog_id)?;
    Ok(catalog_id)
}

fn ensure_directory(path: &Path) -> Result<(), DownloadError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => validate_directory_metadata(path, &metadata),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir(path).map_err(|error| io_error(path, error))?;
            let metadata = fs::symlink_metadata(path).map_err(|error| io_error(path, error))?;
            validate_directory_metadata(path, &metadata)
        }
        Err(error) => Err(io_error(path, error)),
    }
}

fn validate_directory_metadata(path: &Path, metadata: &fs::Metadata) -> Result<(), DownloadError> {
    if metadata.file_type().is_symlink() {
        return Err(DownloadError::SymlinkPath {
            path: path.display().to_string(),
        });
    }
    if !metadata.is_dir() {
        return Err(DownloadError::NotDirectory {
            path: path.display().to_string(),
        });
    }
    Ok(())
}

fn validate_staging_contents(
    staging: &Path,
    manifest: &DownloadManifest,
) -> Result<(), DownloadError> {
    let allowed = manifest
        .files
        .iter()
        .map(|file| format!("{}.part", file.path))
        .collect::<BTreeSet<_>>();
    for entry in fs::read_dir(staging).map_err(|error| io_error(staging, error))? {
        let entry = entry.map_err(|error| io_error(staging, error))?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path).map_err(|error| io_error(&path, error))?;
        if !allowed.contains(name.as_ref())
            || metadata.file_type().is_symlink()
            || !metadata.is_file()
        {
            return Err(DownloadError::UnexpectedStagingPath {
                path: path.display().to_string(),
            });
        }
    }
    Ok(())
}

fn part_path(staging: &Path, file: &ManifestFile) -> PathBuf {
    staging.join(format!("{}.part", file.path))
}

fn existing_part_len(staging: &Path, file: &ManifestFile) -> Result<u64, DownloadError> {
    let path = part_path(staging, file);
    match fs::symlink_metadata(&path) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() {
                return Err(DownloadError::SymlinkPath {
                    path: path.display().to_string(),
                });
            }
            if !metadata.is_file() {
                return Err(DownloadError::UnexpectedStagingPath {
                    path: path.display().to_string(),
                });
            }
            if metadata.len() > file.size {
                return Err(DownloadError::SizeMismatch {
                    path: file.path.clone(),
                    expected: file.size,
                    actual: metadata.len(),
                });
            }
            Ok(metadata.len())
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(0),
        Err(error) => Err(io_error(&path, error)),
    }
}

fn download_file<F, E>(
    fetcher: &F,
    staging: &Path,
    file: &ManifestFile,
    manifest_total: u64,
    cancel: &AtomicBool,
    reporter: &mut EventReporter<'_, E>,
    manifest: &DownloadManifest,
) -> Result<(), DownloadError>
where
    F: DownloadFetcher,
    E: DownloadEventSink,
{
    let path = part_path(staging, file);
    let mut hasher = Sha256::new();
    let mut current = hash_existing_part(&path, file, &mut hasher)?;
    if current == file.size {
        if hex_digest(&hasher) == file.sha256 {
            return Ok(());
        }
        fs::remove_file(&path).map_err(|error| io_error(&path, error))?;
        current = 0;
        hasher = Sha256::new();
    }

    check_cancelled(cancel)?;
    let request = DownloadRequest {
        path: file.path.clone(),
        range_start: (current > 0).then_some(current),
        url: manifest.download_url(file),
    };
    let mut response = fetcher.fetch(&request)?;
    validate_redirects(&response.redirect_hosts)?;
    if current == 0 {
        if response.status != 200 {
            return Err(DownloadError::HttpStatus {
                path: file.path.clone(),
                status: response.status,
            });
        }
    } else if response.status != 206
        || !matches_content_range(response.content_range.as_deref(), current, file.size)
    {
        return Err(DownloadError::InvalidContentRange {
            path: file.path.clone(),
            actual: response.content_range,
        });
    }

    let mut output = open_part_for_append(&path)?;
    let mut buffer = [0u8; COPY_BUFFER_BYTES];
    loop {
        check_cancelled(cancel)?;
        let count = response
            .body
            .read(&mut buffer)
            .map_err(|error| io_error(&path, error))?;
        if count == 0 {
            break;
        }
        let next = current
            .checked_add(count as u64)
            .ok_or(DownloadError::ByteCeiling)?;
        if next > file.size {
            return Err(DownloadError::SizeMismatch {
                path: file.path.clone(),
                expected: file.size,
                actual: next,
            });
        }
        let total = reporter
            .downloaded_bytes
            .checked_add(count as u64)
            .ok_or(DownloadError::ByteCeiling)?;
        if total > manifest_total.saturating_add(DOWNLOAD_SLACK_BYTES) {
            return Err(DownloadError::ByteCeiling);
        }
        output
            .write_all(&buffer[..count])
            .map_err(|error| io_error(&path, error))?;
        hasher.update(&buffer[..count]);
        current = next;
        reporter.emit(DownloadPhase::Downloading, total, None);
    }
    output.sync_all().map_err(|error| io_error(&path, error))?;
    if current != file.size {
        return Err(DownloadError::SizeMismatch {
            path: file.path.clone(),
            expected: file.size,
            actual: current,
        });
    }
    if hex_digest(&hasher) != file.sha256 {
        return Err(DownloadError::HashMismatch {
            path: file.path.clone(),
        });
    }
    Ok(())
}

fn hash_existing_part(
    path: &Path,
    file: &ManifestFile,
    hasher: &mut Sha256,
) -> Result<u64, DownloadError> {
    let length = existing_part_len(
        path.parent()
            .ok_or_else(|| DownloadError::UnexpectedStagingPath {
                path: path.display().to_string(),
            })?,
        &ManifestFile {
            path: file.path.clone(),
            size: file.size,
            sha256: file.sha256.clone(),
        },
    )?;
    if length == 0 {
        return Ok(0);
    }
    let mut input = open_existing_part(path, false)?;
    let mut buffer = [0u8; COPY_BUFFER_BYTES];
    let mut read = 0u64;
    loop {
        let count = input
            .read(&mut buffer)
            .map_err(|error| io_error(path, error))?;
        if count == 0 {
            break;
        }
        read = read
            .checked_add(count as u64)
            .ok_or(DownloadError::ByteCeiling)?;
        hasher.update(&buffer[..count]);
    }
    if read != length {
        return Err(DownloadError::SizeMismatch {
            path: file.path.clone(),
            expected: length,
            actual: read,
        });
    }
    Ok(length)
}

fn open_part_for_append(path: &Path) -> Result<File, DownloadError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() {
                return Err(DownloadError::SymlinkPath {
                    path: path.display().to_string(),
                });
            }
            open_existing_part(path, true)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .map_err(|error| io_error(path, error)),
        Err(error) => Err(io_error(path, error)),
    }
}

/// Open a pre-existing `.part` without following a symlink on Unix. The
/// preceding metadata check gives a clear error; O_NOFOLLOW closes the swap
/// window between that check and the actual file descriptor acquisition.
fn open_existing_part(path: &Path, append: bool) -> Result<File, DownloadError> {
    let mut options = OpenOptions::new();
    if append {
        options.append(true);
    } else {
        options.read(true);
    }
    #[cfg(unix)]
    options.custom_flags(libc::O_NOFOLLOW);
    options.open(path).map_err(|error| {
        if error.raw_os_error() == Some(libc::ELOOP) {
            DownloadError::SymlinkPath {
                path: path.display().to_string(),
            }
        } else {
            io_error(path, error)
        }
    })
}

fn validate_redirects(hosts: &[String]) -> Result<(), DownloadError> {
    if hosts.len() > MAX_REDIRECTS {
        return Err(DownloadError::RedirectPolicy {
            host: "redirect limit exceeded".into(),
        });
    }
    for host in hosts {
        if !allowed_download_host(host) {
            return Err(DownloadError::RedirectPolicy { host: host.clone() });
        }
    }
    Ok(())
}

fn matches_content_range(value: Option<&str>, start: u64, total: u64) -> bool {
    if total == 0 || start >= total {
        return false;
    }
    let expected = format!("bytes {start}-{}/{}", total - 1, total);
    value == Some(expected.as_str())
}

fn hex_digest(hasher: &Sha256) -> String {
    format!("{:x}", hasher.clone().finalize())
}

fn finalize_install(
    store: &CatalogStore,
    catalog_dir: &Path,
    staging: &Path,
    manifest: &DownloadManifest,
) -> Result<(), DownloadError> {
    let install = store.qwen_install_dir();
    if fs::symlink_metadata(&install).is_ok() {
        return Err(DownloadError::InstallExists);
    }
    for file in &manifest.files {
        let part = part_path(staging, file);
        let final_name = staging.join(&file.path);
        fs::rename(&part, &final_name).map_err(|error| io_error(&part, error))?;
    }
    let receipt = InstallReceipt {
        catalog_id: QWEN_CATALOG_ID.into(),
        revision: QWEN_REVISION.into(),
        manifest_sha256: store.expected_manifest_sha256(),
        installed_bytes: manifest.total_bytes,
        completed_at_ms: now_unix_ms(),
    };
    let receipt_path = staging.join("install-receipt.json");
    let mut receipt_file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&receipt_path)
        .map_err(|error| io_error(&receipt_path, error))?;
    let receipt_bytes =
        serde_json::to_vec(&receipt).map_err(|error| DownloadError::Manifest(error.to_string()))?;
    receipt_file
        .write_all(&receipt_bytes)
        .map_err(|error| io_error(&receipt_path, error))?;
    receipt_file
        .sync_all()
        .map_err(|error| io_error(&receipt_path, error))?;
    // Both paths live directly under the fixed catalog directory, so this is a
    // same-volume rename. The final model folder appears only after every file
    // and its receipt have been written and synced in staging.
    fs::rename(staging, &install).map_err(|error| io_error(catalog_dir, error))?;
    Ok(())
}

fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().try_into().unwrap_or(u64::MAX))
        .unwrap_or(0)
}

fn io_error(path: &Path, error: std::io::Error) -> DownloadError {
    DownloadError::Io {
        path: path.display().to_string(),
        reason: error.to_string(),
    }
}
