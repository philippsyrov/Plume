//! App-private Browser evidence owned by one local chat session.
//!
//! The underlying immutable text and screenshot stores stay shared with
//! project evidence. This wrapper supplies a backend-derived synthetic
//! root beneath app data, verifies the local owner exists, and provides
//! the tombstone protocol used when that chat is deleted.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

use super::evidence::{
    read_text_evidence, store_text_evidence, BrowserEvidenceError, BrowserEvidenceRecord,
    BrowserEvidenceSummary, CapturedBrowserText,
};
use super::screenshot_evidence::{
    read_screenshot_evidence, store_screenshot_evidence, BrowserScreenshotError,
    BrowserScreenshotSummary, CapturedBrowserScreenshot, StoredBrowserScreenshot,
};
use crate::sessions::{self, SessionStoreError};

const LOCAL_BROWSER_SESSIONS_DIR: &str = "browser-sessions";

static LOCAL_STORE_MUTEX: OnceLock<Mutex<()>> = OnceLock::new();
static TOMBSTONE_NONCE: AtomicU64 = AtomicU64::new(0);

#[cfg(unix)]
pub(crate) struct LocalEvidenceProcessLock {
    file: fs::File,
}

#[cfg(unix)]
impl Drop for LocalEvidenceProcessLock {
    fn drop(&mut self) {
        use std::os::fd::AsRawFd;

        // SAFETY: `file` owns a live descriptor for the lock file until this
        // drop completes. Unlock failure cannot be usefully recovered here;
        // closing the descriptor immediately afterward also releases flock.
        unsafe {
            libc::flock(self.file.as_raw_fd(), libc::LOCK_UN);
        }
    }
}

#[cfg(not(unix))]
pub(crate) struct LocalEvidenceProcessLock;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct LocalEvidenceOwner {
    pub session_id: String,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum LocalEvidenceError {
    #[error("local Browser evidence owner was not found")]
    OwnerNotFound,
    #[error("invalid local Browser evidence owner")]
    InvalidOwner,
    #[error("{0}")]
    Refused(String),
    #[error("local Browser evidence capacity reached")]
    Capacity,
    #[error("{0}")]
    Storage(String),
    #[error(transparent)]
    Session(#[from] SessionStoreError),
}

#[derive(Debug)]
pub(crate) struct LocalEvidenceTombstone {
    local_sessions_dir: PathBuf,
    original: PathBuf,
    tombstone: PathBuf,
}

impl LocalEvidenceTombstone {
    pub(crate) fn tombstone_path(&self) -> &Path {
        &self.tombstone
    }
}

pub(crate) fn store_local_text_evidence(
    local_sessions_dir: &Path,
    owner: &LocalEvidenceOwner,
    capture: CapturedBrowserText,
) -> Result<BrowserEvidenceSummary, LocalEvidenceError> {
    let mutex = local_mutex();
    let _guard = mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let _process_guard = acquire_local_evidence_process_lock(local_sessions_dir)?;
    reconcile_local_evidence_tombstones_unlocked(local_sessions_dir)?;
    ensure_owner(local_sessions_dir, owner)?;
    let root = ensure_session_root(local_sessions_dir, owner)?;
    store_text_evidence(&root, capture).map_err(map_text_error)
}

pub(crate) fn read_local_text_evidence(
    local_sessions_dir: &Path,
    owner: &LocalEvidenceOwner,
    evidence_id: &str,
) -> Result<Option<BrowserEvidenceRecord>, LocalEvidenceError> {
    let mutex = local_mutex();
    let _guard = mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let _process_guard = acquire_local_evidence_process_lock(local_sessions_dir)?;
    reconcile_local_evidence_tombstones_unlocked(local_sessions_dir)?;
    ensure_owner(local_sessions_dir, owner)?;
    let root = session_evidence_root(local_sessions_dir, owner)?;
    refuse_symlink(&root, "local Browser session evidence root")?;
    read_text_evidence(&root, evidence_id).map_err(map_text_error)
}

pub(crate) fn store_local_screenshot_evidence(
    local_sessions_dir: &Path,
    owner: &LocalEvidenceOwner,
    capture: CapturedBrowserScreenshot,
) -> Result<BrowserScreenshotSummary, LocalEvidenceError> {
    let mutex = local_mutex();
    let _guard = mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let _process_guard = acquire_local_evidence_process_lock(local_sessions_dir)?;
    reconcile_local_evidence_tombstones_unlocked(local_sessions_dir)?;
    ensure_owner(local_sessions_dir, owner)?;
    let root = ensure_session_root(local_sessions_dir, owner)?;
    store_screenshot_evidence(&root, capture).map_err(map_screenshot_error)
}

pub(crate) fn read_local_screenshot_evidence(
    local_sessions_dir: &Path,
    owner: &LocalEvidenceOwner,
    evidence_id: &str,
) -> Result<Option<StoredBrowserScreenshot>, LocalEvidenceError> {
    let mutex = local_mutex();
    let _guard = mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let _process_guard = acquire_local_evidence_process_lock(local_sessions_dir)?;
    reconcile_local_evidence_tombstones_unlocked(local_sessions_dir)?;
    ensure_owner(local_sessions_dir, owner)?;
    let root = session_evidence_root(local_sessions_dir, owner)?;
    refuse_symlink(&root, "local Browser session evidence root")?;
    read_screenshot_evidence(&root, evidence_id).map_err(map_screenshot_error)
}

pub(crate) fn session_evidence_root(
    local_sessions_dir: &Path,
    owner: &LocalEvidenceOwner,
) -> Result<PathBuf, LocalEvidenceError> {
    validate_owner_shape(owner)?;
    let app_data = local_sessions_dir.parent().ok_or_else(|| {
        LocalEvidenceError::Storage("local sessions directory has no app-data parent".into())
    })?;
    Ok(app_data
        .join(LOCAL_BROWSER_SESSIONS_DIR)
        .join(&owner.session_id))
}

pub(crate) fn stage_local_evidence_delete(
    local_sessions_dir: &Path,
    owner: &LocalEvidenceOwner,
) -> Result<Option<LocalEvidenceTombstone>, LocalEvidenceError> {
    let mutex = local_mutex();
    let _guard = mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let _process_guard = acquire_local_evidence_process_lock(local_sessions_dir)?;
    reconcile_local_evidence_tombstones_unlocked(local_sessions_dir)?;
    ensure_owner(local_sessions_dir, owner)?;
    stage_delete_unlocked(local_sessions_dir, owner)
}

pub(crate) fn restore_local_evidence_delete(
    staged: LocalEvidenceTombstone,
) -> Result<(), LocalEvidenceError> {
    let mutex = local_mutex();
    let _guard = mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let _process_guard = acquire_local_evidence_process_lock(&staged.local_sessions_dir)?;
    restore_delete_unlocked(&staged)
}

pub(crate) fn finish_local_evidence_delete(
    staged: LocalEvidenceTombstone,
) -> Result<(), LocalEvidenceError> {
    let mutex = local_mutex();
    let _guard = mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let _process_guard = acquire_local_evidence_process_lock(&staged.local_sessions_dir)?;
    finish_delete_unlocked(&staged)
}

/// Hold the local-evidence mutex across stage → database delete →
/// restore/finish so a concurrent capture cannot recreate the original
/// directory in the small window between rename and session deletion.
pub(crate) fn delete_local_session_with_evidence(
    local_sessions_dir: &Path,
    session_id: &str,
) -> Result<(), LocalEvidenceError> {
    let mutex = local_mutex();
    let _guard = mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let _process_guard = acquire_local_evidence_process_lock(local_sessions_dir)?;
    let owner = LocalEvidenceOwner {
        session_id: session_id.into(),
    };
    reconcile_local_evidence_tombstones_unlocked(local_sessions_dir)?;
    ensure_owner_for_delete(local_sessions_dir, &owner)?;
    let staged = stage_delete_unlocked(local_sessions_dir, &owner)?;
    if let Err(error) = sessions::delete(local_sessions_dir, session_id) {
        if let Some(staged) = &staged {
            restore_delete_unlocked(staged)?;
        }
        return Err(LocalEvidenceError::Session(error));
    }
    if let Some(staged) = &staged {
        // The session is already gone. Cleanup failure leaves only an
        // inaccessible bounded tombstone; it must not turn a committed
        // delete into a misleading failure response.
        let _ = finish_delete_unlocked(staged);
    }
    Ok(())
}

/// Repair an interrupted two-phase delete. A tombstone whose session row is
/// still present was renamed before the DB commit and is restored. A tombstone
/// whose owner is gone was left after the commit and is purged.
pub(crate) fn reconcile_local_evidence_tombstones(
    local_sessions_dir: &Path,
) -> Result<(), LocalEvidenceError> {
    let mutex = local_mutex();
    let _guard = mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let _process_guard = acquire_local_evidence_process_lock(local_sessions_dir)?;
    reconcile_local_evidence_tombstones_unlocked(local_sessions_dir)
}

#[cfg(unix)]
pub(crate) fn acquire_local_evidence_process_lock(
    local_sessions_dir: &Path,
) -> Result<LocalEvidenceProcessLock, LocalEvidenceError> {
    use std::os::fd::AsRawFd;
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};

    let app_data = local_sessions_dir.parent().ok_or_else(|| {
        LocalEvidenceError::Storage("local sessions directory has no app-data parent".into())
    })?;
    let base = app_data.join(LOCAL_BROWSER_SESSIONS_DIR);
    refuse_symlink(&base, "local Browser evidence base")?;
    fs::create_dir_all(&base).map_err(|error| {
        LocalEvidenceError::Storage(format!("create local Browser evidence base: {error}"))
    })?;
    refuse_symlink(&base, "local Browser evidence base")?;
    let lock_path = base.join(".process.lock");
    let file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(&lock_path)
        .map_err(|error| {
            LocalEvidenceError::Refused(format!(
                "open local Browser evidence process lock: {error}"
            ))
        })?;
    let guard = LocalEvidenceProcessLock { file };
    let metadata = guard.file.metadata().map_err(|error| {
        LocalEvidenceError::Storage(format!(
            "inspect local Browser evidence process lock: {error}"
        ))
    })?;
    if !metadata.is_file() || metadata.nlink() != 1 {
        return Err(LocalEvidenceError::Refused(
            "local Browser evidence process lock is not a single-link regular file".into(),
        ));
    }
    // SAFETY: the descriptor is live and owned by `guard`; `flock` blocks
    // until any other Plume process releases the same inode.
    let result = unsafe { libc::flock(guard.file.as_raw_fd(), libc::LOCK_EX) };
    if result != 0 {
        return Err(LocalEvidenceError::Storage(format!(
            "lock local Browser evidence process state: {}",
            std::io::Error::last_os_error()
        )));
    }
    Ok(guard)
}

#[cfg(not(unix))]
pub(crate) fn acquire_local_evidence_process_lock(
    _local_sessions_dir: &Path,
) -> Result<LocalEvidenceProcessLock, LocalEvidenceError> {
    Err(LocalEvidenceError::Refused(
        "local Browser evidence requires cross-process file locking on this platform".into(),
    ))
}

fn reconcile_local_evidence_tombstones_unlocked(
    local_sessions_dir: &Path,
) -> Result<(), LocalEvidenceError> {
    let app_data = local_sessions_dir.parent().ok_or_else(|| {
        LocalEvidenceError::Storage("local sessions directory has no app-data parent".into())
    })?;
    let base = app_data.join(LOCAL_BROWSER_SESSIONS_DIR);
    refuse_symlink(&base, "local Browser evidence base")?;
    let entries = match fs::read_dir(&base) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(LocalEvidenceError::Storage(format!(
                "scan local Browser evidence tombstones: {error}"
            )))
        }
    };

    for entry in entries {
        let entry = entry.map_err(|error| {
            LocalEvidenceError::Storage(format!(
                "read local Browser evidence tombstone entry: {error}"
            ))
        })?;
        let Some(owner) = tombstone_owner(&entry.file_name().to_string_lossy()) else {
            continue;
        };
        let tombstone = entry.path();
        refuse_symlink(&tombstone, "local Browser evidence tombstone")?;
        if !entry
            .file_type()
            .map_err(|error| {
                LocalEvidenceError::Storage(format!(
                    "inspect local Browser evidence tombstone: {error}"
                ))
            })?
            .is_dir()
        {
            return Err(LocalEvidenceError::Refused(
                "local Browser evidence tombstone is not a directory".into(),
            ));
        }

        let original = base.join(&owner.session_id);
        refuse_symlink(&original, "local Browser session evidence root")?;
        if sessions::session_exists(local_sessions_dir, &owner.session_id)? {
            if original.exists() {
                return Err(LocalEvidenceError::Refused(
                    "local Browser evidence root and tombstone both exist".into(),
                ));
            }
            fs::rename(&tombstone, &original).map_err(|error| {
                LocalEvidenceError::Storage(format!(
                    "restore interrupted local Browser evidence delete: {error}"
                ))
            })?;
        } else {
            fs::remove_dir_all(&tombstone).map_err(|error| {
                LocalEvidenceError::Storage(format!(
                    "purge committed local Browser evidence tombstone: {error}"
                ))
            })?;
        }
    }
    Ok(())
}

fn tombstone_owner(name: &str) -> Option<LocalEvidenceOwner> {
    let suffix = name.strip_prefix(".deleted-")?;
    if suffix.len() != 50 || suffix.as_bytes().get(33) != Some(&b'-') {
        return None;
    }
    let owner = LocalEvidenceOwner {
        session_id: suffix[..33].into(),
    };
    if validate_owner_shape(&owner).is_err()
        || !suffix[34..].bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return None;
    }
    Some(owner)
}

fn ensure_owner_for_delete(
    local_sessions_dir: &Path,
    owner: &LocalEvidenceOwner,
) -> Result<(), LocalEvidenceError> {
    validate_owner_shape(owner)?;
    match sessions::session_exists(local_sessions_dir, &owner.session_id) {
        Ok(true) => Ok(()),
        Ok(false) => Err(LocalEvidenceError::OwnerNotFound),
        Err(SessionStoreError::Invalid(_)) => Err(LocalEvidenceError::InvalidOwner),
        Err(SessionStoreError::Refused(message)) => Err(LocalEvidenceError::Refused(message)),
        Err(other) => Err(LocalEvidenceError::Session(other)),
    }
}

fn ensure_owner(
    local_sessions_dir: &Path,
    owner: &LocalEvidenceOwner,
) -> Result<(), LocalEvidenceError> {
    validate_owner_shape(owner)?;
    match sessions::load_for_scope(local_sessions_dir, &owner.session_id, false) {
        Ok(_) => Ok(()),
        Err(SessionStoreError::NotFound(_)) => Err(LocalEvidenceError::OwnerNotFound),
        Err(SessionStoreError::Invalid(_)) => Err(LocalEvidenceError::InvalidOwner),
        Err(SessionStoreError::Refused(message)) => Err(LocalEvidenceError::Refused(message)),
        Err(other) => Err(LocalEvidenceError::Session(other)),
    }
}

fn validate_owner_shape(owner: &LocalEvidenceOwner) -> Result<(), LocalEvidenceError> {
    if owner.session_id.len() == 33
        && owner.session_id.starts_with('s')
        && owner.session_id[1..]
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    {
        Ok(())
    } else {
        Err(LocalEvidenceError::InvalidOwner)
    }
}

fn ensure_session_root(
    local_sessions_dir: &Path,
    owner: &LocalEvidenceOwner,
) -> Result<PathBuf, LocalEvidenceError> {
    let root = session_evidence_root(local_sessions_dir, owner)?;
    let base = root
        .parent()
        .expect("session evidence root always has base");
    refuse_symlink(base, "local Browser evidence base")?;
    fs::create_dir_all(base).map_err(|error| {
        LocalEvidenceError::Storage(format!("create local Browser evidence base: {error}"))
    })?;
    refuse_symlink(&root, "local Browser session evidence root")?;
    fs::create_dir(&root)
        .or_else(|error| {
            if error.kind() == std::io::ErrorKind::AlreadyExists {
                Ok(())
            } else {
                Err(error)
            }
        })
        .map_err(|error| {
            LocalEvidenceError::Storage(format!("create local Browser session evidence: {error}"))
        })?;
    refuse_symlink(&root, "local Browser session evidence root")?;
    Ok(root)
}

fn stage_delete_unlocked(
    local_sessions_dir: &Path,
    owner: &LocalEvidenceOwner,
) -> Result<Option<LocalEvidenceTombstone>, LocalEvidenceError> {
    let original = session_evidence_root(local_sessions_dir, owner)?;
    refuse_symlink(&original, "local Browser session evidence root")?;
    if !original.exists() {
        return Ok(None);
    }
    let base = original.parent().expect("session evidence root has base");
    refuse_symlink(base, "local Browser evidence base")?;
    let nonce = TOMBSTONE_NONCE.fetch_add(1, Ordering::Relaxed);
    let tombstone = base.join(format!(".deleted-{}-{nonce:016x}", owner.session_id));
    refuse_symlink(&tombstone, "local Browser evidence tombstone")?;
    fs::rename(&original, &tombstone).map_err(|error| {
        LocalEvidenceError::Storage(format!("stage local Browser evidence delete: {error}"))
    })?;
    Ok(Some(LocalEvidenceTombstone {
        local_sessions_dir: local_sessions_dir.to_path_buf(),
        original,
        tombstone,
    }))
}

fn restore_delete_unlocked(staged: &LocalEvidenceTombstone) -> Result<(), LocalEvidenceError> {
    refuse_symlink(&staged.tombstone, "local Browser evidence tombstone")?;
    refuse_symlink(&staged.original, "local Browser session evidence root")?;
    if staged.original.exists() {
        return Err(LocalEvidenceError::Refused(
            "local Browser evidence root reappeared during delete rollback".into(),
        ));
    }
    fs::rename(&staged.tombstone, &staged.original).map_err(|error| {
        LocalEvidenceError::Storage(format!("restore local Browser evidence delete: {error}"))
    })
}

fn finish_delete_unlocked(staged: &LocalEvidenceTombstone) -> Result<(), LocalEvidenceError> {
    refuse_symlink(&staged.tombstone, "local Browser evidence tombstone")?;
    match fs::remove_dir_all(&staged.tombstone) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(LocalEvidenceError::Storage(format!(
            "finish local Browser evidence delete: {error}"
        ))),
    }
}

fn refuse_symlink(path: &Path, label: &str) -> Result<(), LocalEvidenceError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(LocalEvidenceError::Refused(
            format!("{label} is a symlink; refusing local Browser evidence access"),
        )),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(LocalEvidenceError::Storage(format!(
            "inspect {label}: {error}"
        ))),
    }
}

fn map_text_error(error: BrowserEvidenceError) -> LocalEvidenceError {
    if error.is_capacity() {
        LocalEvidenceError::Capacity
    } else if error.0.contains("symlink") || error.0.contains("hardlink") {
        LocalEvidenceError::Refused(error.0)
    } else {
        LocalEvidenceError::Storage(error.0)
    }
}

fn map_screenshot_error(error: BrowserScreenshotError) -> LocalEvidenceError {
    if error.is_capacity() {
        LocalEvidenceError::Capacity
    } else if error.0.contains("symlink") || error.0.contains("hardlink") {
        LocalEvidenceError::Refused(error.0)
    } else {
        LocalEvidenceError::Storage(error.0)
    }
}

fn local_mutex() -> &'static Mutex<()> {
    LOCAL_STORE_MUTEX.get_or_init(|| Mutex::new(()))
}
