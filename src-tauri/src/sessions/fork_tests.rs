//! Whole-thread fork regression matrix.

use rusqlite::params;

use super::tests::{raw_conn, user_entry, TempDir};
use super::*;

#[test]
fn empty_and_archived_sources_create_live_children() {
    let td = TempDir::new("fork-empty-archived");
    let dir = td.path().join("sessions");
    let source = create(&dir, Some("empty")).unwrap();
    set_archived(&dir, &source.id, true).unwrap();

    let child = fork(&dir, &source.id, false).unwrap();
    assert!(child.entries.is_empty());
    assert_eq!(
        child.forked_from_session_id.as_deref(),
        Some(source.id.as_str())
    );
    assert_eq!(child.forked_through_entry_id, None);
    assert_eq!(child.archived_at_ms, None);
    assert!(load(&dir, &source.id).unwrap().archived_at_ms.is_some());
}

#[test]
fn child_transcript_edits_do_not_change_source() {
    let td = TempDir::new("fork-independent");
    let dir = td.path().join("sessions");
    let source = create(&dir, Some("source")).unwrap();
    save_transcript(&dir, &source.id, &[user_entry("original")], false).unwrap();
    let child = fork(&dir, &source.id, false).unwrap();
    save_transcript(&dir, &child.id, &[user_entry("child edit")], false).unwrap();
    assert_eq!(
        load(&dir, &source.id).unwrap().entries,
        vec![user_entry("original")]
    );
    assert_eq!(
        load(&dir, &child.id).unwrap().entries,
        vec![user_entry("child edit")]
    );
}

#[test]
fn fork_preserves_attachment_stats_modes_cancelled_and_error_metadata() {
    let td = TempDir::new("fork-metadata");
    let dir = td.path().join("sessions");
    let source = create(&dir, Some("metadata")).unwrap();
    let entries = vec![
        TranscriptEntry::Message {
            message: EntryMessage {
                role: EntryRole::User,
                content: "inspect".into(),
            },
            model_used: Some("mlx-model".into()),
            duration_ms: Some(44),
            attachment_rel_path: Some("src/main.rs".into()),
            attachment_line_range: Some(LineRange {
                start_line: 2,
                end_line: 8,
            }),
            stats: Some(EntryStats {
                output_tokens: Some(3),
                eval_ms: Some(4),
                tokens_per_second: Some(5.5),
                prompt_tokens: Some(6),
                prompt_ms: Some(7),
            }),
            sent_in_mode: Some(SentMode::ProposeDiff),
            context_sources: None,
        },
        TranscriptEntry::Cancelled {
            partial: "partial".into(),
            model_used: Some("mlx-model".into()),
            duration_ms: Some(9),
        },
        TranscriptEntry::Error {
            message: "boom".into(),
        },
    ];
    save_transcript(&dir, &source.id, &entries, true).unwrap();
    assert_eq!(fork(&dir, &source.id, true).unwrap().entries, entries);
}

#[test]
fn fork_preserves_accepted_manifests_but_starts_with_an_empty_current_shelf() {
    let td = TempDir::new("fork-context");
    let dir = td.path().join("sessions");
    let source = create(&dir, Some("context source")).unwrap();
    let shelf = vec![ContextSourceRef::TopicFile {
        name: "topics/testing.md".into(),
    }];
    let entries = vec![TranscriptEntry::Message {
        message: EntryMessage {
            role: EntryRole::User,
            content: "use topic".into(),
        },
        model_used: None,
        duration_ms: None,
        attachment_rel_path: None,
        attachment_line_range: None,
        stats: None,
        sent_in_mode: Some(SentMode::Chat),
        context_sources: Some(vec![ContextSourceManifestItem::TopicFile {
            name: "topics/testing.md".into(),
            bytes: 42,
        }]),
    }];
    save_transcript_with_context(&dir, &source.id, &entries, &shelf, true).unwrap();

    let child = fork(&dir, &source.id, true).unwrap();
    assert_eq!(child.entries, entries);
    assert!(child.context_sources.is_empty());
    assert_eq!(load(&dir, &source.id).unwrap().context_sources, shelf);
}

#[test]
fn quota_failure_is_atomic_and_creates_no_child() {
    let td = TempDir::new("fork-cap");
    let dir = td.path().join("sessions");
    let source = create(&dir, Some("source")).unwrap();
    let conn = raw_conn(&dir);
    for n in 1..validation::MAX_SESSIONS {
        conn.execute(
            "INSERT INTO chat_sessions
             (id,title,created_at_ms,updated_at_ms,archived_at_ms,forked_from_session_id,forked_through_entry_id)
             VALUES (?1,'filler',1,1,NULL,NULL,NULL)",
            params![format!("s{n:032x}")],
        ).unwrap();
    }
    drop(conn);
    assert!(matches!(
        fork(&dir, &source.id, false),
        Err(SessionStoreError::Limit(_))
    ));
    assert_eq!(
        list(&dir, true).unwrap().len() as i64,
        validation::MAX_SESSIONS
    );
}

#[test]
fn corrupt_source_row_rolls_back_without_a_child() {
    let td = TempDir::new("fork-corrupt");
    let dir = td.path().join("sessions");
    let source = create(&dir, Some("source")).unwrap();
    save_transcript(&dir, &source.id, &[user_entry("ok")], false).unwrap();
    let conn = raw_conn(&dir);
    conn.execute(
        "UPDATE chat_messages SET kind='streaming' WHERE session_id=?1",
        params![source.id],
    )
    .unwrap();
    drop(conn);
    assert!(matches!(
        fork(&dir, &source.id, false),
        Err(SessionStoreError::Corrupt(_))
    ));
    assert_eq!(list(&dir, true).unwrap().len(), 1);
}

#[test]
fn malformed_source_message_ids_reject_fork_atomically() {
    for (label, bad_id) in [
        ("invalid", "message/id".to_string()),
        ("oversize", "m".repeat(validation::MAX_ID_LEN + 1)),
    ] {
        let td = TempDir::new(&format!("fork-message-id-{label}"));
        let dir = td.path().join("sessions");
        let source = create(&dir, Some("source")).unwrap();
        save_transcript(&dir, &source.id, &[user_entry("ok")], false).unwrap();
        let conn = raw_conn(&dir);
        conn.execute(
            "UPDATE chat_messages SET id=?2 WHERE session_id=?1",
            params![source.id, bad_id],
        )
        .unwrap();
        drop(conn);
        assert!(matches!(
            fork(&dir, &source.id, false),
            Err(SessionStoreError::Corrupt(_))
        ));
        assert_eq!(list(&dir, true).unwrap().len(), 1);
    }
}

#[test]
fn continued_title_is_unicode_safe_and_bounded() {
    let td = TempDir::new("fork-title");
    let dir = td.path().join("sessions");
    let long = "🫏".repeat(120);
    let source = create(&dir, Some(&long)).unwrap();
    let child = fork(&dir, &source.id, false).unwrap();
    assert!(child.title.ends_with(" (continued)"));
    assert_eq!(child.title.chars().count(), 120);
    assert!(child.title.is_char_boundary(child.title.len()));
}

#[test]
fn v2_migration_preserves_rows_and_initializes_null_lineage() {
    let td = TempDir::new("fork-v2-migrate");
    let dir = td.path();
    let conn = raw_conn(dir);
    conn.execute_batch(
        "CREATE TABLE chat_sessions (
           id TEXT PRIMARY KEY NOT NULL, title TEXT NOT NULL,
           created_at_ms INTEGER NOT NULL, updated_at_ms INTEGER NOT NULL,
           archived_at_ms INTEGER);
         CREATE TABLE chat_messages (
           id TEXT PRIMARY KEY NOT NULL, session_id TEXT NOT NULL REFERENCES chat_sessions(id) ON DELETE CASCADE,
           ordinal INTEGER NOT NULL, kind TEXT NOT NULL, role TEXT, content TEXT NOT NULL,
           model_used TEXT, duration_ms INTEGER, attachment_rel_path TEXT,
           attachment_start_line INTEGER, attachment_end_line INTEGER, stats_json TEXT,
           sent_in_mode TEXT, created_at_ms INTEGER NOT NULL, UNIQUE(session_id, ordinal));
         INSERT INTO chat_sessions VALUES ('s00000000000000000000000000000001','legacy',1,2,NULL);
         INSERT INTO chat_messages VALUES ('m00000000000000000000000000000001','s00000000000000000000000000000001',0,
           'message','user','kept',NULL,NULL,NULL,NULL,NULL,NULL,'chat',1);
         PRAGMA user_version=2;"
    ).unwrap();
    drop(conn);

    let migrated = load(dir, "s00000000000000000000000000000001").unwrap();
    assert_eq!(migrated.entries, vec![user_entry("kept")]);
    assert_eq!(migrated.forked_from_session_id, None);
    assert_eq!(migrated.forked_through_entry_id, None);
    let conn = raw_conn(dir);
    let version: i64 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap();
    assert_eq!(version, schema::SCHEMA_VERSION);
}

#[test]
fn v3_migration_loads_legacy_rows_with_empty_shelf_and_absent_manifests() {
    let td = TempDir::new("context-v3-migrate");
    let dir = td.path();
    let conn = raw_conn(dir);
    conn.execute_batch(
        "CREATE TABLE chat_sessions (
           id TEXT PRIMARY KEY NOT NULL, title TEXT NOT NULL,
           created_at_ms INTEGER NOT NULL, updated_at_ms INTEGER NOT NULL,
           archived_at_ms INTEGER, forked_from_session_id TEXT,
           forked_through_entry_id TEXT);
         CREATE TABLE chat_messages (
           id TEXT PRIMARY KEY NOT NULL, session_id TEXT NOT NULL REFERENCES chat_sessions(id) ON DELETE CASCADE,
           ordinal INTEGER NOT NULL, kind TEXT NOT NULL, role TEXT, content TEXT NOT NULL,
           model_used TEXT, duration_ms INTEGER, attachment_rel_path TEXT,
           attachment_start_line INTEGER, attachment_end_line INTEGER, stats_json TEXT,
           sent_in_mode TEXT, created_at_ms INTEGER NOT NULL, UNIQUE(session_id, ordinal));
         INSERT INTO chat_sessions VALUES (
           's00000000000000000000000000000003','legacy-v3',1,2,NULL,
           's00000000000000000000000000000001','m00000000000000000000000000000001');
         INSERT INTO chat_messages VALUES (
           'm00000000000000000000000000000003','s00000000000000000000000000000003',0,
           'message','user','legacy turn',NULL,NULL,NULL,NULL,NULL,NULL,'chat',1);
         PRAGMA user_version=3;",
    )
    .unwrap();
    drop(conn);

    let migrated = load(dir, "s00000000000000000000000000000003").unwrap();
    assert_eq!(migrated.entries, vec![user_entry("legacy turn")]);
    assert!(migrated.context_sources.is_empty());
    assert_eq!(
        migrated.forked_from_session_id.as_deref(),
        Some("s00000000000000000000000000000001")
    );
    assert_eq!(
        migrated.forked_through_entry_id.as_deref(),
        Some("m00000000000000000000000000000001")
    );
    let conn = raw_conn(dir);
    let context_json: Option<String> = conn
        .query_row(
            "SELECT context_sources_json FROM chat_sessions WHERE id=?1",
            params!["s00000000000000000000000000000003"],
            |row| row.get(0),
        )
        .unwrap();
    let manifest_json: Option<String> = conn
        .query_row(
            "SELECT context_manifest_json FROM chat_messages WHERE session_id=?1",
            params!["s00000000000000000000000000000003"],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(context_json, None);
    assert_eq!(manifest_json, None);
}

fn raw_message(
    conn: &rusqlite::Connection,
    session: &str,
    ordinal: i64,
    content: &str,
    attachment: Option<&str>,
) {
    conn.execute(
        "INSERT INTO chat_messages
         (id,session_id,ordinal,kind,role,content,model_used,duration_ms,attachment_rel_path,
          attachment_start_line,attachment_end_line,stats_json,sent_in_mode,created_at_ms)
         VALUES (?1,?2,?3,'message','user',?4,NULL,NULL,?5,NULL,NULL,NULL,'chat',1)",
        params![
            format!("m{ordinal:032x}"),
            session,
            ordinal,
            content,
            attachment
        ],
    )
    .unwrap();
}

#[test]
fn over_500_persisted_rows_are_corrupt_and_create_no_child() {
    let td = TempDir::new("fork-row-cap");
    let dir = td.path().join("sessions");
    let source = create(&dir, Some("source")).unwrap();
    let conn = raw_conn(&dir);
    for ordinal in 0..=500 {
        raw_message(&conn, &source.id, ordinal, "x", None);
    }
    drop(conn);
    assert!(matches!(
        fork(&dir, &source.id, false),
        Err(SessionStoreError::Corrupt(_))
    ));
    assert_eq!(list(&dir, true).unwrap().len(), 1);
}

#[test]
fn aggregate_oversize_persisted_rows_are_corrupt_and_create_no_child() {
    let td = TempDir::new("fork-byte-cap");
    let dir = td.path().join("sessions");
    let source = create(&dir, Some("source")).unwrap();
    let conn = raw_conn(&dir);
    let chunk = "x".repeat(220 * 1024);
    for ordinal in 0..40 {
        raw_message(&conn, &source.id, ordinal, &chunk, None);
    }
    drop(conn);
    assert!(matches!(
        fork(&dir, &source.id, false),
        Err(SessionStoreError::Corrupt(_))
    ));
    assert_eq!(list(&dir, true).unwrap().len(), 1);
}

#[test]
fn local_attachment_in_persisted_source_is_corrupt_and_creates_no_child() {
    let td = TempDir::new("fork-local-attachment");
    let dir = td.path().join("sessions");
    let source = create(&dir, Some("source")).unwrap();
    let conn = raw_conn(&dir);
    raw_message(&conn, &source.id, 0, "x", Some("src/main.rs"));
    drop(conn);
    assert!(matches!(
        fork(&dir, &source.id, false),
        Err(SessionStoreError::Corrupt(_))
    ));
    assert_eq!(list(&dir, true).unwrap().len(), 1);
}

#[test]
fn point_in_time_lineage_survives_source_resave_and_delete() {
    let td = TempDir::new("fork-lineage-point");
    let dir = td.path().join("sessions");
    let source = create(&dir, Some("source")).unwrap();
    save_transcript(&dir, &source.id, &[user_entry("original")], false).unwrap();
    let child = fork(&dir, &source.id, false).unwrap();
    let through = child.forked_through_entry_id.clone();
    save_transcript(&dir, &source.id, &[user_entry("replacement")], false).unwrap();
    assert_eq!(
        load(&dir, &child.id).unwrap().forked_through_entry_id,
        through
    );
    delete(&dir, &source.id).unwrap();
    let durable = load(&dir, &child.id).unwrap();
    assert_eq!(
        durable.forked_from_session_id.as_deref(),
        Some(source.id.as_str())
    );
    assert_eq!(durable.entries, vec![user_entry("original")]);
}
