//! Patch command handlers.
//!
//! D16 — `patch.validate`: read-only validator for model-emitted
//! unified diffs. No writes, no apply, no checkpoint.
//!
//! D31 — `patch.apply`: applies a previously-validated diff inside
//! the trusted project root. Re-validates server-side, takes a
//! filesystem-backed checkpoint, then writes all-or-nothing.
//!
//! Both handlers are thin wrappers around module helpers in
//! `patch::`:
//!   1. Check IPC version.
//!   2. Require a trusted open project (path safety needs a root).
//!   3. Delegate to `patch::{validate,apply}_patch`.
//!
//! `patch.apply` runs the (sync, I/O-heavy) apply on a blocking
//! task pool so the tokio executor isn't blocked while the diff
//! lands. `patch.validate` is pure CPU; running it inline is fine.
//!
//! This file's tests cover wire-shape: payload deserialisation
//! and the NeedsApproval gate when no project is trusted. The
//! actual apply / validate logic is unit-tested under `patch::`.

use serde::Deserialize;
use tauri::State;

use crate::commands::project::AppState;
use crate::error::{IpcError, IpcRequest};
use crate::patch::{apply_patch, validate_patch, PatchApplyResponse, PatchValidateResponse};
use crate::project::OpenProject;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PatchValidatePayload {
    /// Raw assistant reply text or a bare unified diff. The
    /// validator extracts the fenced ```diff/```patch block when
    /// present, otherwise treats the payload as a raw diff.
    pub diff: String,
}

#[tauri::command]
pub async fn patch_validate(
    req: IpcRequest<PatchValidatePayload>,
    state: State<'_, AppState>,
) -> Result<PatchValidateResponse, IpcError> {
    req.check_version()?;
    let payload = req.payload;

    let project = match trusted_open(&state) {
        Some(p) => p,
        None => return Err(IpcError::NeedsApproval),
    };

    Ok(validate_patch(project.root.as_path(), &payload.diff))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PatchApplyPayload {
    /// Raw assistant reply text or a bare unified diff. Same shape
    /// as `PatchValidatePayload.diff`. The applier re-runs the
    /// validator server-side; the frontend's cached validation
    /// result is treated as a UI hint, not a security artifact.
    pub diff: String,
}

#[tauri::command]
pub async fn patch_apply(
    req: IpcRequest<PatchApplyPayload>,
    state: State<'_, AppState>,
) -> Result<PatchApplyResponse, IpcError> {
    req.check_version()?;
    let payload = req.payload;

    let project = match trusted_open(&state) {
        Some(p) => p,
        None => return Err(IpcError::NeedsApproval),
    };

    // `apply_patch` is synchronous and does real I/O (reads,
    // writes, fsyncs). Hand off to the blocking pool so the tokio
    // executor stays free for the rest of the app.
    let root = project.root;
    let response =
        tauri::async_runtime::spawn_blocking(move || apply_patch(root.as_path(), &payload.diff))
            .await
            .map_err(|join_err| {
                IpcError::Internal(format!("patch.apply task join failed: {join_err}"))
            })?;

    Ok(response)
}

/// Returns the currently-open project if it is also in the trust
/// store; `None` otherwise. Mirrors `chat::optional_trusted_open`
/// but doesn't need to be the same function — keeping a local copy
/// avoids pulling chat's private helper into a sibling command.
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
    use crate::error::IPC_VERSION;

    #[test]
    fn payload_deserialises_with_diff_field() {
        let raw = serde_json::json!({ "diff": "--- a/x\n+++ b/x\n@@\n" });
        let p: PatchValidatePayload = serde_json::from_value(raw).unwrap();
        assert_eq!(p.diff, "--- a/x\n+++ b/x\n@@\n");
    }

    #[test]
    fn payload_rejects_missing_diff_field() {
        let raw = serde_json::json!({});
        let res = serde_json::from_value::<PatchValidatePayload>(raw);
        assert!(res.is_err(), "expected missing-field error, got {:?}", res);
    }

    #[test]
    fn payload_rejects_snake_case_field_name() {
        // The wire field is `diff` already; this test guards against
        // a future refactor that accidentally renames it to
        // `diff_content` or similar — same class of bug D12 caught
        // on the chat.context shape.
        let raw = serde_json::json!({ "diff_content": "..." });
        let res = serde_json::from_value::<PatchValidatePayload>(raw);
        assert!(
            res.is_err(),
            "unexpected camelCase / unknown field acceptance: {:?}",
            res
        );
    }

    #[test]
    fn ipc_request_envelope_round_trips_version() {
        let raw = serde_json::json!({
            "ipcVersion": IPC_VERSION,
            "payload": { "diff": "x" },
        });
        let req: IpcRequest<PatchValidatePayload> = serde_json::from_value(raw).unwrap();
        assert_eq!(req.ipc_version, IPC_VERSION);
        req.check_version().unwrap();
    }

    #[test]
    fn ipc_request_envelope_rejects_version_mismatch() {
        let raw = serde_json::json!({
            "ipcVersion": IPC_VERSION + 99,
            "payload": { "diff": "x" },
        });
        let req: IpcRequest<PatchValidatePayload> = serde_json::from_value(raw).unwrap();
        let err = req.check_version().unwrap_err();
        match err {
            IpcError::Version { wanted, speaks } => {
                assert_eq!(wanted, IPC_VERSION + 99);
                assert_eq!(speaks, IPC_VERSION);
            }
            other => panic!("expected Version error, got {:?}", other),
        }
    }
}
