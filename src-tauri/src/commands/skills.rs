//! Trusted-project IPC adapter for the inert manual skill library.

use serde::Deserialize;
use tauri::State;

use crate::commands::project::AppState;
use crate::error::{IpcError, IpcRequest};
use crate::project::OpenProject;
use crate::skills::{
    self, SkillApplyResponse, SkillDocument, SkillIndex, SkillInput, SkillPreview,
    SkillPromotionContext, SkillPromotionError, SkillPromotionPreview, SkillsError,
};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EmptyPayload {}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SkillSlugPayload {
    pub slug: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SkillWritePayload {
    pub slug: String,
    pub name: String,
    pub description: String,
    pub body: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SkillPromotePreviewPayload {
    pub session_id: String,
    pub entry_indexes: Vec<u32>,
    pub snapshot_token: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SkillPromotionContextPayload {
    pub session_id: String,
}

impl From<SkillWritePayload> for SkillInput {
    fn from(value: SkillWritePayload) -> Self {
        Self {
            slug: value.slug,
            name: value.name,
            description: value.description,
            body: value.body,
        }
    }
}

#[tauri::command]
pub async fn skills_list(
    req: IpcRequest<EmptyPayload>,
    state: State<'_, AppState>,
) -> Result<SkillIndex, IpcError> {
    req.check_version()?;
    let project = trusted_open(&state).ok_or(IpcError::NeedsApproval)?;
    skills::list(&project.root).map_err(|e| map_skill_error(e, true))
}

#[tauri::command]
pub async fn skills_load(
    req: IpcRequest<SkillSlugPayload>,
    state: State<'_, AppState>,
) -> Result<SkillDocument, IpcError> {
    req.check_version()?;
    let project = trusted_open(&state).ok_or(IpcError::NeedsApproval)?;
    skills::load(&project.root, &req.payload.slug).map_err(|e| map_skill_error(e, false))
}

#[tauri::command]
pub async fn skills_preview(
    req: IpcRequest<SkillWritePayload>,
    state: State<'_, AppState>,
) -> Result<SkillPreview, IpcError> {
    req.check_version()?;
    let project = trusted_open(&state).ok_or(IpcError::NeedsApproval)?;
    skills::preview(&project.root, &req.payload.into()).map_err(|e| map_skill_error(e, false))
}

#[tauri::command]
pub async fn skills_apply(
    req: IpcRequest<SkillWritePayload>,
    state: State<'_, AppState>,
) -> Result<SkillApplyResponse, IpcError> {
    req.check_version()?;
    let project = trusted_open(&state).ok_or(IpcError::NeedsApproval)?;
    let input = SkillInput::from(req.payload);
    skills::preview(&project.root, &input).map_err(|e| map_skill_error(e, false))?;
    skills::apply(&project.root, &input).map_err(|e| map_skill_error(e, true))
}

#[tauri::command]
pub async fn skills_promote_preview(
    req: IpcRequest<SkillPromotePreviewPayload>,
    state: State<'_, AppState>,
) -> Result<SkillPromotionPreview, IpcError> {
    req.check_version()?;
    skills_promote_preview_impl(&req.payload, &state)
}

// Sync core so scope enforcement (project-only sessions store) is unit
// testable without a Tauri `State`. Mirrors `sessions::*_impl`.
fn skills_promote_preview_impl(
    payload: &SkillPromotePreviewPayload,
    state: &AppState,
) -> Result<SkillPromotionPreview, IpcError> {
    let project = trusted_open(state).ok_or(IpcError::NeedsApproval)?;
    let sessions_dir = promotion_sessions_dir(&project).map_err(map_session_error)?;
    let session = crate::sessions::load_for_scope(&sessions_dir, &payload.session_id, true)
        .map_err(map_session_error)?;
    skills::promote_preview(&session, &payload.entry_indexes, &payload.snapshot_token)
        .map_err(map_promotion_error)
}

#[tauri::command]
pub async fn skills_promotion_context(
    req: IpcRequest<SkillPromotionContextPayload>,
    state: State<'_, AppState>,
) -> Result<SkillPromotionContext, IpcError> {
    req.check_version()?;
    skills_promotion_context_impl(&req.payload, &state)
}

fn skills_promotion_context_impl(
    payload: &SkillPromotionContextPayload,
    state: &AppState,
) -> Result<SkillPromotionContext, IpcError> {
    let project = trusted_open(state).ok_or(IpcError::NeedsApproval)?;
    let sessions_dir = promotion_sessions_dir(&project).map_err(map_session_error)?;
    let session = crate::sessions::load_for_scope(&sessions_dir, &payload.session_id, true)
        .map_err(map_session_error)?;
    skills::promotion_context(&session).map_err(map_promotion_error)
}

fn promotion_sessions_dir(
    project: &OpenProject,
) -> Result<std::path::PathBuf, crate::sessions::SessionStoreError> {
    crate::sessions::project_sessions_dir(&project.root)
}

fn map_promotion_error(error: SkillPromotionError) -> IpcError {
    match error {
        SkillPromotionError::Session(error) => map_session_error(error),
        SkillPromotionError::Skill(error) => map_skill_error(error, false),
        SkillPromotionError::SnapshotMismatch => {
            IpcError::BadArgument("session changed; reload promotion context".into())
        }
    }
}

fn map_session_error(error: crate::sessions::SessionStoreError) -> IpcError {
    use crate::sessions::SessionStoreError;
    match error {
        SessionStoreError::NotFound(id) => IpcError::NotFound(format!("session {id}")),
        SessionStoreError::Invalid(message) => IpcError::BadArgument(message),
        SessionStoreError::Limit(message) | SessionStoreError::Refused(message) => {
            IpcError::Blocked(message)
        }
        SessionStoreError::StorageFull {
            used_bytes,
            cap_bytes,
        } => IpcError::StorageFull {
            used_bytes,
            cap_bytes,
        },
        SessionStoreError::Corrupt(message) | SessionStoreError::Storage(message) => {
            IpcError::Internal(message)
        }
    }
}

fn map_skill_error(error: SkillsError, storage_default: bool) -> IpcError {
    if error.0.contains("symlink") || error.0.contains("hardlink") {
        IpcError::Blocked(error.0)
    } else if storage_default {
        IpcError::Internal(error.0)
    } else {
        IpcError::BadArgument(error.0)
    }
}

fn trusted_open(state: &AppState) -> Option<OpenProject> {
    let open = state.session.current()?;
    let trusted = state
        .trust
        .lock()
        .expect("trust mutex poisoned")
        .is_trusted(&open.root);
    trusted.then_some(open)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::{Arc, Mutex};

    use crate::chat::stream::ChatStreamRegistry;
    use crate::project::trust::TrustStore;
    use crate::project::ProjectSession;

    fn state(base: &std::path::Path) -> AppState {
        AppState {
            session: Arc::new(ProjectSession::default()),
            trust: Mutex::new(TrustStore::load(base.join("trust.json"))),
            chat_streams: Arc::new(ChatStreamRegistry::default()),
            research_runs: Arc::new(crate::research::run_registry::ResearchRunRegistry::default()),
            agent_config: Mutex::new(crate::agent::AgentConfig::default()),
            local_sessions_dir: base.join("sessions"),
            user_memory_dir: base.join("memory"),
            catalog_store: Arc::new(crate::providers::catalog::CatalogStore::new(
                base.to_path_buf(),
            )),
            catalog_downloads: Arc::new(
                crate::providers::catalog_download::CatalogDownloadRegistry::default(),
            ),
        }
    }

    #[test]
    fn write_payload_uses_camel_case_and_rejects_extra_scope_fields() {
        let good =
            serde_json::json!({"slug":"test","name":"Test","description":"A test","body":"# Body"});
        assert!(serde_json::from_value::<SkillWritePayload>(good).is_ok());
        for extra in [
            serde_json::json!({"slug":"test","name":"Test","description":"A test","body":"# Body","root":"/tmp"}),
            serde_json::json!({"slug":"test","name":"Test","description":"A test","body":"# Body","scope":"local"}),
        ] {
            assert!(serde_json::from_value::<SkillWritePayload>(extra).is_err());
        }
    }

    #[test]
    fn promotion_payload_is_strict_and_project_scope_is_not_client_selectable() {
        let good = serde_json::json!({"sessionId":"s_0123456789abcdef0123456789abcdef","entryIndexes":[0,2],"snapshotToken":"sha256:abc"});
        assert!(serde_json::from_value::<SkillPromotePreviewPayload>(good).is_ok());
        for extra in ["root", "scope", "title", "body", "draft"] {
            let mut value = serde_json::json!({"sessionId":"s_0123456789abcdef0123456789abcdef","entryIndexes":[0],"snapshotToken":"sha256:abc"});
            value
                .as_object_mut()
                .unwrap()
                .insert(extra.into(), serde_json::json!("bad"));
            assert!(serde_json::from_value::<SkillPromotePreviewPayload>(value).is_err());
        }
    }

    #[test]
    fn promotion_context_payload_rejects_scope_root_and_selection() {
        let good = serde_json::json!({"sessionId":"s_0123456789abcdef0123456789abcdef"});
        assert!(serde_json::from_value::<SkillPromotionContextPayload>(good).is_ok());
        for extra in ["root", "scope", "entryIndexes", "snapshotToken"] {
            let mut value = serde_json::json!({"sessionId":"s_0123456789abcdef0123456789abcdef"});
            value
                .as_object_mut()
                .unwrap()
                .insert(extra.into(), serde_json::json!("bad"));
            assert!(serde_json::from_value::<SkillPromotionContextPayload>(value).is_err());
        }
    }

    #[test]
    fn promotion_directory_is_always_the_trusted_projects_store() {
        let base = std::env::temp_dir().join(format!(
            "plume-skill-promotion-scope-{}",
            crate::project::mint_id()
        ));
        let root = base.join("project");
        fs::create_dir_all(&root).unwrap();
        let root = fs::canonicalize(root).unwrap();
        let open = OpenProject {
            id: "project-test".into(),
            root: root.clone(),
        };
        assert_eq!(
            promotion_sessions_dir(&open).unwrap(),
            root.join(".plume/sessions")
        );
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn promotion_verbs_reject_a_local_session_id_with_not_found() {
        // Scope enforcement at the command boundary: promotion is a
        // PROJECT-only verb (it always resolves `project_sessions_dir`),
        // so a session id that lives only in the local (app-level) store
        // must miss with NotFound through BOTH promotion verbs — a local
        // chat can never be promoted into a project skill.
        let base = std::env::temp_dir().join(format!(
            "plume-skill-promotion-local-{}",
            crate::project::mint_id()
        ));
        let root = base.join("project");
        fs::create_dir_all(&root).unwrap();
        let root = fs::canonicalize(root).unwrap();
        let state = state(&base);
        state.session.open(root.clone());
        state.trust.lock().unwrap().mark_trusted(&root).unwrap();

        // A session that exists ONLY in the local store…
        let local = crate::sessions::create(&state.local_sessions_dir, Some("local only")).unwrap();
        // …and a real project session, so the project store exists and
        // is non-empty; the local id must still miss it (isolation, not
        // an absent store).
        let project_dir = crate::sessions::project_sessions_dir(&root).unwrap();
        let project_session = crate::sessions::create(&project_dir, Some("project")).unwrap();

        let ctx = skills_promotion_context_impl(
            &SkillPromotionContextPayload {
                session_id: local.id.clone(),
            },
            &state,
        );
        assert!(
            matches!(ctx, Err(IpcError::NotFound(_))),
            "context: {ctx:?}"
        );

        let preview = skills_promote_preview_impl(
            &SkillPromotePreviewPayload {
                session_id: local.id.clone(),
                entry_indexes: vec![0],
                snapshot_token: "sha256:unused".into(),
            },
            &state,
        );
        assert!(
            matches!(preview, Err(IpcError::NotFound(_))),
            "preview: {preview:?}"
        );

        // Control: the same verb resolves a genuine project session, so
        // the NotFound above is scope isolation, not a broken store.
        let ctx_ok = skills_promotion_context_impl(
            &SkillPromotionContextPayload {
                session_id: project_session.id.clone(),
            },
            &state,
        );
        assert!(ctx_ok.is_ok(), "project session should resolve: {ctx_ok:?}");

        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn slug_payload_rejects_unknown_fields() {
        assert!(serde_json::from_value::<SkillSlugPayload>(
            serde_json::json!({"slug":"test","root":"/tmp"})
        )
        .is_err());
    }

    #[test]
    fn trusted_open_requires_both_open_project_and_trust() {
        let base = std::env::temp_dir().join(format!(
            "plume-skill-command-gate-{}",
            crate::project::mint_id()
        ));
        let project = base.join("project");
        fs::create_dir_all(&project).unwrap();
        let project = fs::canonicalize(project).unwrap();
        let state = state(&base);
        assert!(trusted_open(&state).is_none());
        state.session.open(project.clone());
        assert!(trusted_open(&state).is_none());
        state.trust.lock().unwrap().mark_trusted(&project).unwrap();
        assert_eq!(trusted_open(&state).unwrap().root, project);
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn responses_use_the_documented_camel_case_wire_shape() {
        let preview = SkillPreview {
            slug: "demo".into(),
            content: "body".into(),
            exists: false,
        };
        assert_eq!(
            serde_json::to_value(preview).unwrap(),
            serde_json::json!({"slug":"demo","content":"body","exists":false})
        );
        let failure = SkillApplyResponse {
            ok: false,
            skill: None,
            reason: Some("alreadyExists".into()),
            message: Some("exists".into()),
        };
        assert_eq!(
            serde_json::to_value(failure).unwrap(),
            serde_json::json!({"ok":false,"reason":"alreadyExists","message":"exists"})
        );
    }

    #[test]
    fn link_alias_errors_map_to_blocked_not_bad_argument() {
        assert!(matches!(
            map_skill_error(
                crate::skills::SkillsError("SKILL.md is hardlinked".into()),
                false
            ),
            IpcError::Blocked(_)
        ));
        assert!(matches!(
            map_skill_error(
                crate::skills::SkillsError("invalid skill slug".into()),
                false
            ),
            IpcError::BadArgument(_)
        ));
        assert!(matches!(
            map_skill_error(crate::skills::SkillsError("read failed".into()), true),
            IpcError::Internal(_)
        ));
    }
}
