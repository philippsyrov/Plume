use super::*;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::providers::catalog::{
    InstallReceipt, QWEN_CATALOG_ID, QWEN_REPORTED_BYTES, QWEN_REVISION,
};
use crate::providers::catalog_download::{
    remove_catalog_model, CatalogDownloadRegistry, DownloadError,
};

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(label: &str) -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock after Unix epoch")
            .as_nanos();
        let root = std::env::temp_dir()
            .canonicalize()
            .expect("canonicalize system temporary directory");
        let path = root.join(format!(
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

fn installed_qwen_store(base: &std::path::Path) -> crate::providers::catalog::CatalogStore {
    let store = crate::providers::catalog::CatalogStore::new(base.to_path_buf());
    let install_dir = store.qwen_install_dir();
    fs::create_dir_all(&install_dir).expect("create receipt-backed Qwen directory");
    let receipt = InstallReceipt {
        catalog_id: QWEN_CATALOG_ID.into(),
        revision: QWEN_REVISION.into(),
        manifest_sha256: store.expected_manifest_sha256(),
        installed_bytes: QWEN_REPORTED_BYTES,
        completed_at_ms: 1,
    };
    fs::write(
        install_dir.join("install-receipt.json"),
        serde_json::to_vec(&receipt).expect("serialize Qwen receipt"),
    )
    .expect("write Qwen receipt");
    store
}

#[test]
fn catalog_start_holds_the_lifecycle_gate_until_the_starter_reserves_the_verified_path() {
    let temp = TempDir::new("catalog-start-remove-race");
    let store = installed_qwen_store(temp.path());
    let registry = CatalogDownloadRegistry::default();
    let verified_path = store.qwen_install_dir();

    let observed_path = start_catalog_model_with(
        &store,
        &registry,
        QWEN_CATALOG_ID,
        |model_path, reservation| {
            assert_eq!(model_path, verified_path.as_path());
            assert!(
                model_path.is_dir(),
                "starter receives the receipt-verified directory"
            );
            assert!(matches!(
                remove_catalog_model(&registry, &store, QWEN_CATALOG_ID, || false),
                Err(DownloadError::OperationActive { .. })
            ));
            reservation.release_after_starting_reservation();
            Ok(model_path.to_path_buf())
        },
    )
    .expect("catalog start seam accepts the fixed installed model");

    assert_eq!(observed_path, verified_path);
    assert!(
        remove_catalog_model(&registry, &store, QWEN_CATALOG_ID, || false)
            .expect("release occurs before health polling")
            .removed,
        "the lifecycle gate is not held across later health polling"
    );
}

#[test]
fn catalog_start_needs_no_project_while_generic_start_keeps_the_approval_gate() {
    let temp = TempDir::new("catalog-start-no-project");
    let store = installed_qwen_store(temp.path());
    let registry = CatalogDownloadRegistry::default();

    let catalog_result = start_catalog_model_with(
        &store,
        &registry,
        QWEN_CATALOG_ID,
        |model_path, reservation| {
            reservation.release_after_starting_reservation();
            Ok(model_path.to_path_buf())
        },
    );

    assert!(matches!(
        catalog_result,
        Ok(path) if path == store.qwen_install_dir()
    ));
    assert!(matches!(
        generic_start_gate(None),
        Err(IpcError::NeedsApproval)
    ));
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
