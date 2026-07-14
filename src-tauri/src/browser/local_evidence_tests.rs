//! App-private, session-owned Browser evidence regressions.

use std::fs;

use super::evidence::{BrowserCaptureKind, CapturedBrowserText};
use super::local_evidence::{
    acquire_local_evidence_process_lock, delete_local_session_with_evidence,
    finish_local_evidence_delete, read_local_screenshot_evidence, read_local_text_evidence,
    reconcile_local_evidence_tombstones, restore_local_evidence_delete, session_evidence_root,
    stage_local_evidence_delete, store_local_screenshot_evidence, store_local_text_evidence,
    LocalEvidenceError, LocalEvidenceOwner,
};
use super::screenshot_evidence::CapturedBrowserScreenshot;
use crate::sessions;

struct TempDir {
    path: std::path::PathBuf,
}

impl TempDir {
    fn new(label: &str) -> Self {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "plume-local-evidence-{label}-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(&path).unwrap();
        Self { path }
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn capture(content: String) -> CapturedBrowserText {
    CapturedBrowserText {
        capture_kind: BrowserCaptureKind::Page,
        source_url: "https://example.com/page?private=ignored".into(),
        title: Some("Example".into()),
        content,
        source_truncated: false,
    }
}

fn one_pixel_png() -> Vec<u8> {
    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, 1, 1);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().unwrap();
        writer.write_image_data(&[10, 20, 30, 255]).unwrap();
    }
    bytes
}

#[test]
fn local_text_is_redacted_owned_and_physically_separate_per_session() {
    let td = TempDir::new("text-owner");
    let sessions_dir = td.path.join("app-data/sessions");
    let first = sessions::create(&sessions_dir, Some("first")).unwrap();
    let second = sessions::create(&sessions_dir, Some("second")).unwrap();
    let first_owner = LocalEvidenceOwner {
        session_id: first.id.clone(),
    };
    let second_owner = LocalEvidenceOwner {
        session_id: second.id.clone(),
    };
    let secret = format!("sk-{}", "x".repeat(24));
    let summary = store_local_text_evidence(
        &sessions_dir,
        &first_owner,
        capture(format!("private value {secret}")),
    )
    .unwrap();
    assert!(summary.preview.contains("[REDACTED:api-key]"));
    assert!(!summary.preview.contains(&secret));
    let record = read_local_text_evidence(&sessions_dir, &first_owner, &summary.evidence_id)
        .unwrap()
        .expect("owner can read its record");
    assert!(!record.content.contains(&secret));
    assert_eq!(
        read_local_text_evidence(&sessions_dir, &second_owner, &summary.evidence_id).unwrap(),
        None
    );
    assert_ne!(
        session_evidence_root(&sessions_dir, &first_owner).unwrap(),
        session_evidence_root(&sessions_dir, &second_owner).unwrap()
    );
}

#[test]
fn local_screenshot_reuses_png_validation_and_digest_checks() {
    let td = TempDir::new("screenshot");
    let sessions_dir = td.path.join("app-data/sessions");
    let session = sessions::create(&sessions_dir, None).unwrap();
    let owner = LocalEvidenceOwner {
        session_id: session.id,
    };
    let png = one_pixel_png();
    let summary = store_local_screenshot_evidence(
        &sessions_dir,
        &owner,
        CapturedBrowserScreenshot {
            source_url: "https://example.com/screenshot".into(),
            title: Some("Screenshot".into()),
            png_bytes: png.clone(),
            width: 1,
            height: 1,
        },
    )
    .unwrap();
    assert_eq!(summary.bytes, png.len() as u64);
    assert_eq!(summary.sha256.len(), 64);
    let stored = read_local_screenshot_evidence(&sessions_dir, &owner, &summary.evidence_id)
        .unwrap()
        .expect("stored screenshot");
    assert_eq!(stored.png_bytes, png);
    assert_eq!(stored.metadata.sha256, summary.sha256);
}

#[test]
fn unknown_owner_is_rejected_before_any_directory_is_created() {
    let td = TempDir::new("unknown-owner");
    let sessions_dir = td.path.join("app-data/sessions");
    sessions::create(&sessions_dir, None).unwrap();
    let owner = LocalEvidenceOwner {
        session_id: "s00000000000000000000000000009999".into(),
    };
    assert!(matches!(
        store_local_text_evidence(&sessions_dir, &owner, capture("hello".into())),
        Err(LocalEvidenceError::OwnerNotFound)
    ));
    assert!(!session_evidence_root(&sessions_dir, &owner)
        .unwrap()
        .exists());
}

#[test]
fn tombstone_delete_can_restore_on_failure_then_finish_after_commit() {
    let td = TempDir::new("delete-protocol");
    let sessions_dir = td.path.join("app-data/sessions");
    let session = sessions::create(&sessions_dir, None).unwrap();
    let owner = LocalEvidenceOwner {
        session_id: session.id.clone(),
    };
    let summary =
        store_local_text_evidence(&sessions_dir, &owner, capture("keep me".into())).unwrap();
    let original = session_evidence_root(&sessions_dir, &owner).unwrap();

    let staged = stage_local_evidence_delete(&sessions_dir, &owner)
        .unwrap()
        .expect("evidence directory staged");
    assert!(!original.exists());
    restore_local_evidence_delete(staged).unwrap();
    assert!(
        read_local_text_evidence(&sessions_dir, &owner, &summary.evidence_id)
            .unwrap()
            .is_some()
    );

    let staged = stage_local_evidence_delete(&sessions_dir, &owner)
        .unwrap()
        .expect("evidence directory staged again");
    sessions::delete(&sessions_dir, &session.id).unwrap();
    let tombstone = staged.tombstone_path().to_path_buf();
    finish_local_evidence_delete(staged).unwrap();
    assert!(!original.exists());
    assert!(!tombstone.exists());
}

#[test]
fn reconciliation_restores_live_owner_and_purges_deleted_owner_tombstones() {
    let td = TempDir::new("crash-recovery");
    let sessions_dir = td.path.join("app-data/sessions");
    let live = sessions::create(&sessions_dir, Some("live")).unwrap();
    let live_owner = LocalEvidenceOwner {
        session_id: live.id.clone(),
    };
    store_local_text_evidence(&sessions_dir, &live_owner, capture("live evidence".into())).unwrap();
    let live_root = session_evidence_root(&sessions_dir, &live_owner).unwrap();
    let live_tombstone = stage_local_evidence_delete(&sessions_dir, &live_owner)
        .unwrap()
        .unwrap();
    let live_tombstone_path = live_tombstone.tombstone_path().to_path_buf();
    std::mem::forget(live_tombstone); // Simulate process loss after the rename.

    reconcile_local_evidence_tombstones(&sessions_dir).unwrap();
    assert!(live_root.exists());
    assert!(!live_tombstone_path.exists());

    let gone = sessions::create(&sessions_dir, Some("gone")).unwrap();
    let gone_owner = LocalEvidenceOwner {
        session_id: gone.id.clone(),
    };
    store_local_text_evidence(&sessions_dir, &gone_owner, capture("gone evidence".into())).unwrap();
    let gone_tombstone = stage_local_evidence_delete(&sessions_dir, &gone_owner)
        .unwrap()
        .unwrap();
    let gone_tombstone_path = gone_tombstone.tombstone_path().to_path_buf();
    std::mem::forget(gone_tombstone); // Simulate process loss after the DB commit.
    sessions::delete(&sessions_dir, &gone.id).unwrap();

    reconcile_local_evidence_tombstones(&sessions_dir).unwrap();
    assert!(!gone_tombstone_path.exists());
}

#[test]
fn composite_delete_removes_evidence_even_when_the_transcript_is_corrupt() {
    let td = TempDir::new("corrupt-delete");
    let sessions_dir = td.path.join("app-data/sessions");
    let session = sessions::create(&sessions_dir, None).unwrap();
    let owner = LocalEvidenceOwner {
        session_id: session.id.clone(),
    };
    store_local_text_evidence(&sessions_dir, &owner, capture("delete me".into())).unwrap();
    let evidence_root = session_evidence_root(&sessions_dir, &owner).unwrap();

    let conn = rusqlite::Connection::open(sessions_dir.join("state.sqlite")).unwrap();
    conn.execute(
        "INSERT INTO chat_messages
         (id,session_id,ordinal,kind,role,content,created_at_ms)
         VALUES (?1,?2,0,'not-a-kind','user','corrupt',1)",
        rusqlite::params!["m0000000000000000000000000000000f", session.id],
    )
    .unwrap();
    drop(conn);
    assert!(sessions::load(&sessions_dir, &session.id).is_err());

    delete_local_session_with_evidence(&sessions_dir, &session.id).unwrap();
    assert!(!evidence_root.exists());
    assert!(matches!(
        sessions::load(&sessions_dir, &session.id),
        Err(sessions::SessionStoreError::NotFound(_))
    ));
}

#[cfg(unix)]
#[test]
fn process_lock_serializes_independent_file_descriptors() {
    use std::sync::mpsc;
    use std::time::Duration;

    let td = TempDir::new("process-lock");
    let sessions_dir = td.path.join("app-data/sessions");
    sessions::create(&sessions_dir, None).unwrap();
    let held = acquire_local_evidence_process_lock(&sessions_dir).unwrap();
    let contender_dir = sessions_dir.clone();
    let (tx, rx) = mpsc::channel();
    let contender = std::thread::spawn(move || {
        let acquired = acquire_local_evidence_process_lock(&contender_dir).unwrap();
        tx.send(()).unwrap();
        drop(acquired);
    });

    assert!(rx.recv_timeout(Duration::from_millis(100)).is_err());
    drop(held);
    rx.recv_timeout(Duration::from_secs(2))
        .expect("contender acquires only after the first advisory lock drops");
    contender.join().unwrap();
}

#[cfg(unix)]
#[test]
fn symlinked_session_root_and_hardlinked_records_are_refused() {
    use std::os::unix::fs::symlink;

    let td = TempDir::new("aliases");
    let sessions_dir = td.path.join("app-data/sessions");
    let first = sessions::create(&sessions_dir, None).unwrap();
    let first_owner = LocalEvidenceOwner {
        session_id: first.id,
    };
    let root = session_evidence_root(&sessions_dir, &first_owner).unwrap();
    fs::create_dir_all(root.parent().unwrap()).unwrap();
    let outside = td.path.join("outside");
    fs::create_dir_all(&outside).unwrap();
    symlink(&outside, &root).unwrap();
    assert!(matches!(
        store_local_text_evidence(&sessions_dir, &first_owner, capture("blocked".into())),
        Err(LocalEvidenceError::Refused(_))
    ));
    fs::remove_file(&root).unwrap();

    let summary =
        store_local_text_evidence(&sessions_dir, &first_owner, capture("stored".into())).unwrap();
    let record = root
        .join(".plume/browser-evidence")
        .join(format!("{}.json", summary.evidence_id));
    let alias = td.path.join("record-alias.json");
    fs::hard_link(&record, &alias).unwrap();
    assert!(matches!(
        read_local_text_evidence(&sessions_dir, &first_owner, &summary.evidence_id),
        Err(LocalEvidenceError::Refused(_))
    ));
}
