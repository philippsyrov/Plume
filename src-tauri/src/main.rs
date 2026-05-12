// Plume desktop entry point.
//
// Modules wired here:
//   error     IPC envelope + error model (docs/IPC_CONTRACT.md)
//   safety    path validation; command + redaction safety follow
//   project   open project + ProjectMeta + persisted trust
//   commands  Tauri IPC command handlers
//
// Real provider, patch, fs, and chat work lands in subsequent slices.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::Mutex;

use tauri::Manager;

mod commands;
mod error;
mod fs;
mod project;
mod providers;
mod safety;
mod system;

use commands::fs::{fs_list, fs_read};
use commands::project::{
    project_open, project_refresh, project_trust, project_trust_state, AppState,
};
use commands::providers::{providers_health, providers_list, providers_model_details};
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
            providers_model_details,
            system_snapshot,
        ])
        .run(tauri::generate_context!())
        .expect("Plume failed to launch");
}

/// Liveness probe. Kept until the frontend has a real `chat.send`
/// path so dev tooling can confirm the bridge is up.
#[tauri::command]
fn ping() -> &'static str {
    "pong"
}
