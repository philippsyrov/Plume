// Plume desktop entry point.
//
// Intentionally tiny. Real work will live in modules under src/ as features
// land:
//   project   open folder, detect AGENTS.md, package manifests
//   fs        sandboxed reads/writes inside project root
//   git       branch, status, diff, checkpoint
//   providers MLX-LM / Ollama / LM Studio / llama.cpp adapters
//   process   start/stop provider processes
//   safety    path + command validation, approval ledger
//   patch     validate and apply unified diffs
//   settings  persisted app config

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    let filter = tracing_subscriber::EnvFilter::try_from_env("PLUME_LOG")
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    tracing_subscriber::fmt().with_env_filter(filter).init();

    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![ping])
        .run(tauri::generate_context!())
        .expect("Plume failed to launch");
}

#[tauri::command]
fn ping() -> &'static str {
    "pong"
}
