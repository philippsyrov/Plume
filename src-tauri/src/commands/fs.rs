//! `fs.list` and `fs.read` Tauri command handlers.
//!
//! Both verbs share a fixed pre-flight: IPC version, then "is a
//! project open and trusted." The canonical project root comes from
//! `ProjectSession`, never from the frontend — see `docs/IPC_CONTRACT.md`
//! § fs.

use serde::Deserialize;
use tauri::State;

use crate::commands::project::AppState;
use crate::error::{IpcError, IpcRequest};
use crate::fs::{list_dir, read_file, resolve, FileContent, FileEntry};
use crate::project::OpenProject;

#[derive(Debug, Deserialize)]
pub struct PathPayload {
    pub path: String,
}

#[tauri::command]
pub async fn fs_list(
    req: IpcRequest<PathPayload>,
    state: State<'_, AppState>,
) -> Result<Vec<FileEntry>, IpcError> {
    req.check_version()?;
    let open = require_trusted_open(&state)?;
    let target = resolve(&open.root, &req.payload.path)?;
    list_dir(&open.root, &target)
}

#[tauri::command]
pub async fn fs_read(
    req: IpcRequest<PathPayload>,
    state: State<'_, AppState>,
) -> Result<FileContent, IpcError> {
    req.check_version()?;
    let open = require_trusted_open(&state)?;
    let target = resolve(&open.root, &req.payload.path)?;
    read_file(&open.root, &target)
}

/// "There is an open project AND its canonical root is in the trust
/// store." Returns the open project so callers reuse the canonical
/// root from `ProjectSession` rather than re-canonicalizing.
fn require_trusted_open(state: &AppState) -> Result<OpenProject, IpcError> {
    let open = state.session.current().ok_or(IpcError::NeedsApproval)?;
    let trusted = {
        let store = state.trust.lock().expect("trust mutex poisoned");
        store.is_trusted(&open.root)
    };
    if !trusted {
        return Err(IpcError::NeedsApproval);
    }
    Ok(open)
}
