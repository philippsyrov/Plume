// Tauri build script. Generates the runtime context (icons, ACL,
// etc.) consumed by `tauri::generate_context!()` at compile time.
//
// D129C: also embeds the BUILD-TIME git identity (commit sha + dirty
// state) as rustc env vars. The `plume_bench` sidecar reports these
// over its identity handshake so the benchmark harness can refuse a
// stale or foreign binary before any record is labeled with the
// checkout's Plume identity.

use std::process::Command;

#[path = "src/app_commands.rs"]
mod app_commands;

fn main() {
    emit_build_identity();
    let manifest = tauri_build::AppManifest::new().commands(app_commands::APP_COMMANDS);
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(manifest))
        .expect("failed to build Tauri application manifest");
}

fn git_value(args: &[&str]) -> Option<String> {
    let output = Command::new("git").args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn emit_build_identity() {
    // Force this script to rerun on EVERY cargo invocation: watching a
    // path that never exists means the embedded identity can never go
    // stale relative to the build that carries it (a commit or a
    // working-tree change between builds is always picked up).
    println!("cargo:rerun-if-changed=.plume-build-identity-always-rerun");

    let sha = git_value(&["rev-parse", "HEAD"]).unwrap_or_else(|| "unknown".to_string());
    let dirty = match git_value(&["status", "--porcelain"]) {
        Some(status) => {
            if status.is_empty() {
                "false"
            } else {
                "true"
            }
        }
        None => "unknown",
    };
    println!("cargo:rustc-env=PLUME_BUILD_GIT_SHA={sha}");
    println!("cargo:rustc-env=PLUME_BUILD_DIRTY={dirty}");
}
