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
//! All three are gated on a trusted open project (same pattern as
//! the patch verbs). The read/write functions live in
//! `crate::memory`; this file is a thin adapter that maps payloads
//! and projects onto them.
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
    forget as memory_forget_impl, read_index, remember as memory_remember_impl,
    search as memory_search_impl, MemoryForgetResponse, MemoryIndex, MemoryRememberResponse,
    MemorySearchResponse,
};
use crate::project::OpenProject;

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
    use super::*;

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
}
