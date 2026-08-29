//! Immutable, session-owned research artifact bundle store.

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::browser::screenshot_evidence::{
    BROWSER_SCREENSHOT_BYTE_CAP, BROWSER_SCREENSHOT_DIMENSION_CAP,
};
use crate::sessions::owner::{ResolvedSessionOwner, SessionOwnerScope};

use super::evidence::{ResearchEvidenceSource, ResearchScreenshotSource};

#[path = "bundle_delete.rs"]
mod bundle_delete;
use bundle_delete::reconcile_tombstones;
pub(crate) use bundle_delete::{
    delete_local_session_with_artifacts, delete_project_session_with_artifacts, ArtifactDeleteError,
};
#[cfg(test)]
pub(crate) use bundle_delete::{
    fail_delete_after_staging_for_test, stage_interrupted_delete_for_test,
};

const BUNDLE_SCHEMA_VERSION: u32 = 1;
const MAX_BUNDLE_BYTES: usize = 4 * 1024 * 1024;
const MAX_SESSION_BUNDLE_BYTES: u64 = 32 * 1024 * 1024;
const MAX_TEXT_FIELD_BYTES: usize = 256 * 1024;
const MAX_SUMMARIES: usize = 10;
const MAX_DRAFTS: usize = 3;
pub(crate) const MAX_ARTIFACT_RECORDS: usize = 50;

static STORE_MUTEX: OnceLock<Mutex<()>> = OnceLock::new();
static NEXT_ID: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) enum ArtifactOwnerScope {
    Local,
    Project,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) enum ArtifactCitationStatus {
    Verified,
    NeedsReview,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) enum ArtifactOutcome {
    Complete,
    NeedsReview,
    Stopped,
    Failed,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct BundleSourceSummary {
    pub source_id: String,
    pub summary: String,
    pub logical_turn: u32,
    pub provider_calls: u32,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct BundleDraft {
    pub markdown: String,
    pub citation_status: ArtifactCitationStatus,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ArtifactBundleInput {
    pub user_request: String,
    pub provider_id: String,
    pub model_id: String,
    pub runtime_id: String,
    pub sources: Vec<ResearchEvidenceSource>,
    #[serde(default)]
    pub screenshot_sources: Vec<ResearchScreenshotSource>,
    pub summaries: Vec<BundleSourceSummary>,
    pub drafts: Vec<BundleDraft>,
    pub logical_turns: u32,
    pub provider_calls: u32,
    pub duration_ms: u64,
    pub outcome: ArtifactOutcome,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ArtifactBundleRecord {
    pub schema_version: u32,
    pub artifact_id: String,
    pub artifact_version: u32,
    pub owner_scope: ArtifactOwnerScope,
    pub session_id: String,
    pub project_id: Option<String>,
    pub created_at_ms: u64,
    pub input: ArtifactBundleInput,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum ArtifactStoreError {
    #[error("research artifact was not found")]
    NotFound,
    #[error("research artifact store refused an unsafe path: {0}")]
    Refused(String),
    #[error("research artifact store limit: {0}")]
    Limit(String),
    #[error("research artifact record is corrupt: {0}")]
    Corrupt(String),
    #[error("research artifact storage failed: {0}")]
    Storage(String),
}

#[derive(Debug)]
pub(crate) struct ArtifactStore {
    sessions_dir: PathBuf,
    session_root: PathBuf,
    owner_scope: ArtifactOwnerScope,
    session_id: String,
    project_id: Option<String>,
}

impl ArtifactStore {
    pub(crate) fn from_owner(owner: &ResolvedSessionOwner) -> Result<Self, ArtifactStoreError> {
        let (base, owner_scope, project_id) = match owner.scope {
            SessionOwnerScope::Local => {
                let app_data = owner.sessions_dir.parent().ok_or_else(|| {
                    ArtifactStoreError::Storage(
                        "local sessions directory has no app-data parent".into(),
                    )
                })?;
                (
                    app_data.join("research-artifacts"),
                    ArtifactOwnerScope::Local,
                    None,
                )
            }
            SessionOwnerScope::Project => {
                let project = owner.project.as_ref().ok_or_else(|| {
                    ArtifactStoreError::Refused("missing project generation".into())
                })?;
                (
                    project.root.join(".plume").join("research-artifacts"),
                    ArtifactOwnerScope::Project,
                    Some(project.id.clone()),
                )
            }
        };
        Ok(Self {
            sessions_dir: owner.sessions_dir.clone(),
            session_root: base.join(&owner.session_id),
            owner_scope,
            session_id: owner.session_id.clone(),
            project_id,
        })
    }

    pub(crate) fn stage_new(
        &self,
        input: ArtifactBundleInput,
    ) -> Result<ArtifactBundleRecord, ArtifactStoreError> {
        self.with_lock(|store| store.publish(mint_artifact_id(), 1, input))
    }

    #[cfg(test)]
    pub(crate) fn stage_revision(
        &self,
        artifact_id: &str,
        input: ArtifactBundleInput,
    ) -> Result<ArtifactBundleRecord, ArtifactStoreError> {
        validate_artifact_id(artifact_id)?;
        self.with_lock(|store| {
            let versions = store.versions_for(artifact_id)?;
            let latest = versions
                .into_iter()
                .max()
                .ok_or(ArtifactStoreError::NotFound)?;
            let next = latest
                .checked_add(1)
                .ok_or_else(|| ArtifactStoreError::Limit("artifact version overflow".into()))?;
            store.publish(artifact_id.to_string(), next, input)
        })
    }

    pub(crate) fn load_version(
        &self,
        artifact_id: &str,
        version: u32,
    ) -> Result<ArtifactBundleRecord, ArtifactStoreError> {
        validate_artifact_id(artifact_id)?;
        if version == 0 {
            return Err(ArtifactStoreError::NotFound);
        }
        self.with_lock(|store| store.read_record(artifact_id, version))
    }

    pub(crate) fn load_latest(
        &self,
        artifact_id: &str,
    ) -> Result<ArtifactBundleRecord, ArtifactStoreError> {
        validate_artifact_id(artifact_id)?;
        self.with_lock(|store| {
            let mut versions = store.versions_for(artifact_id)?;
            versions.sort_unstable_by(|a, b| b.cmp(a));
            for version in versions {
                match store.read_record(artifact_id, version) {
                    Ok(record) => return Ok(record),
                    Err(ArtifactStoreError::Corrupt(_)) => {
                        store.quarantine(artifact_id, version)?;
                    }
                    Err(error) => return Err(error),
                }
            }
            Err(ArtifactStoreError::NotFound)
        })
    }

    pub(crate) fn list(&self) -> Result<Vec<ArtifactBundleRecord>, ArtifactStoreError> {
        self.with_lock(|store| {
            let mut records = Vec::new();
            for (artifact_id, version) in store.record_entries()? {
                records.push(store.read_record(&artifact_id, version)?);
            }
            records.sort_by(|a, b| {
                a.artifact_id
                    .cmp(&b.artifact_id)
                    .then_with(|| a.artifact_version.cmp(&b.artifact_version))
            });
            Ok(records)
        })
    }

    fn with_lock<T>(
        &self,
        operation: impl FnOnce(&Self) -> Result<T, ArtifactStoreError>,
    ) -> Result<T, ArtifactStoreError> {
        let mutex = STORE_MUTEX.get_or_init(|| Mutex::new(()));
        let _guard = mutex
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let base = self.base_root()?;
        ensure_directory_chain(base)?;
        let _process_lock = ProcessLock::acquire(base)?;
        reconcile_tombstones(base, &self.sessions_dir)?;
        if !crate::sessions::session_exists(&self.sessions_dir, &self.session_id)
            .map_err(|error| ArtifactStoreError::Storage(error.to_string()))?
        {
            return Err(ArtifactStoreError::NotFound);
        }
        self.ensure_root()?;
        self.reconcile_temp_files()?;
        operation(self)
    }

    fn publish(
        &self,
        artifact_id: String,
        artifact_version: u32,
        input: ArtifactBundleInput,
    ) -> Result<ArtifactBundleRecord, ArtifactStoreError> {
        validate_input(&input)?;
        let record = ArtifactBundleRecord {
            schema_version: BUNDLE_SCHEMA_VERSION,
            artifact_id: artifact_id.clone(),
            artifact_version,
            owner_scope: self.owner_scope,
            session_id: self.session_id.clone(),
            project_id: self.project_id.clone(),
            created_at_ms: now_ms(),
            input,
        };
        let bytes = serde_json::to_vec(&record)
            .map_err(|error| ArtifactStoreError::Storage(format!("serialize bundle: {error}")))?;
        if bytes.len() > MAX_BUNDLE_BYTES {
            return Err(ArtifactStoreError::Limit(
                "one bundle exceeds the 4 MiB cap".into(),
            ));
        }
        self.enforce_capacity(bytes.len())?;
        let target = self.record_path(&artifact_id, artifact_version);
        refuse_path_surprises(&target)?;
        if target.exists() {
            return Err(ArtifactStoreError::Refused(
                "immutable artifact version already exists".into(),
            ));
        }
        write_atomic(&self.session_root, &target, &bytes)?;
        Ok(record)
    }

    fn read_record(
        &self,
        artifact_id: &str,
        version: u32,
    ) -> Result<ArtifactBundleRecord, ArtifactStoreError> {
        let path = self.record_path(artifact_id, version);
        let bytes = read_regular_single_link(&path)?;
        if bytes.len() > MAX_BUNDLE_BYTES {
            return Err(ArtifactStoreError::Corrupt(
                "record exceeds byte cap".into(),
            ));
        }
        let record: ArtifactBundleRecord = serde_json::from_slice(&bytes)
            .map_err(|error| ArtifactStoreError::Corrupt(format!("parse bundle: {error}")))?;
        if record.schema_version != BUNDLE_SCHEMA_VERSION
            || record.artifact_id != artifact_id
            || record.artifact_version != version
            || record.owner_scope != self.owner_scope
            || record.session_id != self.session_id
            || record.project_id != self.project_id
        {
            return Err(ArtifactStoreError::Corrupt(
                "record identity or version mismatch".into(),
            ));
        }
        validate_input(&record.input)
            .map_err(|error| ArtifactStoreError::Corrupt(error.to_string()))?;
        Ok(record)
    }

    fn ensure_root(&self) -> Result<(), ArtifactStoreError> {
        ensure_directory_chain(&self.session_root)
    }

    fn base_root(&self) -> Result<&Path, ArtifactStoreError> {
        self.session_root.parent().ok_or_else(|| {
            ArtifactStoreError::Storage("artifact session root has no parent".into())
        })
    }

    fn record_entries(&self) -> Result<Vec<(String, u32)>, ArtifactStoreError> {
        let mut entries = Vec::new();
        for entry in fs::read_dir(&self.session_root)
            .map_err(|error| ArtifactStoreError::Storage(format!("list bundles: {error}")))?
        {
            let entry = entry.map_err(|error| {
                ArtifactStoreError::Storage(format!("read bundle entry: {error}"))
            })?;
            if let Some(parsed) = parse_record_name(&entry.file_name().to_string_lossy()) {
                entries.push(parsed);
            }
        }
        Ok(entries)
    }

    fn versions_for(&self, artifact_id: &str) -> Result<Vec<u32>, ArtifactStoreError> {
        Ok(self
            .record_entries()?
            .into_iter()
            .filter_map(|(id, version)| (id == artifact_id).then_some(version))
            .collect())
    }

    fn enforce_capacity(&self, pending_bytes: usize) -> Result<(), ArtifactStoreError> {
        let entries = self.record_entries()?;
        if entries.len() >= MAX_ARTIFACT_RECORDS {
            return Err(ArtifactStoreError::Limit(
                "session artifact record cap reached".into(),
            ));
        }
        let bytes = entries
            .into_iter()
            .try_fold(0_u64, |total, (id, version)| {
                let metadata =
                    fs::symlink_metadata(self.record_path(&id, version)).map_err(|error| {
                        ArtifactStoreError::Storage(format!("inspect bundle usage: {error}"))
                    })?;
                if metadata.file_type().is_symlink() || !metadata.is_file() {
                    return Err(ArtifactStoreError::Refused(
                        "bundle record is not a regular file".into(),
                    ));
                }
                total.checked_add(metadata.len()).ok_or_else(|| {
                    ArtifactStoreError::Limit("session artifact byte accounting overflow".into())
                })
            })?;
        let projected = bytes.checked_add(pending_bytes as u64).ok_or_else(|| {
            ArtifactStoreError::Limit("session artifact byte accounting overflow".into())
        })?;
        if projected > MAX_SESSION_BUNDLE_BYTES {
            return Err(ArtifactStoreError::Limit(
                "session artifact byte cap reached".into(),
            ));
        }
        Ok(())
    }

    fn reconcile_temp_files(&self) -> Result<(), ArtifactStoreError> {
        for entry in fs::read_dir(&self.session_root)
            .map_err(|error| ArtifactStoreError::Storage(format!("scan temp bundles: {error}")))?
        {
            let entry = entry.map_err(|error| {
                ArtifactStoreError::Storage(format!("read temp entry: {error}"))
            })?;
            if entry.file_name().to_string_lossy().starts_with(".tmp-") {
                refuse_path_surprises(&entry.path())?;
                fs::remove_file(entry.path()).map_err(|error| {
                    ArtifactStoreError::Storage(format!("remove interrupted temp bundle: {error}"))
                })?;
            }
        }
        Ok(())
    }

    fn quarantine(&self, artifact_id: &str, version: u32) -> Result<(), ArtifactStoreError> {
        let source = self.record_path(artifact_id, version);
        refuse_path_surprises(&source)?;
        let target = self.session_root.join(format!(
            ".corrupt-{}-{:016x}",
            record_name(artifact_id, version),
            NEXT_ID.fetch_add(1, Ordering::Relaxed)
        ));
        refuse_path_surprises(&target)?;
        fs::rename(source, target)
            .map_err(|error| ArtifactStoreError::Storage(format!("quarantine bundle: {error}")))
    }

    fn record_path(&self, artifact_id: &str, version: u32) -> PathBuf {
        self.session_root.join(record_name(artifact_id, version))
    }

    #[cfg(test)]
    pub(crate) fn record_path_for_test(&self, artifact_id: &str, version: u32) -> PathBuf {
        self.record_path(artifact_id, version)
    }

    #[cfg(test)]
    pub(crate) fn session_root_for_test(&self) -> &Path {
        &self.session_root
    }

    #[cfg(test)]
    pub(crate) fn sessions_dir_for_test(&self) -> &Path {
        &self.sessions_dir
    }

    #[cfg(test)]
    pub(crate) fn session_id_for_test(&self) -> &str {
        &self.session_id
    }
}

fn validate_input(input: &ArtifactBundleInput) -> Result<(), ArtifactStoreError> {
    for (label, value) in [
        ("user request", input.user_request.as_str()),
        ("provider id", input.provider_id.as_str()),
        ("model id", input.model_id.as_str()),
        ("runtime id", input.runtime_id.as_str()),
    ] {
        if value.trim().is_empty() || value.len() > MAX_TEXT_FIELD_BYTES {
            return Err(ArtifactStoreError::Limit(format!(
                "{label} is empty or oversized"
            )));
        }
    }
    if input.sources.is_empty()
        || input.sources.len() > MAX_SUMMARIES
        || input.summaries.len() != input.sources.len()
        || input.drafts.is_empty()
        || input.drafts.len() > MAX_DRAFTS
    {
        return Err(ArtifactStoreError::Limit(
            "bundle source, summary, or draft count is invalid".into(),
        ));
    }
    for source in &input.sources {
        let digest = format!("{:x}", Sha256::digest(source.content.as_bytes()));
        if source.content.len() as u64 != source.bytes || digest != source.sha256 {
            return Err(ArtifactStoreError::Refused(
                "bundle source bytes or hash changed".into(),
            ));
        }
    }
    if input
        .sources
        .len()
        .saturating_add(input.screenshot_sources.len())
        > MAX_SUMMARIES
    {
        return Err(ArtifactStoreError::Limit(
            "bundle text and screenshot source count is invalid".into(),
        ));
    }
    let mut screenshot_ids = std::collections::HashSet::new();
    for screenshot in &input.screenshot_sources {
        if !valid_screenshot_id(&screenshot.evidence_id)
            || !screenshot_ids.insert(&screenshot.evidence_id)
            || screenshot.source_url.trim().is_empty()
            || screenshot.source_url.len() > MAX_TEXT_FIELD_BYTES
            || screenshot
                .title
                .as_ref()
                .is_some_and(|title| title.len() > 512)
            || screenshot.width == 0
            || screenshot.height == 0
            || screenshot.width > BROWSER_SCREENSHOT_DIMENSION_CAP
            || screenshot.height > BROWSER_SCREENSHOT_DIMENSION_CAP
            || screenshot.bytes == 0
            || screenshot.bytes > BROWSER_SCREENSHOT_BYTE_CAP as u64
            || screenshot.sha256.len() != 64
            || !screenshot
                .sha256
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(ArtifactStoreError::Refused(
                "bundle screenshot provenance is invalid".into(),
            ));
        }
    }
    if input
        .summaries
        .iter()
        .any(|summary| summary.summary.is_empty() || summary.summary.len() > 16 * 1024)
        || input
            .drafts
            .iter()
            .any(|draft| draft.markdown.is_empty() || draft.markdown.len() > MAX_TEXT_FIELD_BYTES)
    {
        return Err(ArtifactStoreError::Limit(
            "bundle summary or draft is empty or oversized".into(),
        ));
    }
    Ok(())
}

fn valid_screenshot_id(id: &str) -> bool {
    id.len() == 35 && id.starts_with("bs_") && id[3..].bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn record_name(artifact_id: &str, version: u32) -> String {
    format!("{artifact_id}-v{version:06}.json")
}

fn parse_record_name(name: &str) -> Option<(String, u32)> {
    let stem = name.strip_suffix(".json")?;
    let (artifact_id, version) = stem.rsplit_once("-v")?;
    validate_artifact_id(artifact_id).ok()?;
    if version.len() != 6 || !version.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let version = version.parse().ok()?;
    (version > 0).then(|| (artifact_id.to_string(), version))
}

fn validate_artifact_id(id: &str) -> Result<(), ArtifactStoreError> {
    if id.len() == 35
        && id.starts_with("ra_")
        && id[3..].bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        Ok(())
    } else {
        Err(ArtifactStoreError::Refused(
            "invalid research artifact id".into(),
        ))
    }
}

fn mint_artifact_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos() as u64)
        .unwrap_or(0);
    let counter = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    format!("ra_{nanos:016x}{:08x}{counter:08x}", std::process::id())
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

fn ensure_directory_chain(path: &Path) -> Result<(), ArtifactStoreError> {
    if let Some(parent) = path.parent() {
        if parent.exists() {
            refuse_directory(parent)?;
        } else {
            ensure_directory_chain(parent)?;
        }
    }
    if !path.exists() {
        fs::create_dir(path).map_err(|error| {
            ArtifactStoreError::Storage(format!("create artifact directory: {error}"))
        })?;
    }
    refuse_directory(path)
}

fn refuse_directory(path: &Path) -> Result<(), ArtifactStoreError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        ArtifactStoreError::Storage(format!("inspect artifact directory: {error}"))
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        Err(ArtifactStoreError::Refused(
            "artifact directory is not a real directory".into(),
        ))
    } else {
        Ok(())
    }
}

fn refuse_path_surprises(path: &Path) -> Result<(), ArtifactStoreError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(ArtifactStoreError::Refused(
            "artifact path is a symlink".into(),
        )),
        Ok(metadata) if metadata.is_file() => ensure_single_link(&metadata),
        Ok(_) => Err(ArtifactStoreError::Refused(
            "artifact path is not a regular file".into(),
        )),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(ArtifactStoreError::Storage(format!(
            "inspect artifact path: {error}"
        ))),
    }
}

fn read_regular_single_link(path: &Path) -> Result<Vec<u8>, ArtifactStoreError> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(ArtifactStoreError::NotFound)
        }
        Err(error) => {
            return Err(ArtifactStoreError::Storage(format!(
                "inspect bundle: {error}"
            )))
        }
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(ArtifactStoreError::Refused(
            "bundle is not a regular file".into(),
        ));
    }
    ensure_single_link(&metadata)?;
    let file = OpenOptions::new()
        .read(true)
        .open(path)
        .map_err(|error| ArtifactStoreError::Storage(format!("open bundle: {error}")))?;
    let mut bytes = Vec::new();
    file.take((MAX_BUNDLE_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|error| ArtifactStoreError::Storage(format!("read bundle: {error}")))?;
    Ok(bytes)
}

#[cfg(unix)]
fn ensure_single_link(metadata: &fs::Metadata) -> Result<(), ArtifactStoreError> {
    use std::os::unix::fs::MetadataExt;
    if metadata.nlink() == 1 {
        Ok(())
    } else {
        Err(ArtifactStoreError::Refused(
            "artifact file has more than one hard link".into(),
        ))
    }
}

#[cfg(not(unix))]
fn ensure_single_link(_metadata: &fs::Metadata) -> Result<(), ArtifactStoreError> {
    Err(ArtifactStoreError::Refused(
        "artifact single-link validation is unavailable".into(),
    ))
}

fn write_atomic(root: &Path, target: &Path, bytes: &[u8]) -> Result<(), ArtifactStoreError> {
    let temp = root.join(format!(
        ".tmp-{:016x}",
        NEXT_ID.fetch_add(1, Ordering::Relaxed)
    ));
    refuse_path_surprises(&temp)?;
    let result = (|| {
        let mut file = secure_create_new(&temp)?;
        file.write_all(bytes)
            .map_err(|error| ArtifactStoreError::Storage(format!("write temp bundle: {error}")))?;
        file.sync_all()
            .map_err(|error| ArtifactStoreError::Storage(format!("sync temp bundle: {error}")))?;
        fs::rename(&temp, target)
            .map_err(|error| ArtifactStoreError::Storage(format!("publish bundle: {error}")))?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

fn secure_create_new(path: &Path) -> Result<File, ArtifactStoreError> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    }
    options
        .open(path)
        .map_err(|error| ArtifactStoreError::Storage(format!("create temp bundle: {error}")))
}

#[cfg(unix)]
struct ProcessLock(File);

#[cfg(unix)]
impl ProcessLock {
    fn acquire(base_root: &Path) -> Result<Self, ArtifactStoreError> {
        use std::os::fd::AsRawFd;
        use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
        let path = base_root.join(".process.lock");
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(path)
            .map_err(|error| ArtifactStoreError::Refused(format!("open process lock: {error}")))?;
        let metadata = file.metadata().map_err(|error| {
            ArtifactStoreError::Storage(format!("inspect process lock: {error}"))
        })?;
        if !metadata.is_file() || metadata.nlink() != 1 {
            return Err(ArtifactStoreError::Refused(
                "artifact process lock is not a single-link regular file".into(),
            ));
        }
        // SAFETY: `file` owns a live descriptor until this guard drops.
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) } != 0 {
            return Err(ArtifactStoreError::Storage(format!(
                "lock artifact store: {}",
                std::io::Error::last_os_error()
            )));
        }
        Ok(Self(file))
    }
}

#[cfg(unix)]
impl Drop for ProcessLock {
    fn drop(&mut self) {
        use std::os::fd::AsRawFd;
        // SAFETY: descriptor remains live through this call.
        unsafe {
            libc::flock(self.0.as_raw_fd(), libc::LOCK_UN);
        }
    }
}

#[cfg(not(unix))]
struct ProcessLock;

#[cfg(not(unix))]
impl ProcessLock {
    fn acquire(_base_root: &Path) -> Result<Self, ArtifactStoreError> {
        Err(ArtifactStoreError::Refused(
            "artifact process locking is unavailable".into(),
        ))
    }
}
