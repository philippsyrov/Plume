//! Fixed, app-owned model catalog and receipt validation.
//!
//! This module only describes two candidate model paths and reads a bounded
//! local receipt. It does not download, select, load, or launch a model.

use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};

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

    pub fn qwen_install_dir(&self) -> PathBuf {
        self.app_data_dir
            .join("models")
            .join("catalog")
            .join(QWEN_CATALOG_ID)
            .join(QWEN_REVISION)
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
        let receipt_path = self.qwen_install_dir().join(RECEIPT_NAME);
        if !self.is_safe_catalog_path(&receipt_path) {
            return false;
        }
        let Some(receipt) = read_bounded_receipt(&receipt_path) else {
            return false;
        };
        receipt.catalog_id == QWEN_CATALOG_ID
            && receipt.revision == QWEN_REVISION
            && receipt.manifest_sha256 == self.expected_manifest_sha256()
    }

    /// The catalog path is entirely backend-derived. Still, every existing
    /// component is checked before opening the receipt so a planted symlink
    /// cannot turn an installed-state read into a read elsewhere on disk.
    fn is_safe_catalog_path(&self, receipt_path: &Path) -> bool {
        let Ok(relative) = receipt_path.strip_prefix(&self.app_data_dir) else {
            return false;
        };
        if is_symlink_or_missing(&self.app_data_dir) {
            return false;
        }

        let mut current = self.app_data_dir.clone();
        for component in relative.components() {
            current.push(component.as_os_str());
            if is_symlink_or_missing(&current) {
                return false;
            }
        }
        fs::symlink_metadata(receipt_path)
            .map(|metadata| metadata.file_type().is_file())
            .unwrap_or(false)
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

fn is_symlink_or_missing(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .map(|metadata| metadata.file_type().is_symlink())
        .unwrap_or(true)
}

fn read_bounded_receipt(path: &Path) -> Option<InstallReceipt> {
    let metadata = fs::metadata(path).ok()?;
    if metadata.len() > MAX_RECEIPT_BYTES {
        return None;
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    File::open(path)
        .ok()?
        .take(MAX_RECEIPT_BYTES + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() as u64 > MAX_RECEIPT_BYTES {
        return None;
    }
    serde_json::from_slice(&bytes).ok()
}
