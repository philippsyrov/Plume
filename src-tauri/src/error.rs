//! IPC envelope and error model.
//!
//! See `docs/IPC_CONTRACT.md`. This file is the Rust mirror of that
//! contract; if shapes drift the contract wins.

use serde::{Deserialize, Serialize};

/// Single source of truth for the IPC major version. Bump on any
/// incompatible change to a command shape; do not bump for additions.
pub const IPC_VERSION: u32 = 1;

/// Uniform request envelope. Every Tauri command takes `IpcRequest<T>`
/// so version mismatch is rejected before any handler runs.
///
/// `rename_all = "camelCase"` is load-bearing: TS sends `ipcVersion`,
/// Rust holds `ipc_version`.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IpcRequest<T> {
    pub ipc_version: u32,
    pub payload: T,
}

impl<T> IpcRequest<T> {
    pub fn check_version(&self) -> Result<(), IpcError> {
        if self.ipc_version != IPC_VERSION {
            return Err(IpcError::Version {
                wanted: self.ipc_version,
                speaks: IPC_VERSION,
            });
        }
        Ok(())
    }
}

/// IPC error model. Frontend matches on `kind`; never parses `message`.
///
/// `#[allow(dead_code)]` covers the variants reserved for upcoming
/// slices (cancellation, providers). They are part of the published
/// contract today; deleting them and re-adding later would be a
/// contract regression.
///
/// `NeedsApproval` vs `Blocked`: `NeedsApproval` clears with a user
/// click (e.g. "Trust this project"). `Blocked` is policy the user
/// cannot override from the current UI — secret-pattern filenames,
/// `.git/objects/**`, oversized display reads.
#[allow(dead_code)]
#[derive(Debug, thiserror::Error, Serialize)]
#[serde(tag = "kind", content = "details")]
pub enum IpcError {
    #[error("path is outside project root: {0}")]
    PathEscape(String),
    #[error("path not found: {0}")]
    NotFound(String),
    #[error("operation requires user approval")]
    NeedsApproval,
    #[error("operation cancelled")]
    Cancelled,
    #[error("provider {provider} unavailable: {reason}")]
    ProviderDown { provider: String, reason: String },
    #[error("invalid argument: {0}")]
    BadArgument(String),
    #[error("blocked by safety policy: {0}")]
    Blocked(String),
    /// A durable store has no room for the write. Distinct from `Blocked`
    /// because the user's next action differs: `Blocked` is policy they cannot
    /// override, while this one clears the moment they free space. The numbers
    /// are structured so the surface reports them instead of parsing a
    /// sentence. `rename_all` is load-bearing — `src/lib/api/errors.ts` reads
    /// `usedBytes`.
    #[error("store full: {used_bytes} of {cap_bytes} bytes used")]
    #[serde(rename_all = "camelCase")]
    StorageFull { used_bytes: u64, cap_bytes: u64 },
    #[error("internal: {0}")]
    Internal(String),
    #[error("ipc version mismatch (frontend wants {wanted}, backend speaks {speaks})")]
    Version { wanted: u32, speaks: u32 },
}

impl From<crate::safety::path::PathError> for IpcError {
    fn from(err: crate::safety::path::PathError) -> Self {
        use crate::safety::path::PathError;
        match err {
            PathError::Escape(p) => IpcError::PathEscape(p.display().to_string()),
            PathError::NotFound(p) => IpcError::NotFound(p.display().to_string()),
            PathError::NotADirectory(p) => {
                IpcError::BadArgument(format!("not a directory: {}", p.display()))
            }
            PathError::Hardlink(p) => {
                IpcError::PathEscape(format!("hardlink alias rejected: {}", p.display()))
            }
            PathError::Io { path, source } => {
                IpcError::Internal(format!("io error on {}: {}", path.display(), source))
            }
        }
    }
}
