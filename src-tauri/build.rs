// Tauri build script. Generates the runtime context (icons, ACL,
// etc.) consumed by `tauri::generate_context!()` at compile time.
//
// D129C: also embeds the BUILD-TIME git identity (commit sha + dirty
// state) as rustc env vars. The `plume_bench` sidecar reports these
// over its identity handshake so the benchmark harness can refuse a
// stale or foreign binary before any record is labeled with the
// checkout's Plume identity.

use std::{fs, path::PathBuf, process::Command};

#[path = "src/app_commands.rs"]
mod app_commands;

fn main() {
    emit_build_identity();
    ensure_bundle_resource_dirs();
    let manifest = tauri_build::AppManifest::new().commands(app_commands::APP_COMMANDS);
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(manifest))
        .expect("failed to build Tauri application manifest");
}

fn ensure_bundle_resource_dirs() {
    // `std::env::var`, never `env!`. `env!` reads the variable when this build
    // script is COMPILED and bakes the answer into the binary — and that binary
    // lives in CARGO_TARGET_DIR, not in the checkout. Two checkouts sharing one
    // target dir share the binary whenever their build-script sources match,
    // because the fingerprint is over source content rather than over the
    // manifest path. The second checkout would then create these directories
    // under the first one's path, while `tauri_build::try_build` resolves the
    // same resources against the real manifest dir and fails the build.
    let manifest_dir = PathBuf::from(
        std::env::var("CARGO_MANIFEST_DIR")
            .expect("cargo sets CARGO_MANIFEST_DIR for build scripts"),
    );
    for resource in [
        "runtime/generated/mlx-runtime",
        "runtime/generated/apple-model",
    ] {
        fs::create_dir_all(manifest_dir.join(resource))
            .expect("failed to create an empty generated bundle resource directory");
    }
}

fn git_value(args: &[&str]) -> Option<String> {
    // Inherits this process's working directory, which cargo sets to the
    // package root for every build-script run. That is resolved per invocation,
    // so unlike the trap above it stays correct when a cached build-script
    // binary is reused across checkouts.
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
