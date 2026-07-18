use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use serde_json::json;
use sha2::{Digest, Sha256};

use super::catalog::{CatalogState, CatalogStore, QWEN_CATALOG_ID, QWEN_REVISION};
use super::catalog_download::{
    allowed_download_host, redirect_is_allowed, remove_catalog_model, CatalogDownloadEvent,
    CatalogDownloadManager, CatalogDownloadRegistry, DownloadError, DownloadEventSink,
    DownloadFetcher, DownloadManifest, DownloadPhase, DownloadRequest, DownloadResponse,
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
    ignored_range_starts: Arc<Mutex<BTreeMap<String, Vec<u64>>>>,
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
            ignored_range_starts: Arc::new(Mutex::new(BTreeMap::new())),
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

    fn ignore_range_at(&self, path: &str, start: u64) {
        self.ignored_range_starts
            .lock()
            .expect("ignored ranges mutex")
            .entry(path.into())
            .or_default()
            .push(start);
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
        let end = request
            .range_end
            .map(|end| end as usize)
            .unwrap_or_else(|| plan.bytes.len().saturating_sub(1));
        let ignores_range = self
            .ignored_range_starts
            .lock()
            .expect("ignored ranges mutex")
            .get(&request.path)
            .is_some_and(|starts| starts.contains(&(start as u64)));
        if ignores_range {
            return Ok(DownloadResponse::from_bytes(
                200,
                None,
                plan.redirects,
                plan.bytes,
            ));
        }
        let status = if request.range_end.is_some() || start > 0 {
            206
        } else {
            200
        };
        let content_range = plan.content_range.or_else(|| {
            (status == 206).then(|| format!("bytes {start}-{end}/{}", plan.bytes.len()))
        });
        Ok(DownloadResponse::from_bytes(
            status,
            content_range,
            plan.redirects,
            if plan.bytes.len() > end.saturating_add(1) {
                // A malicious origin can ignore an otherwise valid range end.
                // Keep that protocol-violation path covered separately from a
                // normal ranged response.
                plan.bytes[start..].to_vec()
            } else {
                plan.bytes[start..=end].to_vec()
            },
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

#[derive(Clone)]
struct TerminalCleanupProbe {
    registry: CatalogDownloadRegistry,
    store: Arc<CatalogStore>,
    observed: Arc<AtomicBool>,
}

impl DownloadEventSink for TerminalCleanupProbe {
    fn emit(&self, event: CatalogDownloadEvent) {
        if matches!(
            event.phase,
            DownloadPhase::Installed | DownloadPhase::Cancelled | DownloadPhase::Failed
        ) {
            let retry = self
                .registry
                .begin_download_for_store(&self.store, QWEN_CATALOG_ID)
                .expect("terminal event follows registry cleanup");
            self.registry.finish(&retry);
            self.observed.store(true, Ordering::Release);
        }
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
        let operation = self
            .registry
            .begin_download_for_store(&self.store, QWEN_CATALOG_ID)?;
        let manager = self.manager();
        let result = manager.run(QWEN_CATALOG_ID, &operation);
        self.registry.finish(&operation);
        manager.emit_terminal(&operation, &result);
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
        .begin_download_for_store(&fixture.store, QWEN_CATALOG_ID)
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
            content_range: Some("bytes 0-10/11".into()),
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
fn terminal_event_is_emitted_only_after_registry_cleanup() {
    let fixture = DownloadFixture::matching_manifest();
    let probe = TerminalCleanupProbe {
        registry: fixture.registry.clone(),
        store: fixture.store.clone(),
        observed: Arc::new(AtomicBool::new(false)),
    };
    let manager = CatalogDownloadManager::new(
        fixture.store.clone(),
        fixture.manifest.clone(),
        fixture.fetcher.clone(),
        probe.clone(),
    );
    let operation = fixture
        .registry
        .begin_download_for_store(&fixture.store, QWEN_CATALOG_ID)
        .expect("begin download");
    let result = manager.run(QWEN_CATALOG_ID, &operation);
    fixture.registry.finish(&operation);
    manager.emit_terminal(&operation, &result);

    assert!(result.is_ok());
    assert!(probe.observed.load(Ordering::Acquire));
}

#[test]
fn registry_refuses_a_second_active_operation_for_the_same_catalog() {
    let fixture = DownloadFixture::matching_manifest();
    let first = fixture
        .registry
        .begin_download_for_store(&fixture.store, QWEN_CATALOG_ID)
        .expect("first operation");
    assert!(matches!(
        fixture
            .registry
            .begin_download_for_store(&fixture.store, QWEN_CATALOG_ID),
        Err(DownloadError::OperationActive { .. })
    ));
    fixture.registry.finish(&first);
    assert!(fixture
        .registry
        .begin_download_for_store(&fixture.store, QWEN_CATALOG_ID)
        .is_ok());
}

#[test]
fn remove_refuses_a_running_catalog_model_and_only_removes_the_fixed_install() {
    let fixture = DownloadFixture::matching_manifest();
    fixture.run().expect("install fixture model");
    assert!(matches!(
        remove_catalog_model(&fixture.registry, &fixture.store, QWEN_CATALOG_ID, || true),
        Err(DownloadError::ModelRunning { .. })
    ));
    assert!(fixture.store.qwen_install_dir().exists());
    remove_catalog_model(&fixture.registry, &fixture.store, QWEN_CATALOG_ID, || false)
        .expect("remove stopped model");
    assert!(!fixture.store.qwen_install_dir().exists());
}

#[cfg(unix)]
#[test]
fn hardlinked_staging_part_is_rejected_before_any_download_or_publish() {
    let fixture = DownloadFixture::matching_manifest();
    fs::create_dir_all(fixture.store.staging_dir()).expect("create staging directory");
    let outside = fixture._dir.path().join("outside-readable-file");
    fs::write(&outside, b"tiny readme").expect("write external file");
    fs::hard_link(&outside, fixture.store.staging_dir().join("README.md.part"))
        .expect("create attacker hardlink");

    assert!(matches!(
        fixture.run(),
        Err(DownloadError::UnexpectedStagingPath { .. })
    ));
    assert!(!fixture.store.qwen_install_dir().exists());
    assert_eq!(
        fs::read(&outside).expect("external file survives"),
        b"tiny readme"
    );
}

#[test]
fn corrupt_complete_part_is_discarded_before_initial_progress_and_retried_monotonically() {
    let fixture = DownloadFixture::matching_manifest();
    fs::create_dir_all(fixture.store.staging_dir()).expect("create staging directory");
    fs::write(
        fixture.store.staging_dir().join("README.md.part"),
        b"bad payload",
    )
    .expect("seed same-length corrupt complete part");

    fixture
        .run()
        .expect("a corrupt complete part is redownloaded");
    let events = fixture.events.events();
    assert!(events
        .iter()
        .all(|event| event.downloaded_bytes <= fixture.expected_bytes()));
    assert!(events
        .windows(2)
        .all(|pair| pair[0].downloaded_bytes <= pair[1].downloaded_bytes));
}

#[test]
fn redirect_policy_allows_exactly_five_https_hops_and_rejects_plain_http() {
    let fixture = DownloadFixture::matching_manifest();
    fixture.fetcher.replace_plan(
        "README.md",
        FakePlan {
            bytes: b"tiny readme".to_vec(),
            redirects: vec!["https://huggingface.co/redirect".into(); 5],
            content_range: None,
            cancel_after_request: None,
        },
    );
    fixture
        .run()
        .expect("five reviewed HTTPS redirect hops are allowed");

    let fixture = DownloadFixture::matching_manifest();
    fixture.fetcher.replace_plan(
        "README.md",
        FakePlan {
            bytes: b"tiny readme".to_vec(),
            redirects: vec!["http://huggingface.co/plain-http".into()],
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
fn reqwest_redirect_policy_allows_five_real_hops_and_https_only() {
    let allowed = reqwest::Url::parse("https://huggingface.co/redirect").expect("valid URL");
    assert!(redirect_is_allowed(5, &allowed));
    assert!(!redirect_is_allowed(6, &allowed));

    let plain_http = reqwest::Url::parse("http://huggingface.co/redirect").expect("valid URL");
    assert!(!redirect_is_allowed(1, &plain_http));
    let unreviewed = reqwest::Url::parse("https://example.test/redirect").expect("valid URL");
    assert!(!redirect_is_allowed(1, &unreviewed));
}

#[test]
fn two_independent_registries_contend_for_the_same_catalog_filesystem_lock() {
    let fixture = DownloadFixture::matching_manifest();
    let first_registry = CatalogDownloadRegistry::default();
    let second_registry = CatalogDownloadRegistry::default();
    let first = first_registry
        .begin_download_for_store(&fixture.store, QWEN_CATALOG_ID)
        .expect("first registry owns the catalog lock");
    assert!(matches!(
        second_registry.begin_download_for_store(&fixture.store, QWEN_CATALOG_ID),
        Err(DownloadError::OperationActive { .. })
    ));
    first_registry.finish(&first);
    assert!(second_registry
        .begin_download_for_store(&fixture.store, QWEN_CATALOG_ID)
        .is_ok());
}

#[test]
fn removal_and_begin_share_one_atomic_catalog_lifecycle_gate() {
    let fixture = DownloadFixture::matching_manifest();
    let operation = fixture
        .registry
        .begin_download_for_store(&fixture.store, QWEN_CATALOG_ID)
        .expect("download reserves both lifecycle gates");

    assert!(matches!(
        remove_catalog_model(&fixture.registry, &fixture.store, QWEN_CATALOG_ID, || false,),
        Err(DownloadError::OperationActive { .. })
    ));

    fixture.registry.finish(&operation);
}

#[cfg(unix)]
#[test]
fn descriptor_removal_survives_a_catalog_parent_swap_without_touching_the_target() {
    use std::os::unix::fs::symlink;

    let fixture = DownloadFixture::matching_manifest();
    fixture.run().expect("install fixture model");
    let install = fixture.store.qwen_install_dir();
    let catalog_id_dir = install.parent().expect("revision has catalog-id parent");
    let moved_catalog_id_dir = fixture._dir.path().join("moved-catalog-id");
    let attacker_target = fixture._dir.path().join("attacker-target");
    let sentinel = attacker_target.join(QWEN_REVISION).join("must-survive");
    fs::create_dir_all(sentinel.parent().expect("sentinel parent"))
        .expect("create attacker target");
    fs::write(&sentinel, b"outside").expect("write attacker sentinel");

    let removed = super::catalog_download::remove_verified_install_with_parent_swap_for_test(
        &fixture.store,
        || {
            fs::rename(catalog_id_dir, &moved_catalog_id_dir).expect("move original catalog id");
            symlink(&attacker_target, catalog_id_dir).expect("replace catalog id with symlink");
        },
    )
    .expect("descriptor-rooted removal remains on the opened original directory");

    assert!(removed);
    assert!(!moved_catalog_id_dir.join(QWEN_REVISION).exists());
    assert_eq!(
        fs::read(sentinel).expect("external sentinel survives"),
        b"outside"
    );
}

#[test]
fn stale_prepared_directory_is_recovered_before_a_resumable_retry() {
    let fixture = DownloadFixture::matching_manifest();
    let prepared = fixture
        .store
        .staging_dir()
        .parent()
        .expect("staging has catalog-id parent")
        .join(format!(".{QWEN_REVISION}.prepared"));
    fs::create_dir_all(&prepared).expect("seed interrupted prepared directory");
    fs::write(prepared.join("partial-final-file"), b"incomplete")
        .expect("seed interrupted prepared data");

    fixture
        .run()
        .expect("retry recovers prepared state and installs");
    assert!(!prepared.exists());
    assert!(fixture.store.qwen_install_dir().is_dir());
}

#[path = "catalog_download_resume_tests.rs"]
mod resume_tests;
