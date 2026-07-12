// Plume desktop entry point. The module tree and the Tauri builder
// live in the library crate (src/lib.rs) so additional binaries — the
// D129C plume_bench benchmark sidecar — can reuse the real product
// modules. This file only forwards to it.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    plume::run();
}
