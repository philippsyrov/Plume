//! Tauri IPC command handlers.
//!
//! Every handler takes an `IpcRequest<T>` and returns
//! `Result<Response, IpcError>`. Validation order: version, then args,
//! then state. See `docs/IPC_CONTRACT.md`.

pub mod chat;
pub mod fs;
pub mod project;
pub mod providers;
pub mod system;
