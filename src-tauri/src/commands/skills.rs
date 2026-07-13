//! Trusted-project IPC adapter for the inert manual skill library.

use serde::Deserialize;
use tauri::State;

use crate::commands::project::AppState;
use crate::error::{IpcError, IpcRequest};
use crate::project::OpenProject;
use crate::skills::{
    self, SkillApplyResponse, SkillDocument, SkillIndex, SkillInput, SkillPreview, SkillsError,
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
            session: ProjectSession::default(),
            trust: Mutex::new(TrustStore::load(base.join("trust.json"))),
            chat_streams: Arc::new(ChatStreamRegistry::default()),
            agent_config: Mutex::new(crate::agent::AgentConfig::default()),
            local_sessions_dir: base.join("sessions"),
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
