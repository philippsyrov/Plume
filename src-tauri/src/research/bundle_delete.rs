//! Two-phase cleanup of session-owned research artifact directories.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::browser::local_evidence::{delete_local_session_with_evidence, LocalEvidenceError};
use crate::project::OpenProject;
use crate::sessions::owner::{
    resolve_session_owner, SessionOwnerError, SessionOwnerRef, SessionOwnerScope,
};
use crate::sessions::{self, SessionStoreError};

use super::{refuse_directory, refuse_path_surprises, ArtifactStore, ArtifactStoreError};

static TOMBSTONE_NONCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, thiserror::Error)]
pub(crate) enum ArtifactDeleteError {
    #[error(transparent)]
    Artifact(#[from] ArtifactStoreError),
    #[error(transparent)]
    LocalEvidence(#[from] LocalEvidenceError),
    #[error(transparent)]
    Session(#[from] SessionStoreError),
    #[error(transparent)]
    Owner(#[from] SessionOwnerError),
}

pub(crate) fn delete_local_session_with_artifacts(
    local_sessions_dir: &Path,
    session_id: &str,
) -> Result<(), ArtifactDeleteError> {
    let owner = resolve_session_owner(
        &SessionOwnerRef {
            scope: SessionOwnerScope::Local,
            session_id: session_id.to_string(),
        },
        SessionOwnerScope::Local,
        local_sessions_dir,
        None,
    )?;
    let store = ArtifactStore::from_owner(&owner)?;
    delete_with_store(&store, || {
        delete_local_session_with_evidence(local_sessions_dir, session_id)
            .map_err(ArtifactDeleteError::LocalEvidence)
    })
}

pub(crate) fn delete_project_session_with_artifacts(
    project: &OpenProject,
    project_sessions_dir: &Path,
    session_id: &str,
) -> Result<(), ArtifactDeleteError> {
    let owner = resolve_session_owner(
        &SessionOwnerRef {
            scope: SessionOwnerScope::Project,
            session_id: session_id.to_string(),
        },
        SessionOwnerScope::Project,
        project_sessions_dir,
        Some(project),
    )?;
    let store = ArtifactStore::from_owner(&owner)?;
    delete_with_store(&store, || {
        sessions::delete(project_sessions_dir, session_id).map_err(ArtifactDeleteError::Session)
    })
}

fn delete_with_store(
    store: &ArtifactStore,
    delete_session: impl FnOnce() -> Result<(), ArtifactDeleteError>,
) -> Result<(), ArtifactDeleteError> {
    store.with_lock(|store| {
        let staged = stage_delete(store)?;
        if let Err(error) = delete_session() {
            restore_delete(&staged)?;
            return Err(ArtifactStoreError::Storage(format!(
                "session delete failed after artifact staging: {error}"
            )));
        }
        // The session is already committed gone. Cleanup failure leaves only
        // an inaccessible tombstone for the next reconciliation pass.
        let _ = finish_delete(&staged);
        Ok(())
    })?;
    Ok(())
}

struct ArtifactTombstone {
    original: PathBuf,
    tombstone: PathBuf,
}

fn stage_delete(store: &ArtifactStore) -> Result<ArtifactTombstone, ArtifactStoreError> {
    refuse_directory(&store.session_root)?;
    let base = store.base_root()?;
    let tombstone = base.join(format!(
        ".deleted-{}-{:016x}",
        store.session_id,
        TOMBSTONE_NONCE.fetch_add(1, Ordering::Relaxed)
    ));
    refuse_path_surprises(&tombstone)?;
    fs::rename(&store.session_root, &tombstone)
        .map_err(|error| ArtifactStoreError::Storage(format!("stage artifact delete: {error}")))?;
    Ok(ArtifactTombstone {
        original: store.session_root.clone(),
        tombstone,
    })
}

fn restore_delete(staged: &ArtifactTombstone) -> Result<(), ArtifactStoreError> {
    refuse_directory(&staged.tombstone)?;
    refuse_path_surprises(&staged.original)?;
    fs::rename(&staged.tombstone, &staged.original).map_err(|error| {
        ArtifactStoreError::Storage(format!("restore staged artifact delete: {error}"))
    })
}

fn finish_delete(staged: &ArtifactTombstone) -> Result<(), ArtifactStoreError> {
    refuse_directory(&staged.tombstone)?;
    fs::remove_dir_all(&staged.tombstone).map_err(|error| {
        ArtifactStoreError::Storage(format!("purge staged artifact delete: {error}"))
    })
}

pub(super) fn reconcile_tombstones(
    base: &Path,
    sessions_dir: &Path,
) -> Result<(), ArtifactStoreError> {
    for entry in fs::read_dir(base)
        .map_err(|error| ArtifactStoreError::Storage(format!("scan tombstones: {error}")))?
    {
        let entry = entry
            .map_err(|error| ArtifactStoreError::Storage(format!("read tombstone: {error}")))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some(session_id) = tombstone_session_id(&name) else {
            continue;
        };
        let tombstone = entry.path();
        refuse_directory(&tombstone)?;
        let original = base.join(&session_id);
        refuse_path_surprises(&original)?;
        let owner_exists = sessions::session_exists(sessions_dir, &session_id)
            .map_err(|error| ArtifactStoreError::Storage(error.to_string()))?;
        if owner_exists {
            if original.exists() {
                return Err(ArtifactStoreError::Refused(
                    "artifact root and tombstone both exist".into(),
                ));
            }
            fs::rename(&tombstone, &original).map_err(|error| {
                ArtifactStoreError::Storage(format!("restore interrupted tombstone: {error}"))
            })?;
        } else {
            fs::remove_dir_all(&tombstone).map_err(|error| {
                ArtifactStoreError::Storage(format!("purge committed tombstone: {error}"))
            })?;
        }
    }
    Ok(())
}

fn tombstone_session_id(name: &str) -> Option<String> {
    let suffix = name.strip_prefix(".deleted-")?;
    let (session_id, nonce) = suffix.rsplit_once('-')?;
    if session_id.is_empty()
        || session_id.len() > 64
        || !session_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        || nonce.len() != 16
        || !nonce.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return None;
    }
    Some(session_id.to_string())
}

#[cfg(test)]
pub(crate) fn fail_delete_after_staging_for_test(
    store: &ArtifactStore,
) -> Result<(), ArtifactDeleteError> {
    delete_with_store(store, || {
        Err(ArtifactDeleteError::Session(SessionStoreError::Storage(
            "injected failure".into(),
        )))
    })
}

#[cfg(test)]
pub(crate) fn stage_interrupted_delete_for_test(
    store: &ArtifactStore,
) -> Result<PathBuf, ArtifactStoreError> {
    store.with_lock(|store| stage_delete(store).map(|staged| staged.tombstone))
}
