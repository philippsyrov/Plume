//! `system.snapshot` Tauri command handler.
//!
//! Returns a cheap point-in-time read of host machine state — memory,
//! swap, load average, identifying labels. The status strip polls
//! this every 5–10 s; see `docs/IPC_CONTRACT.md § system` for the
//! wire shape.
//!
//! Doesn't require an open project — host state is global.

use crate::commands::project::EmptyPayload;
use crate::error::{IpcError, IpcRequest};
use crate::system::{self, MachineSnapshot};

#[tauri::command]
pub async fn system_snapshot(req: IpcRequest<EmptyPayload>) -> Result<MachineSnapshot, IpcError> {
    req.check_version()?;
    // The macOS readers shell out to small CLI tools (`sysctl`,
    // `vm_stat`, `uname`, `sw_vers`). Each is microseconds; combined
    // they stay under ~10 ms on a real machine. We still hop to the
    // blocking pool so the async runtime is not held during the
    // process spawns.
    let snap = tauri::async_runtime::spawn_blocking(system::snapshot)
        .await
        .map_err(|e| IpcError::Internal(format!("system.snapshot task join: {e}")))?;
    Ok(snap)
}
