//! Tauri IPC command handlers.
//!
//! Every handler takes an `IpcRequest<T>` and returns
//! `Result<Response, IpcError>`. Validation order: version, then args,
//! then state. See `docs/IPC_CONTRACT.md`.

pub mod agent;
pub mod browser;
pub mod chat;
pub mod fs;
pub mod memory;
pub mod patch;
pub mod project;
pub mod providers;
pub mod session;
pub mod sessions;
pub mod skills;
pub mod system;
pub mod tools;
