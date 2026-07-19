use std::fs;

use super::owner::{resolve_session_owner, SessionOwnerError, SessionOwnerRef, SessionOwnerScope};
use super::{create, project_sessions_dir};
use crate::project::OpenProject;

#[test]
fn local_and_project_owners_resolve_only_in_their_own_store() {
    let temp = tempfile::tempdir().expect("tempdir");
    let local_dir = temp.path().join("local-sessions");
    let project_root = temp.path().join("project");
    fs::create_dir_all(&project_root).expect("project root");
    let project_root = fs::canonicalize(project_root).expect("canonical project");
    let project = OpenProject {
        id: "project-generation-1".into(),
        root: project_root.clone(),
    };
    let local = create(&local_dir, Some("local")).expect("local session");
    let project_session = create(
        &project_sessions_dir(&project_root).expect("project sessions dir"),
        Some("project"),
    )
    .expect("project session");

    let resolved_local = resolve_session_owner(
        &SessionOwnerRef {
            scope: SessionOwnerScope::Local,
            session_id: local.id.clone(),
        },
        SessionOwnerScope::Local,
        &local_dir,
        Some(&project),
    )
    .expect("local owner");
    assert_eq!(resolved_local.sessions_dir, local_dir);
    assert!(resolved_local.project.is_none());

    let resolved_project = resolve_session_owner(
        &SessionOwnerRef {
            scope: SessionOwnerScope::Project,
            session_id: project_session.id,
        },
        SessionOwnerScope::Project,
        &resolved_local.sessions_dir,
        Some(&project),
    )
    .expect("project owner");
    let resolved_generation = resolved_project
        .project
        .as_ref()
        .expect("project generation");
    assert_eq!(resolved_generation.id, project.id);
    assert_eq!(resolved_generation.root, project.root);

    assert!(matches!(
        resolve_session_owner(
            &SessionOwnerRef {
                scope: SessionOwnerScope::Local,
                session_id: local.id,
            },
            SessionOwnerScope::Project,
            &resolved_local.sessions_dir,
            resolved_project.project.as_ref(),
        ),
        Err(SessionOwnerError::ScopeMismatch)
    ));
}

#[test]
fn project_owner_requires_a_backend_resolved_trusted_project() {
    let temp = tempfile::tempdir().expect("tempdir");
    let result = resolve_session_owner(
        &SessionOwnerRef {
            scope: SessionOwnerScope::Project,
            session_id: "s0123456789abcdef0123456789abcdef".into(),
        },
        SessionOwnerScope::Project,
        &temp.path().join("local"),
        None,
    );
    assert!(matches!(result, Err(SessionOwnerError::ProjectUnavailable)));
}

#[test]
fn missing_owner_is_not_confused_with_the_other_scope() {
    let temp = tempfile::tempdir().expect("tempdir");
    let local_dir = temp.path().join("local");
    create(&local_dir, Some("different session")).expect("initialize local store");
    let result = resolve_session_owner(
        &SessionOwnerRef {
            scope: SessionOwnerScope::Local,
            session_id: "s0123456789abcdef0123456789abcdef".into(),
        },
        SessionOwnerScope::Local,
        &local_dir,
        None,
    );
    assert!(matches!(result, Err(SessionOwnerError::NotFound)));
}
