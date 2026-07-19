//! Wire-neutral resolution of one persisted chat-session owner.

use std::path::{Path, PathBuf};

use crate::project::OpenProject;

use super::{project_sessions_dir, session_exists, SessionStoreError};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SessionOwnerScope {
    Local,
    Project,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SessionOwnerRef {
    pub scope: SessionOwnerScope,
    pub session_id: String,
}

#[derive(Debug, Clone)]
pub(crate) struct ResolvedSessionOwner {
    pub scope: SessionOwnerScope,
    pub session_id: String,
    pub sessions_dir: PathBuf,
    /// The exact backend-resolved project generation. `None` for local chats.
    pub project: Option<OpenProject>,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum SessionOwnerError {
    #[error("session owner scope does not match this surface")]
    ScopeMismatch,
    #[error("no trusted project generation is available")]
    ProjectUnavailable,
    #[error("session owner was not found")]
    NotFound,
    #[error(transparent)]
    Store(#[from] SessionStoreError),
}

pub(crate) fn resolve_session_owner(
    owner: &SessionOwnerRef,
    expected_scope: SessionOwnerScope,
    local_sessions_dir: &Path,
    trusted_project: Option<&OpenProject>,
) -> Result<ResolvedSessionOwner, SessionOwnerError> {
    if owner.scope != expected_scope {
        return Err(SessionOwnerError::ScopeMismatch);
    }
    let (sessions_dir, project) = match owner.scope {
        SessionOwnerScope::Local => (local_sessions_dir.to_path_buf(), None),
        SessionOwnerScope::Project => {
            let project = trusted_project.ok_or(SessionOwnerError::ProjectUnavailable)?;
            (project_sessions_dir(&project.root)?, Some(project.clone()))
        }
    };
    if !session_exists(&sessions_dir, &owner.session_id)? {
        return Err(SessionOwnerError::NotFound);
    }
    Ok(ResolvedSessionOwner {
        scope: owner.scope,
        session_id: owner.session_id.clone(),
        sessions_dir,
        project,
    })
}
