// Tauri build script. Generates the runtime context (icons, ACL,
// etc.) consumed by `tauri::generate_context!()` at compile time.

fn main() {
    tauri_build::build();
}
