use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use serde_json::json;
use sha2::{Digest, Sha256};

use super::catalog::{CatalogState, CatalogStore, QWEN_CATALOG_ID, QWEN_REVISION};
use super::catalog_download::{
    allowed_download_host, remove_catalog_model, CatalogDownloadEvent, CatalogDownloadManager,
    CatalogDownloadRegistry, DownloadError, DownloadEventSink, DownloadFetcher, DownloadManifest,
    DownloadPhase, DownloadRequest, DownloadResponse,
};

static TEMP_DIR_SEQUENCE: AtomicUsize = AtomicUsize::new(0);

struct TestDir {
    path: PathBuf,
}

impl TestDir {
    fn new(label: &str) -> Self {
        let sequence = TEMP_DIR_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir()
            .canonicalize()
            .expect("canonicalize system temporary directory");
        let path = root.join(format!(
            "plume-catalog-download-{label}-{}-{sequence}",
            std::process::id()
        ));
        fs::create_dir_all(&path).expect("create isolated test directory");
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

#[derive(Clone)]
struct FakePlan {
    bytes: Vec<u8>,
    redirects: Vec<String>,
    content_range: Option<String>,
    cancel_after_request: Option<Arc<AtomicBool>>,
}

#[derive(Clone, Default)]
struct FakeFetcher {
    plans: Arc<Mutex<BTreeMap<String, FakePlan>>>,
    requests: Arc<Mutex<Vec<DownloadRequest>>>,
}

impl FakeFetcher {
    fn with_files(files: &BTreeMap<String, Vec<u8>>) -> Self {
        let plans = files
            .iter()
            .map(|(path, bytes)| {
                (
                    path.clone(),
                    FakePlan {
                        bytes: bytes.clone(),
                        redirects: Vec::new(),
                        content_range: None,
                        cancel_after_request: None,
                    },
                )
            })
            .collect();
        Self {
            plans: Arc::new(Mutex::new(plans)),
            requests: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn replace_plan(&self, path: &str, plan: FakePlan) {
        self.plans
            .lock()
            .expect("plans mutex")
            .insert(path.into(), plan);
    }

    fn requests(&self) -> Vec<DownloadRequest> {
        self.requests.lock().expect("requests mutex").clone()
    }
}

impl DownloadFetcher for FakeFetcher {
    fn fetch(&self, request: &DownloadRequest) -> Result<DownloadResponse, DownloadError> {
        self.requests
            .lock()
            .expect("requests mutex")
            .push(request.clone());
        let plan = self
            .plans
            .lock()
            .expect("plans mutex")
            .get(&request.path)
            .cloned()
            .expect("fixture contains requested path");
        if let Some(cancel) = plan.cancel_after_request {
            cancel.store(true, Ordering::Release);
        }
        let start = request.range_start.unwrap_or(0) as usize;
        let status = if start == 0 { 200 } else { 206 };
        let content_range = plan.content_range.or_else(|| {
            (start > 0).then(|| {
                format!(
                    "bytes {start}-{}/{}",
                    plan.bytes.len() - 1,
                    plan.bytes.len()
                )
            })
        });
        Ok(DownloadResponse::from_bytes(
            status,
            content_range,
            plan.redirects,
            plan.bytes[start..].to_vec(),
        ))
    }
}

#[derive(Clone, Default)]
struct EventRecorder(Arc<Mutex<Vec<CatalogDownloadEvent>>>);

impl EventRecorder {
    fn events(&self) -> Vec<CatalogDownloadEvent> {
        self.0.lock().expect("events mutex").clone()
    }
}

impl DownloadEventSink for EventRecorder {
    fn emit(&self, event: CatalogDownloadEvent) {
        self.0.lock().expect("events mutex").push(event);
    }
}

struct DownloadFixture {
    _dir: TestDir,
    store: Arc<CatalogStore>,
    manifest: DownloadManifest,
    fetcher: FakeFetcher,
    events: EventRecorder,
    registry: CatalogDownloadRegistry,
}

impl DownloadFixture {
    fn matching_manifest() -> Self {
        let dir = TestDir::new("matching");
        let files = BTreeMap::from([
            ("README.md".to_string(), b"tiny readme".to_vec()),
            (
                "config.json".to_string(),
                b"{\"model_type\":\"qwen2\"}".to_vec(),
            ),
            ("model.safetensors".to_string(), b"tiny-model".to_vec()),
        ]);
        let manifest = manifest_for(&files);
        Self {
            store: Arc::new(CatalogStore::new(dir.path().to_path_buf())),
            fetcher: FakeFetcher::with_files(&files),
            events: EventRecorder::default(),
            registry: CatalogDownloadRegistry::default(),
            manifest,
            _dir: dir,
        }
    }

    fn manager(&self) -> CatalogDownloadManager<FakeFetcher, EventRecorder> {
        CatalogDownloadManager::new(
            self.store.clone(),
            self.manifest.clone(),
            self.fetcher.clone(),
            self.events.clone(),
        )
    }

    fn run(&self) -> Result<super::catalog_download::DownloadResult, DownloadError> {
        let operation = self.registry.begin_download(QWEN_CATALOG_ID)?;
        let result = self.manager().run(QWEN_CATALOG_ID, &operation);
        self.registry.finish(&operation);
        result
    }

    fn expected_bytes(&self) -> u64 {
        self.manifest.total_bytes
    }
}

fn manifest_for(files: &BTreeMap<String, Vec<u8>>) -> DownloadManifest {
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
    DownloadManifest::parse_json(
        &json!({
            "catalogId": QWEN_CATALOG_ID,
            "repo": "mlx-community/Qwen2.5-Coder-1.5B-Instruct-4bit",
            "revision": QWEN_REVISION,
            "license": "Apache-2.0",
            "weightBytes": files.get("model.safetensors").expect("fixture weight").len(),
            "totalBytes": files.values().map(|bytes| bytes.len() as u64).sum::<u64>(),
            "files": entries,
        })
        .to_string(),
    )
    .expect("fixture manifest must be valid")
}

fn qwen_state(store: &CatalogStore) -> CatalogState {
    store
        .list()
        .expect("catalog lists")
        .into_iter()
        .find(|entry| entry.id == QWEN_CATALOG_ID)
        .expect("Qwen entry")
        .state
}

#[test]
fn fixed_manifest_matches_the_verified_pinned_qwen_identity() {
    let manifest = DownloadManifest::fixed().expect("fixed manifest parses");
    assert_eq!(manifest.catalog_id, QWEN_CATALOG_ID);
    assert_eq!(manifest.revision, QWEN_REVISION);
    assert_eq!(manifest.total_bytes, 880_170_581);
    assert_eq!(manifest.files.len(), 10);
    assert!(manifest
        .files
        .iter()
        .any(|file| file.path == "model.safetensors" && file.size == 868_628_559));
}

#[test]
fn manifest_rejects_zero_duplicate_unsafe_hash_total_catalog_and_revision_drift() {
    let fixture = DownloadFixture::matching_manifest();
    let cases = [
        json!({"files": [{"path":"README.md", "size":0, "sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}]}),
        json!({"files": [{"path":"README.md", "size":1, "sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}, {"path":"README.md", "size":1, "sha256":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"}]}),
        json!({"files": [{"path":"../escape", "size":1, "sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}]}),
        json!({"files": [{"path":"README.md", "size":1, "sha256":"short"}]}),
        json!({"totalBytes": 99}),
        json!({"catalogId":"wrong-catalog"}),
        json!({"revision":"mutable-main"}),
    ];
    let baseline = serde_json::to_value(&fixture.manifest).expect("serialize baseline");
    for patch in cases {
        let mut value = baseline.clone();
        for (key, replacement) in patch.as_object().expect("object patch") {
            value[key] = replacement.clone();
        }
        assert!(
            DownloadManifest::parse_json(&value.to_string()).is_err(),
            "{value}"
        );
    }
}

#[test]
fn verified_download_installs_atomically_and_writes_receipt() {
    let fixture = DownloadFixture::matching_manifest();
    let result = fixture.run().expect("verified download succeeds");
    assert_eq!(result.installed_bytes, fixture.expected_bytes());
    assert!(fixture
        .store
        .qwen_install_dir()
        .join("install-receipt.json")
        .is_file());
    assert!(!fixture.store.staging_dir().exists());
    assert_eq!(qwen_state(&fixture.store), CatalogState::Installed);
}

#[test]
fn cancellation_preserves_parts_without_installing() {
    let fixture = DownloadFixture::matching_manifest();
    let operation = fixture
        .registry
        .begin_download(QWEN_CATALOG_ID)
        .expect("begin download");
    fixture.fetcher.replace_plan(
        "README.md",
        FakePlan {
            bytes: b"tiny readme".to_vec(),
            redirects: Vec::new(),
            content_range: None,
            cancel_after_request: Some(operation.cancel.clone()),
        },
    );
    let result = fixture.manager().run(QWEN_CATALOG_ID, &operation);
    fixture.registry.finish(&operation);
    assert!(matches!(result, Err(DownloadError::Cancelled)));
    assert!(fixture.store.staging_dir().join("README.md.part").is_file());
    assert_eq!(qwen_state(&fixture.store), CatalogState::Absent);
}

#[test]
fn resume_sends_range_and_requires_matching_content_range() {
    let fixture = DownloadFixture::matching_manifest();
    fs::create_dir_all(fixture.store.staging_dir()).expect("create stable staging directory");
    fs::write(fixture.store.staging_dir().join("README.md.part"), b"tiny ").expect("seed part");
    fixture.run().expect("resume succeeds");
    assert!(fixture
        .fetcher
        .requests()
        .iter()
        .any(|request| request.path == "README.md" && request.range_start == Some(5)));

    let fixture = DownloadFixture::matching_manifest();
    fs::create_dir_all(fixture.store.staging_dir()).expect("create stable staging directory");
    fs::write(fixture.store.staging_dir().join("README.md.part"), b"tiny ").expect("seed part");
    fixture.fetcher.replace_plan(
        "README.md",
        FakePlan {
            bytes: b"tiny readme".to_vec(),
            redirects: Vec::new(),
            content_range: Some("bytes 4-10/11".into()),
            cancel_after_request: None,
        },
    );
    assert!(matches!(
        fixture.run(),
        Err(DownloadError::InvalidContentRange { .. })
    ));
    assert_eq!(qwen_state(&fixture.store), CatalogState::Absent);
}

#[test]
fn one_byte_past_manifest_size_fails_without_installing() {
    let fixture = DownloadFixture::matching_manifest();
    fixture.fetcher.replace_plan(
        "README.md",
        FakePlan {
            bytes: b"tiny readme!".to_vec(),
            redirects: Vec::new(),
            content_range: None,
            cancel_after_request: None,
        },
    );
    assert!(matches!(
        fixture.run(),
        Err(DownloadError::SizeMismatch { .. })
    ));
    assert_eq!(qwen_state(&fixture.store), CatalogState::Absent);
}

#[test]
fn hash_mismatch_fails_without_installing() {
    let fixture = DownloadFixture::matching_manifest();
    fixture.fetcher.replace_plan(
        "README.md",
        FakePlan {
            bytes: b"tiny readmx".to_vec(),
            redirects: Vec::new(),
            content_range: None,
            cancel_after_request: None,
        },
    );
    assert!(matches!(
        fixture.run(),
        Err(DownloadError::HashMismatch { .. })
    ));
    assert_eq!(qwen_state(&fixture.store), CatalogState::Absent);
}

#[cfg(unix)]
#[test]
fn symlinked_catalog_parent_is_rejected_without_following_it() {
    use std::os::unix::fs::symlink;

    let fixture = DownloadFixture::matching_manifest();
    let redirected = fixture._dir.path().join("redirected-models");
    fs::create_dir_all(&redirected).expect("create redirected directory");
    symlink(&redirected, fixture._dir.path().join("models")).expect("install attacker symlink");
    assert!(matches!(
        fixture.run(),
        Err(DownloadError::SymlinkPath { .. })
    ));
    assert!(!redirected.join("catalog").exists());
}

#[test]
fn redirects_only_allow_reviewed_hosts_and_cap_the_chain() {
    assert!(allowed_download_host("huggingface.co"));
    assert!(allowed_download_host("cdn-lfs.huggingface.co"));
    assert!(!allowed_download_host("huggingface.co.evil.example"));
    let fixture = DownloadFixture::matching_manifest();
    fixture.fetcher.replace_plan(
        "README.md",
        FakePlan {
            bytes: b"tiny readme".to_vec(),
            redirects: vec!["huggingface.co".into(); 6],
            content_range: None,
            cancel_after_request: None,
        },
    );
    assert!(matches!(
        fixture.run(),
        Err(DownloadError::RedirectPolicy { .. })
    ));
}

#[test]
fn download_events_are_monotonic_and_finish_after_the_atomic_install() {
    let fixture = DownloadFixture::matching_manifest();
    fixture.run().expect("download succeeds");
    let events = fixture.events.events();
    assert!(events.windows(2).all(|pair| pair[0].seq < pair[1].seq));
    assert!(events
        .windows(2)
        .all(|pair| pair[0].downloaded_bytes <= pair[1].downloaded_bytes));
    assert_eq!(
        events.last().expect("terminal event").phase,
        DownloadPhase::Installed
    );
    assert!(fixture.store.qwen_install_dir().is_dir());
    assert!(!fixture.store.staging_dir().exists());
}

#[test]
fn registry_refuses_a_second_active_operation_for_the_same_catalog() {
    let registry = CatalogDownloadRegistry::default();
    let first = registry
        .begin_download(QWEN_CATALOG_ID)
        .expect("first operation");
    assert!(matches!(
        registry.begin_download(QWEN_CATALOG_ID),
        Err(DownloadError::OperationActive { .. })
    ));
    registry.finish(&first);
    assert!(registry.begin_download(QWEN_CATALOG_ID).is_ok());
}

#[test]
fn remove_refuses_a_running_catalog_model_and_only_removes_the_fixed_install() {
    let fixture = DownloadFixture::matching_manifest();
    fixture.run().expect("install fixture model");
    assert!(matches!(
        remove_catalog_model(&fixture.store, QWEN_CATALOG_ID, true),
        Err(DownloadError::ModelRunning { .. })
    ));
    assert!(fixture.store.qwen_install_dir().exists());
    remove_catalog_model(&fixture.store, QWEN_CATALOG_ID, false).expect("remove stopped model");
    assert!(!fixture.store.qwen_install_dir().exists());
}
