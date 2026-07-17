//! Fixed, app-owned model catalog and receipt validation.
//!
//! This module only describes two candidate model paths and reads a bounded
//! local receipt. It does not download, select, load, or launch a model.

use std::ffi::OsStr;
use std::path::PathBuf;

#[cfg(unix)]
use std::ffi::CString;
#[cfg(unix)]
use std::fs::File;
#[cfg(unix)]
use std::io::Read;
#[cfg(unix)]
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
#[cfg(unix)]
use std::os::unix::ffi::OsStrExt;
#[cfg(unix)]
use std::os::unix::fs::MetadataExt;
#[cfg(unix)]
use std::path::{Component, Path};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const APPLE_CATALOG_ID: &str = "apple-system";
pub const QWEN_CATALOG_ID: &str = "qwen-coder-1.5b-mlx-4bit";
pub const QWEN_REVISION: &str = "b3252a2f97102b1fb1571fec2c9b27219a8536be";
pub const QWEN_REPORTED_BYTES: u64 = 868_628_559;

const CATALOG_MANIFEST_BYTES: &[u8] = include_bytes!("catalog_manifest.json");
const RECEIPT_NAME: &str = "install-receipt.json";
const MAX_RECEIPT_BYTES: u64 = 16 * 1024;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CatalogEntry {
    pub id: String,
    pub display_name: String,
    pub subtitle: String,
    pub provider_id: String,
    pub model_id: String,
    pub state: CatalogState,
    pub availability_reason: Option<String>,
    pub download_bytes: Option<u64>,
    pub license: String,
    pub source_url: Option<String>,
    pub revision: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
#[allow(dead_code)] // Later catalog tasks own the available/running/failed transitions.
pub enum CatalogState {
    Available,
    Unavailable,
    Absent,
    Installed,
    Running,
    Failed,
}

#[derive(Debug, thiserror::Error)]
pub enum CatalogError {
    #[error("embedded catalog manifest is invalid: {0}")]
    InvalidManifest(#[from] serde_json::Error),
    #[error("embedded catalog manifest is unexpected: {0}")]
    UnexpectedManifest(String),
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ManifestEntry {
    id: String,
    display_name: String,
    subtitle: String,
    provider_id: String,
    model_id: String,
    availability_reason: Option<String>,
    download_bytes: Option<u64>,
    license: String,
    source_url: Option<String>,
    revision: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct InstallReceipt {
    pub catalog_id: String,
    pub revision: String,
    pub manifest_sha256: String,
    pub installed_bytes: u64,
    pub completed_at_ms: u64,
}

#[derive(Debug)]
pub struct CatalogStore {
    app_data_dir: PathBuf,
}

impl CatalogStore {
    pub fn new(app_data_dir: PathBuf) -> Self {
        Self { app_data_dir }
    }

    pub fn list(&self) -> Result<Vec<CatalogEntry>, CatalogError> {
        let manifest: Vec<ManifestEntry> = serde_json::from_slice(CATALOG_MANIFEST_BYTES)?;
        validate_manifest(&manifest)?;
        Ok(manifest
            .into_iter()
            .map(|entry| self.catalog_entry(entry))
            .collect())
    }

    #[allow(dead_code)] // Task 2 writes receipts under this fixed path.
    pub fn qwen_install_dir(&self) -> PathBuf {
        self.app_data_dir
            .join("models")
            .join("catalog")
            .join(QWEN_CATALOG_ID)
            .join(QWEN_REVISION)
    }

    /// Test-only path helper for seeding resumable state. Production downloader
    /// authority stays descriptor-rooted and never re-resolves this pathname.
    #[cfg(test)]
    pub(crate) fn staging_dir(&self) -> PathBuf {
        self.app_data_dir
            .join("models")
            .join("catalog")
            .join(QWEN_CATALOG_ID)
            .join(format!(".{QWEN_REVISION}.part"))
    }

    /// Downloader-only access to the app-owned root. IPC callers never supply
    /// this path, so downloads cannot be redirected into a project directory.
    pub(crate) fn app_data_dir(&self) -> &std::path::Path {
        &self.app_data_dir
    }

    pub(crate) fn expected_manifest_sha256(&self) -> String {
        format!("{:x}", Sha256::digest(CATALOG_MANIFEST_BYTES))
    }

    fn catalog_entry(&self, entry: ManifestEntry) -> CatalogEntry {
        let state = match entry.id.as_str() {
            APPLE_CATALOG_ID => CatalogState::Unavailable,
            QWEN_CATALOG_ID if self.qwen_receipt_is_valid() => CatalogState::Installed,
            QWEN_CATALOG_ID => CatalogState::Absent,
            _ => CatalogState::Unavailable,
        };
        CatalogEntry {
            id: entry.id,
            display_name: entry.display_name,
            subtitle: entry.subtitle,
            provider_id: entry.provider_id,
            model_id: entry.model_id,
            state,
            availability_reason: entry.availability_reason,
            download_bytes: entry.download_bytes,
            license: entry.license,
            source_url: entry.source_url,
            revision: entry.revision,
        }
    }

    fn qwen_receipt_is_valid(&self) -> bool {
        self.qwen_receipt_is_valid_after_directory_open(|_| {})
    }

    #[cfg(test)]
    pub(crate) fn qwen_receipt_is_valid_with_hook<F>(&self, after_directory_open: F) -> bool
    where
        F: FnMut(&OsStr),
    {
        self.qwen_receipt_is_valid_after_directory_open(after_directory_open)
    }

    fn qwen_receipt_is_valid_after_directory_open<F>(&self, after_directory_open: F) -> bool
    where
        F: FnMut(&OsStr),
    {
        let Some(receipt) =
            read_bounded_receipt_from_app_data(&self.app_data_dir, after_directory_open)
        else {
            return false;
        };
        receipt.catalog_id == QWEN_CATALOG_ID
            && receipt.revision == QWEN_REVISION
            && receipt.manifest_sha256 == self.expected_manifest_sha256()
    }
}

fn validate_manifest(entries: &[ManifestEntry]) -> Result<(), CatalogError> {
    let qwen = entries
        .iter()
        .find(|entry| entry.id == QWEN_CATALOG_ID)
        .ok_or_else(|| CatalogError::UnexpectedManifest("missing Qwen entry".into()))?;
    if entries.len() != 2
        || entries.first().map(|entry| entry.id.as_str()) != Some(APPLE_CATALOG_ID)
        || qwen.revision.as_deref() != Some(QWEN_REVISION)
        || qwen.download_bytes != Some(QWEN_REPORTED_BYTES)
        || qwen.license != "Apache-2.0"
    {
        return Err(CatalogError::UnexpectedManifest(
            "catalog entries do not match the fixed Apple and Qwen identities".into(),
        ));
    }
    Ok(())
}

/// Opens every path component from the filesystem root by descriptor. A name
/// swap after one component has opened cannot redirect later `openat` calls,
/// and `O_NOFOLLOW` rejects a symlink before the kernel resolves it.
#[cfg(unix)]
fn read_bounded_receipt_from_app_data<F>(
    app_data_dir: &Path,
    after_directory_open: F,
) -> Option<InstallReceipt>
where
    F: FnMut(&OsStr),
{
    let mut after_directory_open = after_directory_open;
    let mut directory = open_app_data_directory(app_data_dir)?;
    for component in ["models", "catalog", QWEN_CATALOG_ID, QWEN_REVISION] {
        directory = open_directory_at(directory.as_raw_fd(), OsStr::new(component))?;
        after_directory_open(OsStr::new(component));
    }
    let name = CString::new(RECEIPT_NAME).ok()?;
    let fd = unsafe {
        libc::openat(
            directory.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK,
        )
    };
    if fd < 0 {
        return None;
    }
    let file = unsafe { File::from_raw_fd(fd) };
    let metadata = file.metadata().ok()?;
    if !metadata.is_file() || metadata.nlink() != 1 {
        return None;
    }
    if metadata.len() > MAX_RECEIPT_BYTES {
        return None;
    }
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(metadata.len() as usize).ok()?;
    file.take(MAX_RECEIPT_BYTES + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() as u64 > MAX_RECEIPT_BYTES || bytes.len() as u64 != metadata.len() {
        return None;
    }
    serde_json::from_slice(&bytes).ok()
}

#[cfg(not(unix))]
fn read_bounded_receipt_from_app_data<F>(
    _app_data_dir: &std::path::Path,
    _after_directory_open: F,
) -> Option<InstallReceipt>
where
    F: FnMut(&std::ffi::OsStr),
{
    None
}

#[cfg(unix)]
fn open_app_data_directory(path: &Path) -> Option<OwnedFd> {
    if !path.is_absolute() {
        return None;
    }
    let slash = CString::new("/").ok()?;
    let fd = unsafe {
        libc::open(
            slash.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        return None;
    }
    let mut directory = unsafe { OwnedFd::from_raw_fd(fd) };
    for component in path.components() {
        match component {
            Component::RootDir => {}
            Component::Normal(name) => {
                directory = open_directory_at(directory.as_raw_fd(), name)?;
            }
            Component::CurDir | Component::ParentDir | Component::Prefix(_) => return None,
        }
    }
    Some(directory)
}

#[cfg(unix)]
fn open_directory_at(parent: RawFd, name: &OsStr) -> Option<OwnedFd> {
    let name = CString::new(name.as_bytes()).ok()?;
    let fd = unsafe {
        libc::openat(
            parent,
            name.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        return None;
    }
    Some(unsafe { OwnedFd::from_raw_fd(fd) })
}
