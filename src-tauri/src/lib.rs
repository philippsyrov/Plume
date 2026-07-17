// Plume library crate.
//
// The module tree lives here (not in main.rs) so that additional
// binary targets — the D129C `plume_bench` benchmark sidecar — can
// reuse the REAL product modules (`patch`, `prompts`, `chat`, …)
// instead of reimplementing them. The desktop entry point stays
// `src/main.rs`, which just calls [`run`].
//
// Modules:
//   error     IPC envelope + error model (docs/IPC_CONTRACT.md)
//   safety    path validation; command + redaction safety follow
//   project   open project + ProjectMeta + persisted trust
//   chat      streaming read-only chat transport (Ollama + Plume-managed
//             MLX-LM as of D45)
//   prompts   D8 prompt assembly + Rust-private prompt-read +
//             content redaction; never exposed as an IPC verb
//   patch     D16 read-only unified-diff parser + validator, D31
//             `patch.apply` (the first writing verb), D33
//             `patch.revert` + rename apply. Three-way merge is
//             reserved for a follow-up slice.
//   sessions  D63A durable chat sessions — SQLite spine for local
//             (`<app-data>/sessions`) and trusted-project
//             (`<project>/.plume/sessions`) chat databases
//   commands  Tauri IPC command handlers

use std::sync::{Arc, Mutex};

use tauri::Manager;

// Only the modules the `plume_bench` sidecar reuses are public; the
// rest stay crate-private so the library surface is exactly the
// benchmark-parity surface and nothing more.
mod agent;
#[cfg(test)]
mod app_commands;
mod browser;
pub mod chat;
mod commands;
mod error;
mod fs;
mod memory;
pub mod patch;
mod project;
pub mod prompts;
mod providers;
mod safety;
mod sessions;
mod skills;
mod system;

// The product's chat budget constants, re-exported for the sidecar so
// benchmark and app timeout behavior cannot drift.
pub use commands::chat::{CHAT_OVERALL_BUDGET, CONNECT_TIMEOUT};

fn plume_context<R: tauri::Runtime>() -> tauri::Context<R> {
    tauri::generate_context!()
}

use chat::stream::ChatStreamRegistry;
use commands::agent::{agent_dry_run, agent_single_step};
use commands::browser::{
    browser_sandbox_back, browser_sandbox_capture_screenshot, browser_sandbox_capture_text,
    browser_sandbox_close, browser_sandbox_focus, browser_sandbox_forward, browser_sandbox_open,
    browser_sandbox_reload, browser_sandbox_state,
};
use commands::browser_workspace::{
    browser_workspace_load, browser_workspace_reset, browser_workspace_save,
};
use commands::chat::{chat_cancel, chat_context, chat_send};
use commands::fs::{fs_list, fs_read};
use commands::memory::{
    memory_distill_apply, memory_distill_log, memory_distill_preview, memory_forget, memory_index,
    memory_remember, memory_search, memory_set_links, memory_topics, memory_update,
    memory_user_forget, memory_user_index, memory_user_remember, memory_user_search,
    memory_user_update,
};
use commands::patch::{patch_apply, patch_revert, patch_validate};
use commands::project::{
    project_open, project_refresh, project_trust, project_trust_state, AppState,
};
use commands::providers::{
    providers_catalog_list, providers_health, providers_list, providers_list_servers,
    providers_local_model_details, providers_local_models, providers_model_details,
    providers_server_diagnostics, providers_start_server, providers_stop_server,
};
use commands::session::{
    session_set_allowlist, session_set_approval_policy, session_set_mode, session_state,
};
use commands::sessions::{
    sessions_archive, sessions_create, sessions_delete, sessions_fork, sessions_list,
    sessions_load, sessions_rename, sessions_rollback, sessions_save_transcript, sessions_search,
};
use commands::skills::{
    skills_apply, skills_list, skills_load, skills_preview, skills_promote_preview,
    skills_promotion_context,
};
use commands::system::system_snapshot;
use commands::task_browser::{
    task_browser_activate, task_browser_back, task_browser_capture_screenshot,
    task_browser_capture_text, task_browser_close_tab, task_browser_deactivate,
    task_browser_forward, task_browser_navigate, task_browser_open_tab, task_browser_reload,
    task_browser_select_tab, task_browser_set_geometry, task_browser_set_suspended,
};
use commands::tools::{tools_list, tools_search};
use project::trust::TrustStore;
use project::ProjectSession;

/// Desktop entry point body — builds and runs the Tauri app.
pub fn run() {
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
                agent_config: Mutex::new(agent::AgentConfig::default()),
                local_sessions_dir: sessions::local_sessions_dir(&app_data_dir),
                user_memory_dir: memory::user_memory_dir(&app_data_dir),
                catalog_store: Arc::new(providers::catalog::CatalogStore::new(app_data_dir)),
            });
            app.manage(browser::state::BrowserSandboxStore::default());
            app.manage(browser::runtime::BrowserRuntimeManager::new(
                browser::runtime::TauriBrowserRuntimePort::new(app.handle().clone()),
            ));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            ping,
            browser_sandbox_open,
            browser_sandbox_close,
            browser_sandbox_state,
            browser_sandbox_focus,
            browser_sandbox_back,
            browser_sandbox_forward,
            browser_sandbox_reload,
            browser_sandbox_capture_text,
            browser_sandbox_capture_screenshot,
            browser_workspace_load,
            browser_workspace_save,
            browser_workspace_reset,
            task_browser_activate,
            task_browser_deactivate,
            task_browser_set_suspended,
            task_browser_open_tab,
            task_browser_close_tab,
            task_browser_select_tab,
            task_browser_navigate,
            task_browser_back,
            task_browser_forward,
            task_browser_reload,
            task_browser_set_geometry,
            task_browser_capture_text,
            task_browser_capture_screenshot,
            project_open,
            project_refresh,
            project_trust,
            project_trust_state,
            fs_list,
            fs_read,
            providers_list,
            providers_health,
            providers_catalog_list,
            providers_local_models,
            providers_local_model_details,
            providers_model_details,
            providers_start_server,
            providers_stop_server,
            providers_server_diagnostics,
            providers_list_servers,
            system_snapshot,
            chat_send,
            chat_cancel,
            chat_context,
            patch_validate,
            patch_apply,
            patch_revert,
            memory_index,
            memory_remember,
            memory_update,
            memory_forget,
            memory_search,
            memory_distill_preview,
            memory_distill_apply,
            memory_distill_log,
            memory_topics,
            memory_set_links,
            memory_user_index,
            memory_user_remember,
            memory_user_update,
            memory_user_forget,
            memory_user_search,
            session_set_mode,
            session_set_approval_policy,
            session_set_allowlist,
            session_state,
            sessions_list,
            sessions_fork,
            sessions_rollback,
            sessions_create,
            sessions_load,
            sessions_rename,
            sessions_archive,
            sessions_delete,
            sessions_save_transcript,
            sessions_search,
            skills_list,
            skills_load,
            skills_preview,
            skills_apply,
            skills_promote_preview,
            skills_promotion_context,
            tools_list,
            tools_search,
            agent_dry_run,
            agent_single_step,
        ])
        .build(plume_context())
        .expect("Plume failed to launch")
        .run(|_app_handle, event| {
            // Thermos I1: NORMAL-exit sweep for Plume-managed MLX
            // servers. The supervisor's children live in their own
            // sessions (see `configure_own_session`) and its registry
            // is a leaked static, so without this hook a quit leaves
            // every running `python -m mlx_lm server` orphaned. The
            // sweep is bounded: one stopper thread per child (capped
            // by MAX_MANAGED_SERVERS), each running the same
            // SIGINT-grace → SIGKILL escalation as
            // `providers.stopServer`, joined concurrently — worst
            // case ~one grace period, not one per child.
            //
            // This covers NORMAL exit only. A SIGKILLed or crashed
            // Plume never reaches this event, and the children's
            // detached sessions mean nothing else signals them —
            // that limitation is documented in
            // `docs/MLX_RUNTIME.md § Shutdown`.
            if matches!(event, tauri::RunEvent::Exit) {
                let summary = providers::mlx_lm::shutdown_all_managed_servers();
                if summary.stopped > 0 || summary.errors > 0 || summary.killed_starting > 0 {
                    tracing::info!(
                        stopped = summary.stopped,
                        escalated = summary.escalated,
                        errors = summary.errors,
                        killed_starting = summary.killed_starting,
                        "exit sweep stopped managed MLX servers"
                    );
                }
            }
        });
}

/// Liveness probe. Kept around even now that `chat.send` is wired up
/// (D7) because the bridge can still be exercised without a running
/// model — dev tooling and the verify script use `ping` as a cheap
/// "is the IPC layer reachable" smoke.
#[tauri::command]
fn ping() -> &'static str {
    "pong"
}

#[cfg(test)]
mod manifest_tests {
    /// D132 packaged-app regression: the crate ships two binaries
    /// (desktop shell + plume_bench sidecar), and without an explicit
    /// `default-run` the Tauri bundler packaged plume_bench as
    /// Plume.app's CFBundleExecutable — the app exited immediately on
    /// launch. This pins the manifest line that selects the desktop
    /// shell; smoke-app.sh asserts the built bundle agrees.
    #[test]
    fn default_run_pins_the_desktop_binary() {
        let manifest = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"));
        assert!(
            manifest.contains("default-run = \"plume\""),
            "Cargo.toml must pin default-run = \"plume\" so the Tauri bundler \
             packages the desktop shell, not the plume_bench sidecar"
        );
    }
}
