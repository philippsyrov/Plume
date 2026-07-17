use super::*;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(label: &str) -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock after Unix epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "plume-providers-cmd-{label}-{}-{nanos}",
            std::process::id(),
        ));
        fs::create_dir_all(&path).expect("create isolated command test directory");
        Self { path }
    }

    fn path(&self) -> &std::path::Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

#[test]
fn scan_does_not_surface_stray_non_model_files() {
    let td = TempDir::new("stray-readme");
    fs::write(td.path().join("README.md"), b"# notes").expect("write readme");
    fs::write(td.path().join("notes.txt"), b"todo").expect("write note");
    let inventory = local_models::scan_model_dir(td.path());
    assert!(
        inventory.is_empty(),
        "stray non-model files must NOT be in the inventory; got {inventory:?}"
    );
}

#[test]
fn handler_gate_rejects_id_absent_from_inventory() {
    let td = TempDir::new("absent-id");
    fs::write(td.path().join("tiny.gguf"), b"fake gguf").expect("write model fixture");
    let inventory = local_models::scan_model_dir(td.path());
    assert_eq!(inventory.len(), 1);
    assert!(inventory
        .iter()
        .any(|model| model.id == "plume-model-dir:tiny.gguf"));
    assert!(!inventory
        .iter()
        .any(|model| model.id == "plume-model-dir:README.md"));
    assert!(!inventory
        .iter()
        .any(|model| model.id == "plume-model-dir:subdir/model"));
    assert!(!inventory.iter().any(|model| model.id == "tiny.gguf"));
}

#[test]
fn handler_gate_accepts_id_present_in_inventory() {
    let td = TempDir::new("present-id");
    fs::write(td.path().join("tiny.gguf"), b"fake gguf").expect("write model fixture");
    let inventory = local_models::scan_model_dir(td.path());
    assert!(inventory
        .iter()
        .any(|model| model.id == "plume-model-dir:tiny.gguf"));
}

#[test]
fn d50_resolver_treats_unknown_source_prefix_as_stale() {
    assert!(local_models::parse_inventory_id("imaginary-source:foo").is_none());
    assert!(local_models::parse_inventory_id("plume-model-dir:foo").is_some());
    assert!(local_models::parse_inventory_id("locally-ai-cache:foo").is_some());
    assert!(local_models::parse_inventory_id("lm-studio-cache:foo").is_some());
}

#[test]
fn list_servers_response_serializes_camel_case_fields() {
    let response = ListServersResponse {
        servers: vec![ManagedServerInfo {
            handle_id: "srv_0000000000000001".into(),
            port: 4242,
            pid: 999,
            model_id: "plume-model-dir:qwen".into(),
            model_label: "/models/qwen".into(),
            started_at_ms: 1_700_000_000_000,
            uptime_ms: 5_000,
        }],
    };
    let json = serde_json::to_value(&response).expect("serialize");
    let server = &json["servers"][0];
    assert_eq!(server["handleId"], "srv_0000000000000001");
    assert_eq!(server["port"], 4242);
    assert_eq!(server["pid"], 999);
    assert_eq!(server["modelId"], "plume-model-dir:qwen");
    assert_eq!(server["modelLabel"], "/models/qwen");
    assert_eq!(server["startedAtMs"], 1_700_000_000_000u64);
    assert_eq!(server["uptimeMs"], 5_000);
}
