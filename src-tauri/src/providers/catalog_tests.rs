use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use super::catalog::{
    CatalogState, CatalogStore, InstallReceipt, QWEN_CATALOG_ID, QWEN_REPORTED_BYTES, QWEN_REVISION,
};

static TEMP_DIR_SEQUENCE: AtomicUsize = AtomicUsize::new(0);

struct TestDir {
    path: PathBuf,
}

impl TestDir {
    fn new() -> Self {
        let sequence = TEMP_DIR_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
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
    let install_dir = store.qwen_install_dir();
    fs::create_dir_all(&install_dir).expect("create Qwen install directory");
    fs::write(
        install_dir.join("install-receipt.json"),
        serde_json::to_vec(receipt).expect("serialize receipt"),
    )
    .expect("write receipt");
}

fn valid_receipt(store: &CatalogStore) -> InstallReceipt {
    InstallReceipt {
        catalog_id: QWEN_CATALOG_ID.into(),
        revision: QWEN_REVISION.into(),
        manifest_sha256: store.expected_manifest_sha256(),
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
        ["apple-system", "qwen-coder-1.5b-mlx-4bit"]
    );
    let qwen = entries
        .iter()
        .find(|entry| entry.id == QWEN_CATALOG_ID)
        .expect("Qwen entry must exist");
    assert_eq!(qwen.revision.as_deref(), Some(QWEN_REVISION));
    assert_eq!(qwen.license, "Apache-2.0");
    assert_eq!(qwen.download_bytes, Some(QWEN_REPORTED_BYTES));
    assert!(store.qwen_install_dir().starts_with(temp.path()));
}

#[test]
fn matching_receipt_marks_qwen_installed() {
    let temp = TestDir::new();
    let store = CatalogStore::new(temp.path().to_path_buf());
    write_receipt(&store, &valid_receipt(&store));

    assert_eq!(qwen_entry(&store).state, CatalogState::Installed);
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
