//! D63A store-level tests. Every test drives the public store API
//! against a throwaway directory; "raw" connections are used only to
//! tamper with or inspect the database the way a hostile or corrupted
//! file would present itself.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::json;

use super::*;

pub(super) struct TempDir {
    path: PathBuf,
}

impl TempDir {
    pub(super) fn new(label: &str) -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock before epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "plume-sessions-test-{}-{}-{}",
            label,
            std::process::id(),
            nanos
        ));
        fs::create_dir_all(&path).expect("create tempdir");
        Self { path }
    }
    pub(super) fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

pub(super) fn raw_conn(sessions_dir: &Path) -> rusqlite::Connection {
    rusqlite::Connection::open(sessions_dir.join(schema::DB_FILE_NAME)).expect("raw open")
}

pub(super) fn user_entry(content: &str) -> TranscriptEntry {
    TranscriptEntry::Message {
        message: EntryMessage {
            role: EntryRole::User,
            content: content.to_string(),
        },
        model_used: None,
        duration_ms: None,
        attachment_rel_path: None,
        attachment_line_range: None,
        stats: None,
        sent_in_mode: Some(SentMode::Chat),
    }
}

fn assistant_entry(content: &str) -> TranscriptEntry {
    TranscriptEntry::Message {
        message: EntryMessage {
            role: EntryRole::Assistant,
            content: content.to_string(),
        },
        model_used: Some("qwen2.5-coder".to_string()),
        duration_ms: Some(1234),
        attachment_rel_path: None,
        attachment_line_range: None,
        stats: Some(EntryStats {
            output_tokens: Some(42),
            eval_ms: Some(900),
            tokens_per_second: Some(46.5),
            prompt_tokens: Some(101),
            prompt_ms: None,
        }),
        sent_in_mode: None,
    }
}

// ---------------------------------------------------------------
// Physical separation
// ---------------------------------------------------------------

#[test]
fn local_and_project_stores_are_physically_separate() {
    let td = TempDir::new("separate");
    let local = local_sessions_dir(&td.path().join("app-data"));
    let project_root = td.path().join("project");
    fs::create_dir_all(&project_root).unwrap();
    let project = project_sessions_dir(&project_root).unwrap();

    let local_session = create(&local, Some("local chat")).unwrap();
    let project_session = create(&project, Some("project chat")).unwrap();

    // A mismatched id against the other database is a plain NotFound.
    assert!(matches!(
        load(&project, &local_session.id),
        Err(SessionStoreError::NotFound(_))
    ));
    assert!(matches!(
        load(&local, &project_session.id),
        Err(SessionStoreError::NotFound(_))
    ));

    // Each list shows only its own sessions.
    let local_list = list(&local, true).unwrap();
    assert_eq!(local_list.len(), 1);
    assert_eq!(local_list[0].id, local_session.id);
    let project_list = list(&project, true).unwrap();
    assert_eq!(project_list.len(), 1);
    assert_eq!(project_list[0].id, project_session.id);

    // And the files really are two different databases on disk.
    assert!(local.join(schema::DB_FILE_NAME).is_file());
    assert!(project.join(schema::DB_FILE_NAME).is_file());
}

// ---------------------------------------------------------------
// Create / list ordering
// ---------------------------------------------------------------

#[test]
fn create_defaults_title_and_trims_a_provided_one() {
    let td = TempDir::new("titles");
    let dir = td.path().join("sessions");
    let defaulted = create(&dir, None).unwrap();
    assert_eq!(defaulted.title, "New chat");
    assert!(defaulted.archived_at_ms.is_none());
    assert_eq!(defaulted.created_at_ms, defaulted.updated_at_ms);

    let trimmed = create(&dir, Some("  Padded title  ")).unwrap();
    assert_eq!(trimmed.title, "Padded title");
}

#[test]
fn list_orders_by_latest_update() {
    let td = TempDir::new("ordering");
    let dir = td.path().join("sessions");
    let first = create(&dir, Some("first")).unwrap();
    let second = create(&dir, Some("second")).unwrap();

    // Newest creation first…
    let ids: Vec<String> = list(&dir, false)
        .unwrap()
        .into_iter()
        .map(|s| s.id)
        .collect();
    assert_eq!(ids, vec![second.id.clone(), first.id.clone()]);

    // …and a transcript save bumps the older one back to the top.
    save_transcript(&dir, &first.id, &[user_entry("hi")], false).unwrap();
    let ids: Vec<String> = list(&dir, false)
        .unwrap()
        .into_iter()
        .map(|s| s.id)
        .collect();
    assert_eq!(ids, vec![first.id, second.id]);
}

#[test]
fn create_fails_past_the_session_cap() {
    let td = TempDir::new("cap");
    let dir = td.path().join("sessions");
    for i in 0..validation::MAX_SESSIONS {
        create(&dir, Some(&format!("chat {i}"))).unwrap();
    }
    let err = create(&dir, Some("one too many")).expect_err("cap enforced");
    assert!(matches!(err, SessionStoreError::Limit(_)), "got {err:?}");
}

// ---------------------------------------------------------------
// Rename
// ---------------------------------------------------------------

#[test]
fn rename_trims_and_bounds_titles() {
    let td = TempDir::new("rename");
    let dir = td.path().join("sessions");
    let session = create(&dir, None).unwrap();

    let renamed = rename(&dir, &session.id, "  Real topic  ").unwrap();
    assert_eq!(renamed.title, "Real topic");
    assert!(renamed.updated_at_ms > session.updated_at_ms);

    assert!(matches!(
        rename(&dir, &session.id, "   "),
        Err(SessionStoreError::Invalid(_))
    ));
    let too_long = "x".repeat(validation::MAX_TITLE_CHARS + 1);
    assert!(matches!(
        rename(&dir, &session.id, &too_long),
        Err(SessionStoreError::Invalid(_))
    ));
    // Bound is Unicode scalar values, not bytes: 120 emoji are fine.
    let exactly_max = "🦀".repeat(validation::MAX_TITLE_CHARS);
    let renamed = rename(&dir, &session.id, &exactly_max).unwrap();
    assert_eq!(renamed.title.chars().count(), validation::MAX_TITLE_CHARS);

    assert!(matches!(
        rename(&dir, "s0000000000000000000000000000000", "ok"),
        Err(SessionStoreError::NotFound(_))
    ));
}

// ---------------------------------------------------------------
// Archive
// ---------------------------------------------------------------

#[test]
fn archive_hides_includes_and_unarchives() {
    let td = TempDir::new("archive");
    let dir = td.path().join("sessions");
    let keep = create(&dir, Some("keep")).unwrap();
    let shelve = create(&dir, Some("shelve")).unwrap();

    let archived = set_archived(&dir, &shelve.id, true).unwrap();
    assert!(archived.archived_at_ms.is_some());
    // Archiving does not bump updated_at_ms.
    assert_eq!(archived.updated_at_ms, shelve.updated_at_ms);

    let visible: Vec<String> = list(&dir, false)
        .unwrap()
        .into_iter()
        .map(|s| s.id)
        .collect();
    assert_eq!(visible, vec![keep.id.clone()]);
    let all: Vec<String> = list(&dir, true)
        .unwrap()
        .into_iter()
        .map(|s| s.id)
        .collect();
    assert_eq!(all, vec![shelve.id.clone(), keep.id.clone()]);

    // Idempotent: archiving again keeps the original stamp.
    let again = set_archived(&dir, &shelve.id, true).unwrap();
    assert_eq!(again.archived_at_ms, archived.archived_at_ms);

    let restored = set_archived(&dir, &shelve.id, false).unwrap();
    assert!(restored.archived_at_ms.is_none());
    assert_eq!(list(&dir, false).unwrap().len(), 2);
}

// ---------------------------------------------------------------
// Delete
// ---------------------------------------------------------------

#[test]
fn delete_cascades_messages_and_second_delete_is_not_found() {
    let td = TempDir::new("delete");
    let dir = td.path().join("sessions");
    let session = create(&dir, Some("doomed")).unwrap();
    save_transcript(
        &dir,
        &session.id,
        &[user_entry("hello"), assistant_entry("world")],
        false,
    )
    .unwrap();

    delete(&dir, &session.id).unwrap();

    // Cascade proof through a raw connection: no orphaned messages.
    // The delete ran on a freshly opened connection against an existing
    // database, so this also proves foreign keys are re-enabled on
    // reopen (the pragma is per-connection).
    let raw = raw_conn(&dir);
    let orphans: i64 = raw
        .query_row("SELECT COUNT(*) FROM chat_messages", [], |r| r.get(0))
        .unwrap();
    assert_eq!(orphans, 0);

    assert!(matches!(
        delete(&dir, &session.id),
        Err(SessionStoreError::NotFound(_))
    ));
}

// ---------------------------------------------------------------
// Transcript round-trips
// ---------------------------------------------------------------

#[test]
fn transcript_round_trips_message_cancelled_and_error_entries() {
    let td = TempDir::new("roundtrip");
    let dir = td.path().join("sessions");
    let session = create(&dir, Some("full house")).unwrap();

    let entries = vec![
        TranscriptEntry::Message {
            message: EntryMessage {
                role: EntryRole::User,
                content: "change greet() to an f-string".to_string(),
            },
            model_used: None,
            duration_ms: None,
            attachment_rel_path: None,
            attachment_line_range: None,
            stats: None,
            sent_in_mode: Some(SentMode::ProposeDiff),
        },
        assistant_entry("done — here is the diff"),
        TranscriptEntry::Cancelled {
            partial: "I was about to say".to_string(),
            model_used: Some("qwen2.5-coder".to_string()),
            duration_ms: Some(432),
        },
        TranscriptEntry::Error {
            message: "provider went away".to_string(),
        },
    ];
    save_transcript(&dir, &session.id, &entries, false).unwrap();

    let record = load(&dir, &session.id).unwrap();
    assert_eq!(record.entries, entries);
    assert_eq!(record.title, "full house");
}

#[test]
fn attachment_metadata_round_trips_for_project_scope() {
    let td = TempDir::new("attach");
    let project_root = td.path().join("project");
    fs::create_dir_all(&project_root).unwrap();
    let dir = project_sessions_dir(&project_root).unwrap();
    let session = create(&dir, None).unwrap();

    let entries = vec![TranscriptEntry::Message {
        message: EntryMessage {
            role: EntryRole::User,
            content: "explain this file".to_string(),
        },
        model_used: None,
        duration_ms: None,
        attachment_rel_path: Some("src/greet.py".to_string()),
        attachment_line_range: Some(LineRange {
            start_line: 3,
            end_line: 7,
        }),
        stats: None,
        sent_in_mode: None,
    }];
    save_transcript(&dir, &session.id, &entries, true).unwrap();
    let record = load(&dir, &session.id).unwrap();
    assert_eq!(record.entries, entries);
}

#[test]
fn saving_an_empty_transcript_clears_previous_entries() {
    let td = TempDir::new("clear");
    let dir = td.path().join("sessions");
    let session = create(&dir, None).unwrap();
    save_transcript(&dir, &session.id, &[user_entry("hi")], false).unwrap();
    save_transcript(&dir, &session.id, &[], false).unwrap();
    assert!(load(&dir, &session.id).unwrap().entries.is_empty());
}

// ---------------------------------------------------------------
// Wire-side rejections
// ---------------------------------------------------------------

#[test]
fn local_scope_rejects_attachment_metadata() {
    let td = TempDir::new("local-attach");
    let dir = td.path().join("sessions");
    let session = create(&dir, None).unwrap();

    let mut with_path = user_entry("hi");
    if let TranscriptEntry::Message {
        attachment_rel_path,
        ..
    } = &mut with_path
    {
        *attachment_rel_path = Some("src/greet.py".to_string());
    }
    let err = save_transcript(&dir, &session.id, &[with_path], false)
        .expect_err("local attachments refused");
    match err {
        SessionStoreError::Invalid(msg) => assert!(msg.contains("local"), "{msg}"),
        other => panic!("expected Invalid, got {other:?}"),
    }
}

#[test]
fn parse_entries_rejects_streaming_kind_and_transport_roles() {
    let streaming = json!({ "kind": "streaming", "streamId": "x", "content": "", "tokenCount": 0 });
    let err = parse_entries(&[streaming]).expect_err("streaming rejected");
    assert!(err.to_string().contains("streaming"), "{err}");

    for role in ["system", "tool"] {
        let entry = json!({ "kind": "message", "message": { "role": role, "content": "x" } });
        let err = parse_entries(&[entry]).expect_err("transport role rejected");
        assert!(err.to_string().contains(role), "{err}");
    }
}

#[test]
fn parse_entries_rejects_unknown_kind_mode_and_extra_stats_fields() {
    let unknown_kind = json!({ "kind": "banana", "message": { "role": "user", "content": "x" } });
    assert!(parse_entries(&[unknown_kind]).is_err());

    let unknown_mode = json!({
        "kind": "message",
        "message": { "role": "user", "content": "x" },
        "sentInMode": "agent"
    });
    assert!(parse_entries(&[unknown_mode]).is_err());

    let padded_stats = json!({
        "kind": "message",
        "message": { "role": "user", "content": "x" },
        "stats": { "outputTokens": 1, "smuggled": true }
    });
    assert!(parse_entries(&[padded_stats]).is_err());

    let half_range = json!({
        "kind": "message",
        "message": { "role": "user", "content": "x" },
        "attachmentRelPath": "a.py",
        "attachmentLineRange": { "startLine": 2 }
    });
    assert!(parse_entries(&[half_range]).is_err());
}

#[test]
fn save_rejects_malformed_ranges_and_paths() {
    let td = TempDir::new("ranges");
    let project_root = td.path().join("project");
    fs::create_dir_all(&project_root).unwrap();
    let dir = project_sessions_dir(&project_root).unwrap();
    let session = create(&dir, None).unwrap();

    let build = |rel: Option<&str>, range: Option<(u32, u32)>| {
        vec![TranscriptEntry::Message {
            message: EntryMessage {
                role: EntryRole::User,
                content: "x".to_string(),
            },
            model_used: None,
            duration_ms: None,
            attachment_rel_path: rel.map(str::to_string),
            attachment_line_range: range.map(|(s, e)| LineRange {
                start_line: s,
                end_line: e,
            }),
            stats: None,
            sent_in_mode: None,
        }]
    };

    for (label, entries) in [
        ("zero start", build(Some("a.py"), Some((0, 3)))),
        ("inverted", build(Some("a.py"), Some((5, 2)))),
        ("range without path", build(None, Some((1, 2)))),
        ("absolute path", build(Some("/etc/passwd"), None)),
        ("traversal", build(Some("src/../../etc/passwd"), None)),
        ("nul byte", build(Some("a\0.py"), None)),
    ] {
        let result = save_transcript(&dir, &session.id, &entries, true);
        assert!(
            matches!(result, Err(SessionStoreError::Invalid(_))),
            "{label}: expected Invalid, got {result:?}"
        );
    }
}

#[test]
fn save_rejects_oversize_entries_counts_and_transcripts() {
    let td = TempDir::new("caps");
    let dir = td.path().join("sessions");
    let session = create(&dir, None).unwrap();

    let oversize = user_entry(&"a".repeat(validation::MAX_ENTRY_CONTENT_BYTES + 1));
    assert!(matches!(
        save_transcript(&dir, &session.id, &[oversize], false),
        Err(SessionStoreError::Invalid(_))
    ));

    let too_many: Vec<TranscriptEntry> = (0..validation::MAX_TRANSCRIPT_ENTRIES + 1)
        .map(|i| user_entry(&format!("m{i}")))
        .collect();
    assert!(matches!(
        save_transcript(&dir, &session.id, &too_many, false),
        Err(SessionStoreError::Invalid(_))
    ));

    // Each entry is under the per-entry cap, but the serialized whole
    // crosses the transcript cap.
    let chunk = "a".repeat(250 * 1024);
    let bulk: Vec<TranscriptEntry> = (0..40).map(|_| user_entry(&chunk)).collect();
    let err = save_transcript(&dir, &session.id, &bulk, false).expect_err("transcript cap");
    match err {
        SessionStoreError::Invalid(msg) => assert!(msg.contains("serializes"), "{msg}"),
        other => panic!("expected Invalid, got {other:?}"),
    }
}

#[test]
fn malformed_ids_are_rejected_before_lookup() {
    let td = TempDir::new("ids");
    let dir = td.path().join("sessions");
    for bad in ["", "../../etc/passwd", "id with spaces", "id/with/slash"] {
        assert!(
            matches!(load(&dir, bad), Err(SessionStoreError::Invalid(_))),
            "id {bad:?} should be rejected"
        );
    }
    let too_long = "a".repeat(validation::MAX_ID_LEN + 1);
    assert!(matches!(
        load(&dir, &too_long),
        Err(SessionStoreError::Invalid(_))
    ));
}

// ---------------------------------------------------------------
// Atomic replacement
// ---------------------------------------------------------------

#[test]
fn failed_validation_leaves_the_previous_transcript_intact() {
    let td = TempDir::new("atomic-validate");
    let dir = td.path().join("sessions");
    let session = create(&dir, None).unwrap();
    let original = vec![user_entry("keep me")];
    save_transcript(&dir, &session.id, &original, false).unwrap();

    let mut bad = vec![user_entry("fine")];
    bad.push(user_entry(
        &"a".repeat(validation::MAX_ENTRY_CONTENT_BYTES + 1),
    ));
    assert!(save_transcript(&dir, &session.id, &bad, false).is_err());

    assert_eq!(load(&dir, &session.id).unwrap().entries, original);
}

#[cfg(unix)]
#[test]
fn failed_write_leaves_the_previous_transcript_intact() {
    use std::os::unix::fs::PermissionsExt;

    let td = TempDir::new("atomic-write");
    let dir = td.path().join("sessions");
    let session = create(&dir, None).unwrap();
    let original = vec![user_entry("keep me")];
    save_transcript(&dir, &session.id, &original, false).unwrap();
    let before = load(&dir, &session.id).unwrap();

    let db = dir.join(schema::DB_FILE_NAME);
    let writable = fs::metadata(&db).unwrap().permissions();
    fs::set_permissions(&db, fs::Permissions::from_mode(0o444)).unwrap();
    let err = save_transcript(&dir, &session.id, &[user_entry("clobber")], false)
        .expect_err("write to read-only database must fail");
    assert!(
        matches!(err, SessionStoreError::Storage(_)),
        "expected typed Storage error, got {err:?}"
    );
    fs::set_permissions(&db, writable).unwrap();

    let after = load(&dir, &session.id).unwrap();
    assert_eq!(after.entries, before.entries);
    assert_eq!(after.updated_at_ms, before.updated_at_ms);
}

// ---------------------------------------------------------------
// Reopened-database guarantees
// ---------------------------------------------------------------

#[test]
fn schema_version_holds_across_reopen_and_future_versions_are_refused() {
    let td = TempDir::new("version");
    let dir = td.path().join("sessions");
    create(&dir, None).unwrap();
    // Every public op opens a fresh connection; this second op is a
    // reopen of an existing database.
    list(&dir, false).unwrap();

    let raw = raw_conn(&dir);
    let version: i64 = raw
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap();
    assert_eq!(version, schema::SCHEMA_VERSION);

    raw.pragma_update(None, "user_version", 99).unwrap();
    drop(raw);
    let err = list(&dir, false).expect_err("future schema refused");
    match err {
        SessionStoreError::Corrupt(msg) => assert!(msg.contains("99"), "{msg}"),
        other => panic!("expected Corrupt, got {other:?}"),
    }
}

#[test]
fn malformed_persisted_rows_are_rejected_on_load() {
    let cases: [(&str, &str); 4] = [
        ("kind", "UPDATE chat_messages SET kind = 'streaming'"),
        ("role", "UPDATE chat_messages SET role = 'system'"),
        (
            "stats",
            "UPDATE chat_messages SET stats_json = '{\"smuggled\":1}'",
        ),
        ("mode", "UPDATE chat_messages SET sent_in_mode = 'agent'"),
    ];
    for (label, tamper_sql) in cases {
        let td = TempDir::new("tamper");
        let dir = td.path().join("sessions");
        let session = create(&dir, None).unwrap();
        save_transcript(&dir, &session.id, &[user_entry("hi")], false).unwrap();

        raw_conn(&dir).execute(tamper_sql, []).unwrap();
        let err = load(&dir, &session.id)
            .expect_err(&format!("tampered {label} must be rejected on load"));
        assert!(
            matches!(err, SessionStoreError::Corrupt(_)),
            "{label}: expected Corrupt, got {err:?}"
        );
    }
}

// ---------------------------------------------------------------
// Symlink refusal
// ---------------------------------------------------------------

#[cfg(unix)]
#[test]
fn symlinked_plume_sessions_dir_or_db_file_is_refused() {
    use std::os::unix::fs::symlink;

    // Planted `.plume` symlink: refused before the sessions path is
    // even built.
    let td = TempDir::new("symlink-plume");
    let root = td.path().join("project");
    let elsewhere = td.path().join("elsewhere");
    fs::create_dir_all(&root).unwrap();
    fs::create_dir_all(&elsewhere).unwrap();
    symlink(&elsewhere, root.join(".plume")).unwrap();
    assert!(matches!(
        project_sessions_dir(&root),
        Err(SessionStoreError::Refused(_))
    ));

    // Real `.plume`, symlinked `sessions` inside it.
    let td = TempDir::new("symlink-sessions");
    let root = td.path().join("project");
    let elsewhere = td.path().join("elsewhere");
    fs::create_dir_all(root.join(".plume")).unwrap();
    fs::create_dir_all(&elsewhere).unwrap();
    symlink(&elsewhere, root.join(".plume").join("sessions")).unwrap();
    let dir = project_sessions_dir(&root).unwrap();
    assert!(matches!(
        create(&dir, None),
        Err(SessionStoreError::Refused(_))
    ));

    // Real directories, symlinked database file.
    let td = TempDir::new("symlink-db");
    let dir = td.path().join("sessions");
    fs::create_dir_all(&dir).unwrap();
    let decoy = td.path().join("decoy.sqlite");
    fs::write(&decoy, b"").unwrap();
    symlink(&decoy, dir.join(schema::DB_FILE_NAME)).unwrap();
    assert!(matches!(
        create(&dir, None),
        Err(SessionStoreError::Refused(_))
    ));
}

#[cfg(unix)]
#[test]
fn hardlinked_database_file_is_refused_and_the_decoy_is_untouched() {
    let td = TempDir::new("hardlink-db");
    let dir = td.path().join("sessions");
    fs::create_dir_all(&dir).unwrap();
    // A decoy SQLite file elsewhere on the same filesystem, hardlinked
    // into place as the session database: same inode, nlink = 2. A
    // symlink check cannot see this — the planted path IS a regular
    // file — so without the link-count guard every session write would
    // land in the decoy.
    let decoy = td.path().join("decoy.sqlite");
    fs::write(&decoy, b"decoy-bytes").unwrap();
    fs::hard_link(&decoy, dir.join(schema::DB_FILE_NAME)).unwrap();

    let err = create(&dir, None).expect_err("hardlinked db must be refused");
    match &err {
        SessionStoreError::Refused(msg) => assert!(msg.contains("hardlink"), "{msg}"),
        other => panic!("expected Refused, got {other:?}"),
    }

    // The write-heavy verb refuses the same way (well-formed id so the
    // refusal provably comes from the open path, not id validation).
    let err = save_transcript(
        &dir,
        "s0000000000000000000000000000000",
        &[user_entry("x")],
        false,
    )
    .expect_err("hardlinked db must be refused");
    assert!(matches!(err, SessionStoreError::Refused(_)), "got {err:?}");

    // Nothing reached the aliased inode.
    assert_eq!(fs::read(&decoy).unwrap(), b"decoy-bytes");
}

// ---------------------------------------------------------------
// Ids
// ---------------------------------------------------------------

#[test]
fn minted_ids_are_unique_and_pass_validation() {
    let a = validation::mint_session_id();
    let b = validation::mint_session_id();
    assert_ne!(a, b);
    validation::validate_id(&a).unwrap();
    let m = validation::mint_message_id();
    validation::validate_id(&m).unwrap();
    assert!(a.starts_with('s'));
    assert!(m.starts_with('m'));
}
