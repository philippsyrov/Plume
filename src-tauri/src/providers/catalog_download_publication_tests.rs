use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

use serde_json::json;
use sha2::{Digest, Sha256};

use super::catalog::{CatalogStore, QWEN_CATALOG_ID, QWEN_REVISION};
use super::catalog_download::{
    with_publication_hook_for_test, CatalogDownloadEvent, CatalogDownloadManager,
    CatalogDownloadRegistry, DownloadError, DownloadEventSink, DownloadFetcher, DownloadManifest,
    DownloadPhase, DownloadRequest, DownloadResponse,
};

static TEMP_DIR_SEQUENCE: AtomicUsize = AtomicUsize::new(0);

struct TestDir(PathBuf);

impl TestDir {
    fn new() -> Self {
        let sequence = TEMP_DIR_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir()
            .canonicalize()
            .expect("canonicalize system temporary directory");
        let path = root.join(format!(
            "plume-catalog-publication-{}-{sequence}",
            std::process::id()
        ));
        fs::create_dir_all(&path).expect("create isolated test directory");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[derive(Clone)]
struct FixtureFetcher(BTreeMap<String, Vec<u8>>);

impl DownloadFetcher for FixtureFetcher {
    fn fetch(&self, request: &DownloadRequest) -> Result<DownloadResponse, DownloadError> {
        let bytes = self.0.get(&request.path).expect("fixture contains path");
        let start = request.range_start.expect("bounded range start") as usize;
        let end = request.range_end.expect("bounded range end") as usize;
        Ok(DownloadResponse::from_bytes(
            206,
            Some(format!("bytes {start}-{end}/{}", bytes.len())),
            Vec::new(),
            bytes[start..=end].to_vec(),
        ))
    }
}

#[derive(Clone, Copy)]
struct NoopEvents;

impl DownloadEventSink for NoopEvents {
    fn emit(&self, _: CatalogDownloadEvent) {}
}

#[derive(Clone)]
struct CancelAtVerification {
    cancel: Arc<AtomicBool>,
}

impl DownloadEventSink for CancelAtVerification {
    fn emit(&self, event: CatalogDownloadEvent) {
        if event.phase == DownloadPhase::Verifying {
            self.cancel.store(true, Ordering::Release);
        }
    }
}

struct Fixture {
    _dir: TestDir,
    store: Arc<CatalogStore>,
    manifest: DownloadManifest,
    fetcher: FixtureFetcher,
    registry: CatalogDownloadRegistry,
}

impl Fixture {
    fn new() -> Self {
        let dir = TestDir::new();
        let files = BTreeMap::from([
            ("README.md".to_string(), b"tiny readme".to_vec()),
            (
                "config.json".to_string(),
                b"{\"model_type\":\"qwen2\"}".to_vec(),
            ),
            ("model.safetensors".to_string(), b"tiny-model".to_vec()),
        ]);
        let entries = files
            .iter()
            .map(|(path, bytes)| {
                json!({
                    "path": path,
                    "size": bytes.len(),
                    "sha256": format!("{:x}", Sha256::digest(bytes)),
                })
            })
            .collect::<Vec<_>>();
        let manifest = DownloadManifest::parse_json(
            &json!({
                "catalogId": QWEN_CATALOG_ID,
                "repo": "mlx-community/Qwen2.5-Coder-1.5B-Instruct-4bit",
                "revision": QWEN_REVISION,
                "license": "Apache-2.0",
                "weightBytes": files["model.safetensors"].len(),
                "totalBytes": files.values().map(|bytes| bytes.len() as u64).sum::<u64>(),
                "files": entries,
            })
            .to_string(),
        )
        .expect("fixture manifest is valid");
        Self {
            store: Arc::new(CatalogStore::new(dir.path().to_path_buf())),
            manifest,
            fetcher: FixtureFetcher(files),
            registry: CatalogDownloadRegistry::default(),
            _dir: dir,
        }
    }

    fn manager<E: DownloadEventSink>(
        &self,
        events: E,
    ) -> CatalogDownloadManager<FixtureFetcher, E> {
        CatalogDownloadManager::new(
            self.store.clone(),
            self.manifest.clone(),
            self.fetcher.clone(),
            events,
        )
    }

    fn prepared_dir(&self) -> PathBuf {
        self.store
            .staging_dir()
            .parent()
            .expect("catalog-id directory")
            .join(format!(".{QWEN_REVISION}.prepared"))
    }
}

#[cfg(unix)]
#[test]
fn raced_hardlink_before_output_validation_never_receives_download_bytes() {
    let fixture = Fixture::new();
    let prepared = fixture.prepared_dir();
    let outside = fixture._dir.path().join("outside-hardlink");
    let _hook = with_publication_hook_for_test({
        let prepared = prepared.clone();
        let outside = outside.clone();
        move |point| {
            if point == "before-output-validation:README.md" {
                fs::hard_link(prepared.join("README.md"), &outside).expect("race hardlink");
            }
        }
    });
    let operation = fixture
        .registry
        .begin_download_for_store(&fixture.store, QWEN_CATALOG_ID)
        .expect("begin download");
    let result = fixture.manager(NoopEvents).run(QWEN_CATALOG_ID, &operation);
    fixture.registry.finish(&operation);

    assert!(matches!(
        result,
        Err(DownloadError::UnexpectedStagingPath { .. })
    ));
    assert_eq!(
        fs::metadata(outside)
            .expect("outside hardlink exists")
            .len(),
        0
    );
    assert!(!fixture.store.qwen_install_dir().exists());
}

#[test]
fn preexisting_prepared_output_is_never_reopened_after_exclusive_create_fails() {
    let fixture = Fixture::new();
    let prepared = fixture.prepared_dir();
    let _hook = with_publication_hook_for_test(move |point| {
        if point == "before-output-create:README.md" {
            fs::write(prepared.join("README.md"), b"planted-before-create")
                .expect("plant output before exclusive create");
        }
    });
    let operation = fixture
        .registry
        .begin_download_for_store(&fixture.store, QWEN_CATALOG_ID)
        .expect("begin download");
    let result = fixture.manager(NoopEvents).run(QWEN_CATALOG_ID, &operation);
    fixture.registry.finish(&operation);

    assert!(matches!(result, Err(DownloadError::AlreadyExists { .. })));
    assert_eq!(
        fs::read(fixture.prepared_dir().join("README.md")).expect("planted output survives"),
        b"planted-before-create"
    );
    assert!(!fixture.store.qwen_install_dir().exists());
}

#[test]
fn replaced_output_after_hash_is_rejected_before_the_directory_publish() {
    let fixture = Fixture::new();
    let prepared = fixture.prepared_dir();
    let _hook = with_publication_hook_for_test(move |point| {
        if point == "before-publish-validation" {
            fs::remove_file(prepared.join("README.md")).expect("replace verified output");
            fs::write(prepared.join("README.md"), b"attacker replacement")
                .expect("write replacement");
        }
    });
    let operation = fixture
        .registry
        .begin_download_for_store(&fixture.store, QWEN_CATALOG_ID)
        .expect("begin download");
    let result = fixture.manager(NoopEvents).run(QWEN_CATALOG_ID, &operation);
    fixture.registry.finish(&operation);

    assert!(matches!(result, Err(DownloadError::PathSwap { .. })));
    assert!(!fixture.store.qwen_install_dir().exists());
}

#[test]
fn unexpected_prepared_entry_after_hash_is_rejected_before_the_directory_publish() {
    let fixture = Fixture::new();
    let prepared = fixture.prepared_dir();
    let _hook = with_publication_hook_for_test(move |point| {
        if point == "before-publish-validation" {
            fs::write(prepared.join("attacker-extra"), b"unexpected").expect("add extra output");
        }
    });
    let operation = fixture
        .registry
        .begin_download_for_store(&fixture.store, QWEN_CATALOG_ID)
        .expect("begin download");
    let result = fixture.manager(NoopEvents).run(QWEN_CATALOG_ID, &operation);
    fixture.registry.finish(&operation);

    assert!(matches!(
        result,
        Err(DownloadError::UnexpectedStagingPath { .. })
    ));
    assert!(!fixture.store.qwen_install_dir().exists());
}

#[test]
fn cancellation_after_verifying_event_preserves_parts_and_never_publishes() {
    let fixture = Fixture::new();
    let operation = fixture
        .registry
        .begin_download_for_store(&fixture.store, QWEN_CATALOG_ID)
        .expect("begin download");
    let manager = fixture.manager(CancelAtVerification {
        cancel: operation.cancel.clone(),
    });
    let result = manager.run(QWEN_CATALOG_ID, &operation);
    fixture.registry.finish(&operation);
    manager.emit_terminal(&operation, &result);

    assert!(matches!(result, Err(DownloadError::Cancelled)));
    assert!(fixture.store.staging_dir().join("README.md.part").is_file());
    assert!(!fixture.prepared_dir().exists());
    assert!(!fixture.store.qwen_install_dir().exists());
}
