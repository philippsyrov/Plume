//! D37 + D43 memory command handlers.
//!
//! Four verbs:
//!
//!   * `memory.index` — read the current entry list. Cheap (one
//!     synchronous file read); doesn't need a blocking pool.
//!   * `memory.remember` — append a redacted text entry.
//!   * `memory.forget` — remove an entry by id. Idempotent.
//!   * `memory.search` — D43, capped substring search over the
//!     redacted entries. Read-only.
//!
//! Project-memory verbs are gated on a trusted open project (same pattern as
//! the patch verbs). The `memory_user_*` family is app-private and deliberately
//! project-independent: it reads the backend-owned app-data path from
//! `AppState`, never from a caller payload. The read/write functions live in
//! `crate::memory`; this file is a thin adapter over those ownership rules.
//!
//! Concurrency: `memory.remember` and `memory.forget` do real I/O
//! (atomic rename), but the work is small and synchronous. We do
//! NOT take the `patch::apply_mutex` — memory writes are scoped to
//! `.plume/memory/` and never overlap with the patch checkpoint
//! tree, so an in-flight `patch.apply` and a `memory.remember` can
//! safely interleave. Two concurrent memory writes on the same
//! project would race on the JSONL file; in practice the panel is
//! synchronous so this is theoretical, but a follow-up slice can
//! add a memory-local mutex if needed.

use serde::Deserialize;
use tauri::State;

use crate::commands::project::AppState;
use crate::error::{IpcError, IpcRequest};
use crate::memory::{
    distill_apply as memory_distill_apply_impl, distill_preview as memory_distill_preview_impl,
    forget as memory_forget_impl, forget_user_memory, read_distill_log as memory_distill_log_impl,
    read_index, read_topics as memory_topics_impl, read_user_memory_index,
    remember as memory_remember_impl, remember_user_memory, search as memory_search_impl,
    search_user_memory, set_links as memory_set_links_impl, update as memory_update_impl,
    update_user_memory, DistillLogEntry, DistillPreview, MemoryDistillApplyResponse,
    MemoryForgetResponse, MemoryIndex, MemoryRememberResponse, MemorySearchResponse,
    MemorySetLinksResponse, MemoryTopics, MemoryUpdateResponse, UserMemoryForgetResponse,
    UserMemoryIndex, UserMemoryRememberResponse, UserMemorySearchResponse,
    UserMemoryUpdateResponse,
};
use crate::project::OpenProject;

/// Empty payload for app-private user-memory reads. A dedicated type keeps
/// unknown caller-supplied path or scope fields fail-closed.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UserMemoryEmptyPayload {}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UserMemoryRememberPayload {
    pub text: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UserMemoryUpdatePayload {
    pub entry_id: String,
    pub text: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UserMemoryForgetPayload {
    pub entry_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UserMemorySearchPayload {
    pub query: String,
    pub limit: u32,
}

#[tauri::command]
pub async fn memory_user_index(
    req: IpcRequest<UserMemoryEmptyPayload>,
    state: State<'_, AppState>,
) -> Result<UserMemoryIndex, IpcError> {
    req.check_version()?;
    user_memory_index_for_state(&state)
}

fn user_memory_index_for_state(state: &AppState) -> Result<UserMemoryIndex, IpcError> {
    read_user_memory_index(&state.user_memory_dir).map_err(|error| IpcError::Internal(error.0))
}

#[tauri::command]
pub async fn memory_user_remember(
    req: IpcRequest<UserMemoryRememberPayload>,
    state: State<'_, AppState>,
) -> Result<UserMemoryRememberResponse, IpcError> {
    req.check_version()?;
    Ok(user_memory_remember_for_state(&state, &req.payload.text))
}

fn user_memory_remember_for_state(state: &AppState, text: &str) -> UserMemoryRememberResponse {
    remember_user_memory(&state.user_memory_dir, text)
}

#[tauri::command]
pub async fn memory_user_update(
    req: IpcRequest<UserMemoryUpdatePayload>,
    state: State<'_, AppState>,
) -> Result<UserMemoryUpdateResponse, IpcError> {
    req.check_version()?;
    Ok(update_user_memory(
        &state.user_memory_dir,
        &req.payload.entry_id,
        &req.payload.text,
    ))
}

#[tauri::command]
pub async fn memory_user_forget(
    req: IpcRequest<UserMemoryForgetPayload>,
    state: State<'_, AppState>,
) -> Result<UserMemoryForgetResponse, IpcError> {
    req.check_version()?;
    Ok(forget_user_memory(
        &state.user_memory_dir,
        &req.payload.entry_id,
    ))
}

#[tauri::command]
pub async fn memory_user_search(
    req: IpcRequest<UserMemorySearchPayload>,
    state: State<'_, AppState>,
) -> Result<UserMemorySearchResponse, IpcError> {
    req.check_version()?;
    Ok(search_user_memory(
        &state.user_memory_dir,
        &req.payload.query,
        req.payload.limit,
    ))
}

#[tauri::command]
pub async fn memory_index(
    req: IpcRequest<EmptyPayload>,
    state: State<'_, AppState>,
) -> Result<MemoryIndex, IpcError> {
    req.check_version()?;
    let project = match trusted_open(&state) {
        Some(p) => p,
        None => return Err(IpcError::NeedsApproval),
    };
    read_index(project.root.as_path()).map_err(|e| IpcError::Internal(e.0))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MemoryRememberPayload {
    /// Raw text the user wants to remember. The backend trims it,
    /// length-caps it, and passes it through the secret redactor
    /// before writing.
    pub text: String,
}

#[tauri::command]
pub async fn memory_remember(
    req: IpcRequest<MemoryRememberPayload>,
    state: State<'_, AppState>,
) -> Result<MemoryRememberResponse, IpcError> {
    req.check_version()?;
    let payload = req.payload;
    let project = match trusted_open(&state) {
        Some(p) => p,
        None => return Err(IpcError::NeedsApproval),
    };
    Ok(memory_remember_impl(project.root.as_path(), &payload.text))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MemoryUpdatePayload {
    /// Opaque id of the entry to edit (from a prior `memory.remember`).
    pub entry_id: String,
    /// Replacement text. Re-redacted and re-capped server-side, exactly
    /// like `memory.remember`; the entry's id and createdMs are kept.
    pub text: String,
}

/// D80: edit an existing memory entry in place. Same trust gate and
/// in-band failure surface as `memory.remember`; a well-formed id that
/// matches no entry returns `notFound`.
#[tauri::command]
pub async fn memory_update(
    req: IpcRequest<MemoryUpdatePayload>,
    state: State<'_, AppState>,
) -> Result<MemoryUpdateResponse, IpcError> {
    req.check_version()?;
    let payload = req.payload;
    let project = match trusted_open(&state) {
        Some(p) => p,
        None => return Err(IpcError::NeedsApproval),
    };
    Ok(memory_update_impl(
        project.root.as_path(),
        &payload.entry_id,
        &payload.text,
    ))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MemoryForgetPayload {
    /// Opaque entry id minted by a prior `memory.remember`. Shape
    /// is validated server-side; a malformed id surfaces as
    /// `MemoryForgetFailure::BadId` in-band.
    pub entry_id: String,
}

#[tauri::command]
pub async fn memory_forget(
    req: IpcRequest<MemoryForgetPayload>,
    state: State<'_, AppState>,
) -> Result<MemoryForgetResponse, IpcError> {
    req.check_version()?;
    let payload = req.payload;
    let project = match trusted_open(&state) {
        Some(p) => p,
        None => return Err(IpcError::NeedsApproval),
    };
    Ok(memory_forget_impl(
        project.root.as_path(),
        &payload.entry_id,
    ))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MemorySearchPayload {
    /// Search needle. Trimmed server-side; an empty / whitespace-
    /// only query rejects with `EmptyQuery`.
    pub query: String,
    /// Max number of hits to return. Clamped to
    /// `[1, SEARCH_MAX_LIMIT]`; out-of-range values reject with
    /// `BadLimit` rather than silently clamping so the caller's
    /// intent stays honest.
    pub limit: u32,
}

#[tauri::command]
pub async fn memory_search(
    req: IpcRequest<MemorySearchPayload>,
    state: State<'_, AppState>,
) -> Result<MemorySearchResponse, IpcError> {
    req.check_version()?;
    let payload = req.payload;
    let project = match trusted_open(&state) {
        Some(p) => p,
        None => return Err(IpcError::NeedsApproval),
    };
    Ok(memory_search_impl(
        project.root.as_path(),
        &payload.query,
        payload.limit,
    ))
}

/// D54: read-only distillation preview. Wires the D48 scaffold
/// (`memory::distill_preview`) through to a `memory.distillPreview`
/// IPC verb so the panel can show "candidates for compaction" without
/// any rewrite or LLM summarizer involvement.
///
/// Trust gate is the same as `memory.index` / `memory.search`: the
/// store lives under `<project>/.plume/memory/`, and a no-project
/// caller has nothing to read. Surfaces a `MemoryStoreError` as
/// `IpcError::Internal` for parity with the existing read verbs —
/// callers that need fine-grained typing can disambiguate via the
/// message; today the panel just renders the string.
#[tauri::command]
pub async fn memory_distill_preview(
    req: IpcRequest<EmptyPayload>,
    state: State<'_, AppState>,
) -> Result<DistillPreview, IpcError> {
    req.check_version()?;
    let project = match trusted_open(&state) {
        Some(p) => p,
        None => return Err(IpcError::NeedsApproval),
    };
    memory_distill_preview_impl(project.root.as_path()).map_err(|e| IpcError::Internal(e.0))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MemoryDistillApplyPayload {
    /// Group ids the user confirmed in the distillation preview. The
    /// backend re-derives the live groups under the memory mutex and
    /// only compacts ids that still match; stale ids are no-ops.
    pub group_ids: Vec<String>,
}

/// D64: apply the rule-based dedupe pass for the confirmed groups —
/// the first writing verb of the distillation track. Wires
/// `memory::distill_apply` through to `memory.distillApply`.
///
/// Same trust gate as `memory.distillPreview`: the store lives under
/// `<project>/.plume/memory/`, and a no-project caller has nothing to
/// rewrite. Store-write failures come back in-band on the response
/// (`MemoryDistillApplyResponse::Err`); the `Result` only surfaces
/// IPC-shape (`Version`) and trust (`NeedsApproval`) errors.
#[tauri::command]
pub async fn memory_distill_apply(
    req: IpcRequest<MemoryDistillApplyPayload>,
    state: State<'_, AppState>,
) -> Result<MemoryDistillApplyResponse, IpcError> {
    req.check_version()?;
    let payload = req.payload;
    let project = match trusted_open(&state) {
        Some(p) => p,
        None => return Err(IpcError::NeedsApproval),
    };
    Ok(memory_distill_apply_impl(
        project.root.as_path(),
        &payload.group_ids,
    ))
}

/// D69: read the distillation audit log (newest first, capped on disk).
/// Same trust gate and `MemoryStoreError → Internal` mapping as the
/// other read verbs. The log records every compaction so the one
/// memory verb that deletes un-named data leaves a visible trail.
#[tauri::command]
pub async fn memory_distill_log(
    req: IpcRequest<EmptyPayload>,
    state: State<'_, AppState>,
) -> Result<Vec<DistillLogEntry>, IpcError> {
    req.check_version()?;
    let project = match trusted_open(&state) {
        Some(p) => p,
        None => return Err(IpcError::NeedsApproval),
    };
    memory_distill_log_impl(project.root.as_path()).map_err(|e| IpcError::Internal(e.0))
}

/// D71: read the curated memory topic files (INDEX/USER/SOUL +
/// `topics/*.md`). Read-only, capped, symlink-safe; same trust gate as
/// `memory.index`. The core trio is always returned (even when missing)
/// so the panel can surface the convention.
#[tauri::command]
pub async fn memory_topics(
    req: IpcRequest<EmptyPayload>,
    state: State<'_, AppState>,
) -> Result<MemoryTopics, IpcError> {
    req.check_version()?;
    let project = match trusted_open(&state) {
        Some(p) => p,
        None => return Err(IpcError::NeedsApproval),
    };
    memory_topics_impl(project.root.as_path()).map_err(|e| IpcError::Internal(e.0))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MemorySetLinksPayload {
    pub id: String,
    pub links: Vec<String>,
}

#[tauri::command]
pub async fn memory_set_links(
    req: IpcRequest<MemorySetLinksPayload>,
    state: State<'_, AppState>,
) -> Result<MemorySetLinksResponse, IpcError> {
    req.check_version()?;
    let payload = req.payload;
    let project = trusted_open(&state).ok_or(IpcError::NeedsApproval)?;
    Ok(memory_set_links_impl(
        project.root.as_path(),
        &payload.id,
        &payload.links,
    ))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EmptyPayload {}

fn trusted_open(state: &AppState) -> Option<OpenProject> {
    let open = state.session.current()?;
    let trusted = {
        let store = state.trust.lock().expect("trust mutex poisoned");
        store.is_trusted(&open.root)
    };
    if trusted {
        Some(open)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;
    use crate::chat::stream::ChatStreamRegistry;
    use crate::project::trust::TrustStore;
    use crate::project::ProjectSession;

    fn user_memory_test_state() -> (std::path::PathBuf, AppState) {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0);
        let base = std::env::temp_dir().join(format!(
            "plume-user-memory-command-{}-{nonce}",
            std::process::id()
        ));
        let state = AppState {
            session: ProjectSession::default(),
            trust: Mutex::new(TrustStore::load(base.join("trust.json"))),
            chat_streams: Arc::new(ChatStreamRegistry::default()),
            agent_config: Mutex::new(crate::agent::AgentConfig::default()),
            local_sessions_dir: base.join("sessions"),
            user_memory_dir: base.join("memory"),
            catalog_store: Arc::new(crate::providers::catalog::CatalogStore::new(base.clone())),
            catalog_downloads: Arc::new(
                crate::providers::catalog_download::CatalogDownloadRegistry::default(),
            ),
        };
        (base, state)
    }

    #[test]
    fn remember_payload_deserialises_camel_case() {
        let raw = serde_json::json!({ "text": "hello" });
        let p: MemoryRememberPayload = serde_json::from_value(raw).unwrap();
        assert_eq!(p.text, "hello");
    }

    #[test]
    fn remember_payload_rejects_unknown_field() {
        let raw = serde_json::json!({ "text": "hi", "extra": 1 });
        let res = serde_json::from_value::<MemoryRememberPayload>(raw);
        assert!(res.is_err());
    }

    #[test]
    fn forget_payload_deserialises_camel_case() {
        let raw = serde_json::json!({ "entryId": "m_00000000000000000000000000000000" });
        let p: MemoryForgetPayload = serde_json::from_value(raw).unwrap();
        assert_eq!(p.entry_id, "m_00000000000000000000000000000000");
    }

    #[test]
    fn forget_payload_rejects_snake_case() {
        let raw = serde_json::json!({ "entry_id": "m_00000000000000000000000000000000" });
        let res = serde_json::from_value::<MemoryForgetPayload>(raw);
        assert!(
            res.is_err(),
            "snake_case field should not deserialise: {:?}",
            res
        );
    }

    #[test]
    fn set_links_payload_is_strict_and_has_no_scope_or_root() {
        let value = serde_json::json!({
            "id": "m_00000000000000000000000000000000",
            "links": ["topics/testing.md"]
        });
        let payload: MemorySetLinksPayload = serde_json::from_value(value).unwrap();
        assert_eq!(payload.links, vec!["topics/testing.md"]);
        for extra in ["root", "scope", "projectRoot"] {
            let mut value = serde_json::json!({"id": payload.id, "links": []});
            value
                .as_object_mut()
                .unwrap()
                .insert(extra.to_string(), serde_json::json!("x"));
            assert!(serde_json::from_value::<MemorySetLinksPayload>(value).is_err());
        }
    }

    #[test]
    fn user_memory_payloads_are_camel_case_strict_and_accept_no_caller_path() {
        let remember: UserMemoryRememberPayload =
            serde_json::from_value(serde_json::json!({"text": "hello"})).unwrap();
        assert_eq!(remember.text, "hello");
        let update: UserMemoryUpdatePayload = serde_json::from_value(serde_json::json!({
            "entryId": "m_00000000000000000000000000000000",
            "text": "updated"
        }))
        .unwrap();
        assert_eq!(update.text, "updated");
        let forget: UserMemoryForgetPayload = serde_json::from_value(serde_json::json!({
            "entryId": "m_00000000000000000000000000000000"
        }))
        .unwrap();
        assert!(forget.entry_id.starts_with("m_"));
        let search: UserMemorySearchPayload =
            serde_json::from_value(serde_json::json!({"query": "hello", "limit": 5})).unwrap();
        assert_eq!(search.limit, 5);

        for forbidden in [
            "root",
            "scope",
            "projectRoot",
            "appDataDir",
            "userMemoryDir",
        ] {
            let mut value = serde_json::json!({"text": "hello"});
            value
                .as_object_mut()
                .unwrap()
                .insert(forbidden.to_string(), serde_json::json!("/tmp/escape"));
            assert!(serde_json::from_value::<UserMemoryRememberPayload>(value).is_err());
        }
        assert!(
            serde_json::from_value::<UserMemoryUpdatePayload>(serde_json::json!({
                "entry_id": "m_00000000000000000000000000000000",
                "text": "updated"
            }))
            .is_err()
        );
    }

    #[test]
    fn user_memory_core_is_available_without_an_open_or_trusted_project() {
        let (base, state) = user_memory_test_state();
        assert!(state.session.current().is_none());
        let remembered = serde_json::to_value(user_memory_remember_for_state(&state, "global"))
            .expect("remember response");
        assert_eq!(remembered["ok"], true);
        let index = user_memory_index_for_state(&state).expect("index without project");
        assert_eq!(index.entries.len(), 1);
        assert!(!base.join("project/.plume").exists());
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn distill_apply_payload_deserialises_camel_case() {
        let raw = serde_json::json!({ "groupIds": ["dup_abc_2", "dup_def_3"] });
        let p: MemoryDistillApplyPayload = serde_json::from_value(raw).unwrap();
        assert_eq!(p.group_ids, vec!["dup_abc_2", "dup_def_3"]);
    }

    #[test]
    fn distill_apply_payload_accepts_empty_list() {
        let raw = serde_json::json!({ "groupIds": [] });
        let p: MemoryDistillApplyPayload = serde_json::from_value(raw).unwrap();
        assert!(p.group_ids.is_empty());
    }

    #[test]
    fn distill_apply_payload_rejects_snake_case() {
        let raw = serde_json::json!({ "group_ids": ["dup_abc_2"] });
        let res = serde_json::from_value::<MemoryDistillApplyPayload>(raw);
        assert!(res.is_err(), "snake_case field should not deserialise");
    }
}
