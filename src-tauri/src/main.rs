// Plume desktop entry point.
//
// Modules wired here:
//   error     IPC envelope + error model (docs/IPC_CONTRACT.md)
//   safety    path validation; command + redaction safety follow
//   project   open project + ProjectMeta + persisted trust
//   chat      D7.1 streaming read-only chat transport (Ollama only)
//   prompts   D8 prompt assembly + Rust-private prompt-read +
//             content redaction; never exposed as an IPC verb
//   patch     D16 read-only unified-diff parser + validator,
//             plus D31 `patch.apply` (the first writing verb).
//             `patch.revert`, rename apply, and three-way merge
//             are reserved for follow-up slices.
//   commands  Tauri IPC command handlers
//
// Command-runner and agent-loop work land in later slices.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::{Arc, Mutex};

use tauri::Manager;

mod chat;
mod commands;
mod error;
mod fs;
mod memory;
mod patch;
mod project;
mod prompts;
mod providers;
mod safety;
mod system;

use chat::stream::ChatStreamRegistry;
use commands::chat::{chat_cancel, chat_context, chat_send};
use commands::fs::{fs_list, fs_read};
use commands::memory::{
    memory_distill_apply, memory_distill_preview, memory_forget, memory_index, memory_remember,
    memory_search,
};
use commands::patch::{patch_apply, patch_revert, patch_validate};
use commands::project::{
    project_open, project_refresh, project_trust, project_trust_state, AppState,
};
use commands::providers::{
    providers_health, providers_list, providers_local_model_details, providers_local_models,
    providers_model_details, providers_server_diagnostics, providers_start_server,
    providers_stop_server,
};
use commands::system::system_snapshot;
use project::trust::TrustStore;
use project::ProjectSession;

fn main() {
    let filter = tracing_subscriber::EnvFilter::try_from_env("PLUME_LOG")
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    tracing_subscriber::fmt().with_env_filter(filter).init();

    tauri::Builder::default()
        .setup(|app| {
            let app_data_dir = app
                .path()
                .app_data_dir()
                .expect("Tauri could not resolve app_data_dir");
            let trust_path = app_data_dir.join("trusted-projects.json");
            tracing::info!(path = %trust_path.display(), "trust store path");

            app.manage(AppState {
                session: ProjectSession::default(),
                trust: Mutex::new(TrustStore::load(trust_path)),
                chat_streams: Arc::new(ChatStreamRegistry::default()),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            ping,
            project_open,
            project_refresh,
            project_trust,
            project_trust_state,
            fs_list,
            fs_read,
            providers_list,
            providers_health,
            providers_local_models,
            providers_local_model_details,
            providers_model_details,
            providers_start_server,
            providers_stop_server,
            providers_server_diagnostics,
            system_snapshot,
            chat_send,
            chat_cancel,
            chat_context,
            patch_validate,
            patch_apply,
            patch_revert,
            memory_index,
            memory_remember,
            memory_forget,
            memory_search,
            memory_distill_preview,
            memory_distill_apply,
        ])
        .run(tauri::generate_context!())
        .expect("Plume failed to launch");
}

/// Liveness probe. Kept around even now that `chat.send` is wired up
/// (D7) because the bridge can still be exercised without a running
/// model — dev tooling and the verify script use `ping` as a cheap
/// "is the IPC layer reachable" smoke.
#[tauri::command]
fn ping() -> &'static str {
    "pong"
}
