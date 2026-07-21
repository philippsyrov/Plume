use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use super::apple_foundation::{AppleAvailability, AppleAvailabilityReason};
use super::catalog::{
    apply_apple_availability, CatalogState, CatalogStore, InstallReceipt, QWEN2_VL_CATALOG_ID,
    QWEN2_VL_REPORTED_BYTES, QWEN2_VL_REVISION, QWEN_CATALOG_ID, QWEN_REPORTED_BYTES,
    QWEN_REVISION,
};
use super::catalog_download::DownloadManifest;

static TEMP_DIR_SEQUENCE: AtomicUsize = AtomicUsize::new(0);
const RECEIPT_CAP_BYTES: usize = 16 * 1024;

struct TestDir {
    path: PathBuf,
}

impl TestDir {
    fn new() -> Self {
        let sequence = TEMP_DIR_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir()
            .canonicalize()
            .expect("canonicalize system temporary directory");
        let path = root.join(format!(
            "plume-catalog-test-{}-{sequence}",
            std::process::id()
        ));
        fs::create_dir_all(&path).expect("create isolated catalog test directory");
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn qwen_entry(store: &CatalogStore) -> super::catalog::CatalogEntry {
    store
        .list()
        .expect("list fixed catalog")
        .into_iter()
        .find(|entry| entry.id == QWEN_CATALOG_ID)
        .expect("Qwen entry must exist")
}

fn write_receipt(store: &CatalogStore, receipt: &InstallReceipt) {
    write_receipt_bytes(
        store,
        &serde_json::to_vec(receipt).expect("serialize receipt"),
    );
}

fn write_receipt_bytes(store: &CatalogStore, bytes: &[u8]) {
    let install_dir = store.qwen_install_dir();
    fs::create_dir_all(&install_dir).expect("create Qwen install directory");
    fs::write(install_dir.join("install-receipt.json"), bytes).expect("write receipt");
}

fn valid_receipt(store: &CatalogStore) -> InstallReceipt {
    InstallReceipt {
        catalog_id: QWEN_CATALOG_ID.into(),
        revision: QWEN_REVISION.into(),
        manifest_sha256: store
            .expected_receipt_manifest_sha256(QWEN_CATALOG_ID)
            .expect("fixed Qwen receipt identity"),
        installed_bytes: QWEN_REPORTED_BYTES,
        completed_at_ms: 1,
    }
}

#[test]
fn catalog_is_fixed_and_qwen_install_lives_under_app_data() {
    let temp = TestDir::new();
    let store = CatalogStore::new(temp.path().to_path_buf());
    let entries = store.list().expect("list fixed catalog");

    assert_eq!(
        entries
            .iter()
            .map(|entry| entry.id.as_str())
            .collect::<Vec<_>>(),
        [
            "apple-system",
            "qwen-coder-1.5b-mlx-4bit",
            "qwen2-vl-2b-instruct-4bit"
        ]
    );
    let qwen = entries
        .iter()
        .find(|entry| entry.id == QWEN_CATALOG_ID)
        .expect("Qwen entry must exist");
    let apple = entries
        .iter()
        .find(|entry| entry.id == "apple-system")
        .expect("Apple entry must exist");
    assert_eq!(apple.display_name, "Apple On-Device");
    assert_eq!(apple.subtitle, "Built into this Mac");
    assert_eq!(apple.provider_id, "apple-foundation");
    assert_eq!(apple.model_id, "system");
    assert_eq!(qwen.display_name, "Qwen Coder 1.5B");
    assert_eq!(qwen.subtitle, "Recommended for coding");
    assert_eq!(qwen.revision.as_deref(), Some(QWEN_REVISION));
    assert_eq!(qwen.license, "Apache-2.0");
    assert_eq!(qwen.download_bytes, Some(QWEN_REPORTED_BYTES));
    assert!(store.qwen_install_dir().starts_with(temp.path()));
    let qwen2_vl = entries
        .iter()
        .find(|entry| entry.id == QWEN2_VL_CATALOG_ID)
        .expect("Qwen2-VL entry must exist");
    assert_eq!(qwen2_vl.display_name, "Qwen2-VL 2B");
    assert_eq!(qwen2_vl.provider_id, "mlx-vlm");
    assert_eq!(qwen2_vl.revision.as_deref(), Some(QWEN2_VL_REVISION));
    assert_eq!(qwen2_vl.download_bytes, Some(QWEN2_VL_REPORTED_BYTES));
    assert_eq!(qwen2_vl.license, "Apache-2.0");
    assert!(store.qwen2_vl_install_dir().starts_with(temp.path()));
}

#[test]
fn qwen2_vl_catalog_download_bytes_match_the_fixed_manifest_total() {
    let temp = TestDir::new();
    let store = CatalogStore::new(temp.path().to_path_buf());
    let entry = store
        .list()
        .expect("list fixed catalog")
        .into_iter()
        .find(|entry| entry.id == QWEN2_VL_CATALOG_ID)
        .expect("Qwen2-VL entry must exist");
    let manifest =
        DownloadManifest::fixed_for(QWEN2_VL_CATALOG_ID).expect("fixed Qwen2-VL manifest parses");

    assert_eq!(entry.download_bytes, Some(manifest.total_bytes));
}

#[test]
fn apple_availability_updates_only_the_fixed_apple_catalog_row() {
    let temp = TestDir::new();
    let store = CatalogStore::new(temp.path().to_path_buf());
    let mut entries = store.list().expect("list fixed catalog");
    apply_apple_availability(
        &mut entries,
        &AppleAvailability {
            available: false,
            reason: Some(AppleAvailabilityReason::ModelNotReady),
            detail: None,
        },
    );

    let apple = entries
        .iter()
        .find(|entry| entry.id == "apple-system")
        .expect("Apple entry must exist");
    let qwen = entries
        .iter()
        .find(|entry| entry.id == QWEN_CATALOG_ID)
        .expect("Qwen entry must exist");
    assert_eq!(apple.state, CatalogState::Unavailable);
    assert_eq!(
        apple.availability_reason.as_deref(),
        Some("The Apple on-device model is not ready yet.")
    );
    assert_eq!(qwen.state, CatalogState::Absent);
}

#[test]
fn matching_receipt_marks_qwen_installed() {
    let temp = TestDir::new();
    let store = CatalogStore::new(temp.path().to_path_buf());
    write_receipt(&store, &valid_receipt(&store));

    assert_eq!(qwen_entry(&store).state, CatalogState::Installed);
}

#[test]
fn matching_qwen2_vl_receipt_marks_the_fixed_vision_model_installed() {
    let temp = TestDir::new();
    let store = CatalogStore::new(temp.path().to_path_buf());
    let receipt = InstallReceipt {
        catalog_id: QWEN2_VL_CATALOG_ID.into(),
        revision: QWEN2_VL_REVISION.into(),
        manifest_sha256: store
            .expected_receipt_manifest_sha256(QWEN2_VL_CATALOG_ID)
            .expect("fixed Qwen2-VL receipt identity"),
        installed_bytes: QWEN2_VL_REPORTED_BYTES,
        completed_at_ms: 1,
    };
    let install_dir = store.qwen2_vl_install_dir();
    fs::create_dir_all(&install_dir).expect("create Qwen2-VL install directory");
    fs::write(
        install_dir.join("install-receipt.json"),
        serde_json::to_vec(&receipt).expect("serialize Qwen2-VL receipt"),
    )
    .expect("write Qwen2-VL receipt");

    assert_eq!(
        store.installed_model_path(QWEN2_VL_CATALOG_ID),
        Some(install_dir)
    );
}

#[test]
fn installed_model_path_requires_the_fixed_receipt_and_a_real_directory() {
    let temp = TestDir::new();
    let store = CatalogStore::new(temp.path().to_path_buf());

    assert!(store.installed_model_path(QWEN_CATALOG_ID).is_none());
    write_receipt(&store, &valid_receipt(&store));

    assert_eq!(
        store.installed_model_path(QWEN_CATALOG_ID),
        Some(store.qwen_install_dir()),
        "only the receipt-backed fixed Qwen directory is launchable"
    );
    assert!(store.installed_model_path("apple-system").is_none());
}

#[cfg(unix)]
#[test]
fn installed_model_path_rejects_a_symlinked_install_directory() {
    use std::os::unix::fs::symlink;

    let temp = TestDir::new();
    let store = CatalogStore::new(temp.path().to_path_buf());
    write_receipt(&store, &valid_receipt(&store));
    let install = store.qwen_install_dir();
    let target = temp.path().join("install-target");
    fs::rename(&install, &target).expect("move valid install aside");
    symlink(&target, &install).expect("replace install with symlink");

    assert!(
        store.installed_model_path(QWEN_CATALOG_ID).is_none(),
        "a catalog receipt never authorizes a symlinked model directory"
    );
}

#[test]
fn mismatched_receipt_never_marks_qwen_installed() {
    let temp = TestDir::new();
    let store = CatalogStore::new(temp.path().to_path_buf());
    let cases = [
        InstallReceipt {
            catalog_id: "wrong-catalog-id".into(),
            ..valid_receipt(&store)
        },
        InstallReceipt {
            revision: "wrong-revision".into(),
            ..valid_receipt(&store)
        },
        InstallReceipt {
            manifest_sha256: "wrong-manifest-digest".into(),
            ..valid_receipt(&store)
        },
    ];

    for receipt in cases {
        write_receipt(&store, &receipt);
        assert_eq!(qwen_entry(&store).state, CatalogState::Absent);
    }
}

#[test]
fn malformed_or_oversized_receipts_never_mark_qwen_installed() {
    let temp = TestDir::new();
    let store = CatalogStore::new(temp.path().to_path_buf());
    write_receipt_bytes(&store, b"not valid JSON");
    assert_eq!(qwen_entry(&store).state, CatalogState::Absent);

    let mut oversized = serde_json::to_vec(&valid_receipt(&store)).expect("serialize receipt");
    oversized.resize(RECEIPT_CAP_BYTES + 1, b' ');
    write_receipt_bytes(&store, &oversized);
    assert_eq!(qwen_entry(&store).state, CatalogState::Absent);
}

#[test]
fn receipt_at_exact_size_cap_can_mark_qwen_installed() {
    let temp = TestDir::new();
    let store = CatalogStore::new(temp.path().to_path_buf());
    let mut receipt = serde_json::to_vec(&valid_receipt(&store)).expect("serialize receipt");
    receipt.resize(RECEIPT_CAP_BYTES, b' ');
    write_receipt_bytes(&store, &receipt);

    assert_eq!(qwen_entry(&store).state, CatalogState::Installed);
}

#[cfg(unix)]
#[test]
fn symlinked_receipt_never_marks_qwen_installed() {
    use std::os::unix::fs::symlink;

    let temp = TestDir::new();
    let store = CatalogStore::new(temp.path().to_path_buf());
    let receipt_target = temp.path().join("receipt-target.json");
    fs::write(
        &receipt_target,
        serde_json::to_vec(&valid_receipt(&store)).expect("serialize receipt"),
    )
    .expect("write receipt target");
    fs::create_dir_all(store.qwen_install_dir()).expect("create Qwen install directory");
    symlink(
        &receipt_target,
        store.qwen_install_dir().join("install-receipt.json"),
    )
    .expect("symlink receipt");

    assert_eq!(qwen_entry(&store).state, CatalogState::Absent);
}

#[cfg(unix)]
#[test]
fn symlinked_intermediate_directory_never_marks_qwen_installed() {
    use std::os::unix::fs::symlink;

    let temp = TestDir::new();
    let store = CatalogStore::new(temp.path().to_path_buf());
    let redirected = temp.path().join("redirected-models");
    let redirected_store = CatalogStore::new(temp.path().join("redirected-app-data"));
    write_receipt(&redirected_store, &valid_receipt(&redirected_store));
    fs::rename(
        redirected_store
            .qwen_install_dir()
            .parent()
            .expect("catalog id parent")
            .parent()
            .expect("catalog parent")
            .parent()
            .expect("models parent"),
        &redirected,
    )
    .expect("place redirected catalog directory");
    symlink(&redirected, temp.path().join("models")).expect("symlink models directory");
    let followed_receipt = temp
        .path()
        .join("models")
        .join("catalog")
        .join(QWEN_CATALOG_ID)
        .join(QWEN_REVISION)
        .join("install-receipt.json");
    let followed_receipt: InstallReceipt = serde_json::from_slice(
        &fs::read(&followed_receipt).expect("following models symlink reaches receipt"),
    )
    .expect("followed receipt remains valid JSON");
    assert_eq!(followed_receipt.catalog_id, QWEN_CATALOG_ID);

    assert_eq!(qwen_entry(&store).state, CatalogState::Absent);
}

#[cfg(unix)]
#[test]
fn opened_model_directory_cannot_be_redirected_before_receipt_read() {
    use std::os::unix::fs::symlink;

    let temp = TestDir::new();
    let store = CatalogStore::new(temp.path().to_path_buf());
    write_receipt(&store, &valid_receipt(&store));
    let attacker = temp.path().join("attacker");
    fs::create_dir_all(attacker.join("catalog")).expect("create attacker catalog directory");
    let original_models = temp.path().join("models-original");
    let models = temp.path().join("models");

    assert!(store.qwen_receipt_is_valid_with_hook(|opened_name| {
        if opened_name == "models" {
            fs::rename(&models, &original_models).expect("move opened models directory");
            symlink(&attacker, &models).expect("redirect models path after open");
        }
    }));
}
