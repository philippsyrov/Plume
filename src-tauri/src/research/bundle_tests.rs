use std::fs;
use std::sync::Arc;

use crate::browser::evidence::BrowserCaptureKind;
use crate::project::OpenProject;
use crate::sessions::owner::{resolve_session_owner, SessionOwnerRef, SessionOwnerScope};
use crate::sessions::{self, create};

use super::bundle::{
    fail_delete_after_staging_for_test, stage_interrupted_delete_for_test, ArtifactBundleInput,
    ArtifactCitationStatus, ArtifactOutcome, ArtifactStore, ArtifactStoreError, BundleDraft,
    BundleSourceSummary, MAX_ARTIFACT_RECORDS,
};
use super::evidence::ResearchEvidenceSource;

fn local_store(temp: &tempfile::TempDir) -> ArtifactStore {
    let sessions_dir = temp.path().join("sessions");
    let session = create(&sessions_dir, Some("research")).expect("session");
    let owner = resolve_session_owner(
        &SessionOwnerRef {
            scope: SessionOwnerScope::Local,
            session_id: session.id,
        },
        SessionOwnerScope::Local,
        &sessions_dir,
        None,
    )
    .expect("owner");
    ArtifactStore::from_owner(&owner).expect("store")
}

fn project_store(temp: &tempfile::TempDir, label: &str) -> ArtifactStore {
    let root = temp.path().join(label);
    fs::create_dir_all(&root).expect("project");
    let root = fs::canonicalize(root).expect("canonical project");
    let project = OpenProject {
        id: format!("generation-{label}"),
        root: root.clone(),
    };
    let sessions_dir = sessions::project_sessions_dir(&root).expect("sessions dir");
    let session = create(&sessions_dir, Some("research")).expect("session");
    let owner = resolve_session_owner(
        &SessionOwnerRef {
            scope: SessionOwnerScope::Project,
            session_id: session.id,
        },
        SessionOwnerScope::Project,
        &temp.path().join("local"),
        Some(&project),
    )
    .expect("owner");
    ArtifactStore::from_owner(&owner).expect("store")
}

fn input(note: &str) -> ArtifactBundleInput {
    let content = "evidence".to_string();
    let source = ResearchEvidenceSource {
        source_id: "S1".into(),
        evidence_id: format!("be_{}", "a".repeat(32)),
        capture_kind: BrowserCaptureKind::Page,
        source_url: "https://example.com".into(),
        title: Some("Example".into()),
        captured_at_ms: 1,
        sha256: format!("{:x}", sha2::Sha256::digest(content.as_bytes())),
        bytes: content.len() as u64,
        content,
        redaction_count: 0,
        truncated: false,
    };
    ArtifactBundleInput {
        user_request: note.into(),
        provider_id: "apple-foundation".into(),
        model_id: "system".into(),
        runtime_id: "apple-system".into(),
        sources: vec![source],
        screenshot_sources: Vec::new(),
        summaries: vec![BundleSourceSummary {
            source_id: "S1".into(),
            summary: "summary".into(),
            logical_turn: 1,
            provider_calls: 1,
        }],
        drafts: vec![BundleDraft {
            markdown: format!("{note} [[S1]]"),
            citation_status: ArtifactCitationStatus::Verified,
        }],
        logical_turns: 2,
        provider_calls: 2,
        duration_ms: 5,
        outcome: ArtifactOutcome::Complete,
    }
}

fn large_input() -> ArtifactBundleInput {
    let mut value = input(&"q".repeat(256 * 1024));
    value.sources = (1..=10)
        .map(|index| {
            let content = "e".repeat(64 * 1024);
            ResearchEvidenceSource {
                source_id: format!("S{index}"),
                evidence_id: format!("be_{index:032x}"),
                capture_kind: BrowserCaptureKind::Page,
                source_url: format!("https://example.com/{index}"),
                title: Some(format!("Source {index}")),
                captured_at_ms: index,
                sha256: format!("{:x}", sha2::Sha256::digest(content.as_bytes())),
                bytes: content.len() as u64,
                content,
                redaction_count: 0,
                truncated: false,
            }
        })
        .collect();
    value.summaries = (1..=10)
        .map(|index| BundleSourceSummary {
            source_id: format!("S{index}"),
            summary: "s".repeat(16 * 1024),
            logical_turn: index as u32,
            provider_calls: index as u32,
        })
        .collect();
    value.drafts = (0..3)
        .map(|_| BundleDraft {
            markdown: "d".repeat(256 * 1024),
            citation_status: ArtifactCitationStatus::Verified,
        })
        .collect();
    value
}

use sha2::Digest;

#[test]
fn bundle_schema_round_trips_screenshot_provenance_separately_from_text_sources() {
    let mut value = serde_json::to_value(input("visual note")).expect("serialize input");
    value.as_object_mut().expect("input object").insert(
        "screenshotSources".into(),
        serde_json::json!([{
            "evidenceId": "bs_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "sourceUrl": "https://example.com/diagram",
            "title": "Diagram",
            "capturedAtMs": 7,
            "sha256": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            "width": 800,
            "height": 600,
            "bytes": 12345
        }]),
    );

    let parsed: ArtifactBundleInput =
        serde_json::from_value(value).expect("parse screenshot provenance");
    let round_trip = serde_json::to_value(parsed).expect("serialize screenshot provenance");

    assert_eq!(
        round_trip["screenshotSources"],
        serde_json::json!([{
            "evidenceId": "bs_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "sourceUrl": "https://example.com/diagram",
            "title": "Diagram",
            "capturedAtMs": 7,
            "sha256": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            "width": 800,
            "height": 600,
            "bytes": 12345
        }])
    );
}

#[test]
fn local_and_project_stores_round_trip_immutable_versions() {
    let temp = tempfile::tempdir().expect("tempdir");
    let local = local_store(&temp);
    let project = project_store(&temp, "project");
    let first = local.stage_new(input("first")).expect("stage local");
    let second = local
        .stage_revision(&first.artifact_id, input("second"))
        .expect("stage revision");
    assert_eq!(first.artifact_version, 1);
    assert_eq!(second.artifact_version, 2);
    assert_eq!(
        local
            .load_version(&first.artifact_id, 1)
            .expect("load v1")
            .input
            .user_request,
        "first"
    );
    assert_eq!(
        local
            .load_latest(&first.artifact_id)
            .expect("latest")
            .input
            .user_request,
        "second"
    );
    assert!(matches!(
        project.load_latest(&first.artifact_id),
        Err(ArtifactStoreError::NotFound)
    ));
}

#[test]
fn a_captured_store_cannot_recreate_artifacts_after_its_session_is_deleted() {
    let temp = tempfile::tempdir().expect("tempdir");
    let store = local_store(&temp);
    sessions::delete(store.sessions_dir_for_test(), store.session_id_for_test())
        .expect("delete owner session");

    assert!(matches!(
        store.stage_new(input("late result")),
        Err(ArtifactStoreError::NotFound)
    ));
    assert!(!store.session_root_for_test().exists());
}

#[test]
fn rejected_and_oversized_publications_leave_no_partial_record() {
    let temp = tempfile::tempdir().expect("tempdir");
    let store = local_store(&temp);
    let mut oversized = input("oversized");
    oversized.user_request = "x".repeat(5 * 1024 * 1024);
    assert!(matches!(
        store.stage_new(oversized),
        Err(ArtifactStoreError::Limit(_))
    ));
    assert!(store.list().expect("list").is_empty());
    assert!(store
        .session_root_for_test()
        .read_dir()
        .map(|entries| entries
            .flatten()
            .all(|entry| !entry.file_name().to_string_lossy().contains(".tmp")))
        .unwrap_or(true));
}

#[test]
fn corrupt_latest_is_quarantined_and_previous_version_recovers() {
    let temp = tempfile::tempdir().expect("tempdir");
    let store = local_store(&temp);
    let first = store.stage_new(input("first")).unwrap();
    store
        .stage_revision(&first.artifact_id, input("second"))
        .unwrap();
    fs::write(
        store.record_path_for_test(&first.artifact_id, 2),
        b"not json",
    )
    .expect("corrupt record");
    let recovered = store
        .load_latest(&first.artifact_id)
        .expect("recover prior");
    assert_eq!(recovered.artifact_version, 1);
    assert!(store
        .session_root_for_test()
        .read_dir()
        .unwrap()
        .flatten()
        .any(|entry| entry.file_name().to_string_lossy().starts_with(".corrupt-")));
}

#[test]
fn concurrent_revisions_are_serialized_and_record_cap_is_hard() {
    let temp = tempfile::tempdir().expect("tempdir");
    let store = Arc::new(local_store(&temp));
    let first = store.stage_new(input("first")).unwrap();
    let mut threads = Vec::new();
    for index in 0..8 {
        let store = store.clone();
        let artifact_id = first.artifact_id.clone();
        threads.push(std::thread::spawn(move || {
            store.stage_revision(&artifact_id, input(&format!("revision-{index}")))
        }));
    }
    for thread in threads {
        thread.join().unwrap().expect("serialized revision");
    }
    assert_eq!(
        store
            .load_latest(&first.artifact_id)
            .unwrap()
            .artifact_version,
        9
    );

    for index in store.list().unwrap().len()..MAX_ARTIFACT_RECORDS {
        store.stage_new(input(&format!("fill-{index}"))).unwrap();
    }
    assert!(matches!(
        store.stage_new(input("past cap")),
        Err(ArtifactStoreError::Limit(_))
    ));
}

#[test]
fn pending_publication_cannot_overshoot_the_session_byte_cap() {
    let temp = tempfile::tempdir().expect("tempdir");
    let store = local_store(&temp);
    while store.stage_new(large_input()).is_ok() {}
    let stored_bytes = store
        .session_root_for_test()
        .read_dir()
        .unwrap()
        .flatten()
        .filter_map(|entry| entry.metadata().ok())
        .filter(|metadata| metadata.is_file())
        .map(|metadata| metadata.len())
        .sum::<u64>();
    assert!(stored_bytes <= 32 * 1024 * 1024);
}

#[cfg(unix)]
#[test]
fn symlink_and_hardlink_records_are_refused() {
    use std::os::unix::fs::symlink;

    let temp = tempfile::tempdir().expect("tempdir");
    let store = local_store(&temp);
    let record = store.stage_new(input("first")).unwrap();
    let path = store.record_path_for_test(&record.artifact_id, 1);
    let outside = temp.path().join("outside.json");
    fs::write(&outside, fs::read(&path).unwrap()).unwrap();
    fs::remove_file(&path).unwrap();
    symlink(&outside, &path).unwrap();
    assert!(matches!(
        store.load_version(&record.artifact_id, 1),
        Err(ArtifactStoreError::Refused(_))
    ));
    fs::remove_file(&path).unwrap();
    fs::hard_link(&outside, &path).unwrap();
    assert!(matches!(
        store.load_version(&record.artifact_id, 1),
        Err(ArtifactStoreError::Refused(_))
    ));
}

#[test]
fn failed_delete_restores_and_interrupted_delete_reconciles() {
    let temp = tempfile::tempdir().expect("tempdir");
    let store = local_store(&temp);
    let record = store.stage_new(input("kept")).unwrap();
    assert!(fail_delete_after_staging_for_test(&store).is_err());
    assert!(store.load_latest(&record.artifact_id).is_ok());

    let tombstone = stage_interrupted_delete_for_test(&store).unwrap();
    assert!(tombstone.is_dir());
    assert!(!store.session_root_for_test().exists());
    assert_eq!(store.list().unwrap().len(), 1);
    assert!(!tombstone.exists());

    let tombstone = stage_interrupted_delete_for_test(&store).unwrap();
    sessions::delete(store.sessions_dir_for_test(), store.session_id_for_test()).unwrap();
    assert!(matches!(store.list(), Err(ArtifactStoreError::NotFound)));
    assert!(!tombstone.exists());
}
