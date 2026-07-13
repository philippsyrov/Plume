//! Non-destructive conversation rewind regression matrix.

use rusqlite::params;

use super::tests::{raw_conn, user_entry, TempDir};
use super::*;

fn assistant(content: &str) -> TranscriptEntry {
    TranscriptEntry::Message {
        message: EntryMessage {
            role: EntryRole::Assistant,
            content: content.into(),
        },
        model_used: None,
        duration_ms: None,
        attachment_rel_path: None,
        attachment_line_range: None,
        stats: None,
        sent_in_mode: None,
        context_sources: None,
    }
}

#[test]
fn rewinds_one_two_and_all_complete_user_turns() {
    let td = TempDir::new("rollback-turns");
    let dir = td.path().join("sessions");
    let source = create(&dir, Some("thread")).unwrap();
    let entries = vec![
        user_entry("u1"),
        assistant("a1"),
        user_entry("u2"),
        TranscriptEntry::Cancelled {
            partial: "partial".into(),
            model_used: None,
            duration_ms: None,
        },
        TranscriptEntry::Error {
            message: "failed".into(),
        },
        user_entry("u3"),
        assistant("a3"),
    ];
    save_transcript(&dir, &source.id, &entries, false).unwrap();
    assert_eq!(
        rollback(&dir, &source.id, 1, false).unwrap().entries,
        entries[..5]
    );
    assert_eq!(
        rollback(&dir, &source.id, 2, false).unwrap().entries,
        entries[..2]
    );
    let empty = rollback(&dir, &source.id, 3, false).unwrap();
    assert!(empty.entries.is_empty());
    assert_eq!(empty.forked_through_entry_id, None);
    assert_eq!(load(&dir, &source.id).unwrap().entries, entries);
}

#[test]
fn rewind_lineage_ends_at_last_retained_source_row_and_child_is_independent() {
    let td = TempDir::new("rollback-lineage");
    let dir = td.path().join("sessions");
    let source = create(&dir, Some("source")).unwrap();
    save_transcript(
        &dir,
        &source.id,
        &[
            user_entry("u1"),
            assistant("a1"),
            user_entry("u2"),
            assistant("a2"),
        ],
        false,
    )
    .unwrap();
    let conn = raw_conn(&dir);
    let retained_id: String = conn
        .query_row(
            "SELECT id FROM chat_messages WHERE session_id=?1 AND ordinal=1",
            params![source.id],
            |row| row.get(0),
        )
        .unwrap();
    drop(conn);
    let child = rollback(&dir, &source.id, 1, false).unwrap();
    assert_eq!(
        child.forked_from_session_id.as_deref(),
        Some(source.id.as_str())
    );
    assert_eq!(
        child.forked_through_entry_id.as_deref(),
        Some(retained_id.as_str())
    );
    save_transcript(&dir, &child.id, &[user_entry("changed")], false).unwrap();
    assert_eq!(load(&dir, &source.id).unwrap().entries.len(), 4);
}

#[test]
fn rewind_preserves_retained_manifests_but_starts_with_an_empty_current_shelf() {
    let td = TempDir::new("rollback-context");
    let dir = td.path().join("sessions");
    let source = create(&dir, Some("context source")).unwrap();
    let shelf = vec![ContextSourceRef::TopicFile {
        name: "topics/testing.md".into(),
    }];
    let mut first = user_entry("u1");
    if let TranscriptEntry::Message {
        context_sources, ..
    } = &mut first
    {
        *context_sources = Some(vec![ContextSourceManifestItem::TopicFile {
            name: "topics/testing.md".into(),
            bytes: 42,
        }]);
    }
    let entries = vec![
        first.clone(),
        assistant("a1"),
        user_entry("u2"),
        assistant("a2"),
    ];
    save_transcript_with_context(&dir, &source.id, &entries, &shelf, true).unwrap();

    let child = rollback(&dir, &source.id, 1, true).unwrap();
    assert_eq!(child.entries, vec![first, assistant("a1")]);
    assert!(child.context_sources.is_empty());
    assert_eq!(load(&dir, &source.id).unwrap().context_sources, shelf);
}

#[test]
fn archived_source_and_unicode_bounded_title_are_supported() {
    let td = TempDir::new("rollback-archived-title");
    let dir = td.path().join("sessions");
    let source = create(&dir, Some(&"🫏".repeat(120))).unwrap();
    save_transcript(&dir, &source.id, &[user_entry("u")], false).unwrap();
    set_archived(&dir, &source.id, true).unwrap();
    let child = rollback(&dir, &source.id, 1, false).unwrap();
    assert_eq!(child.title.chars().count(), 120);
    assert!(child.title.ends_with(" (rewound 1)"));
    assert_eq!(child.archived_at_ms, None);
}

#[test]
fn rejects_invalid_or_insufficient_turn_counts_without_a_child() {
    let td = TempDir::new("rollback-counts");
    let dir = td.path().join("sessions");
    let source = create(&dir, Some("source")).unwrap();
    save_transcript(&dir, &source.id, &[user_entry("u")], false).unwrap();
    for count in [0, 2, 21] {
        assert!(matches!(
            rollback(&dir, &source.id, count, false),
            Err(SessionStoreError::Invalid(_))
        ));
    }
    assert_eq!(list(&dir, true).unwrap().len(), 1);
}

#[test]
fn empty_or_leading_non_user_transcripts_are_deliberately_rejected() {
    let td = TempDir::new("rollback-no-turn");
    let dir = td.path().join("sessions");
    let empty = create(&dir, Some("empty")).unwrap();
    assert!(matches!(
        rollback(&dir, &empty.id, 1, false),
        Err(SessionStoreError::Invalid(_))
    ));
    let preamble = create(&dir, Some("preamble")).unwrap();
    save_transcript(
        &dir,
        &preamble.id,
        &[assistant("impossible"), user_entry("u")],
        false,
    )
    .unwrap();
    assert!(matches!(
        rollback(&dir, &preamble.id, 1, false),
        Err(SessionStoreError::Corrupt(_))
    ));
    assert_eq!(list(&dir, true).unwrap().len(), 2);
}

#[test]
fn corrupt_or_local_attachment_sources_roll_back_atomically() {
    let td = TempDir::new("rollback-hostile");
    let dir = td.path().join("sessions");
    let source = create(&dir, Some("source")).unwrap();
    save_transcript(&dir, &source.id, &[user_entry("u")], false).unwrap();
    let conn = raw_conn(&dir);
    conn.execute(
        "UPDATE chat_messages SET attachment_rel_path='src/main.rs' WHERE session_id=?1",
        params![source.id],
    )
    .unwrap();
    drop(conn);
    assert!(matches!(
        rollback(&dir, &source.id, 1, false),
        Err(SessionStoreError::Corrupt(_))
    ));
    assert_eq!(list(&dir, true).unwrap().len(), 1);
}

#[test]
fn malformed_source_message_ids_reject_rewind_atomically() {
    for (label, bad_id) in [
        ("invalid", "message/id".to_string()),
        ("oversize", "m".repeat(validation::MAX_ID_LEN + 1)),
    ] {
        let td = TempDir::new(&format!("rollback-message-id-{label}"));
        let dir = td.path().join("sessions");
        let source = create(&dir, Some("source")).unwrap();
        save_transcript(&dir, &source.id, &[user_entry("u")], false).unwrap();
        let conn = raw_conn(&dir);
        conn.execute(
            "UPDATE chat_messages SET id=?2 WHERE session_id=?1",
            params![source.id, bad_id],
        )
        .unwrap();
        drop(conn);
        assert!(matches!(
            rollback(&dir, &source.id, 1, false),
            Err(SessionStoreError::Corrupt(_))
        ));
        assert_eq!(list(&dir, true).unwrap().len(), 1);
    }
}

#[test]
fn session_cap_rejects_rewind_without_creating_a_child() {
    let td = TempDir::new("rollback-cap");
    let dir = td.path().join("sessions");
    let source = create(&dir, Some("source")).unwrap();
    save_transcript(&dir, &source.id, &[user_entry("u")], false).unwrap();
    let conn = raw_conn(&dir);
    for n in 1..validation::MAX_SESSIONS {
        conn.execute(
            "INSERT INTO chat_sessions
             (id,title,created_at_ms,updated_at_ms,archived_at_ms,
              forked_from_session_id,forked_through_entry_id)
             VALUES (?1,'filler',1,1,NULL,NULL,NULL)",
            params![format!("s{n:032x}")],
        )
        .unwrap();
    }
    drop(conn);
    assert!(matches!(
        rollback(&dir, &source.id, 1, false),
        Err(SessionStoreError::Limit(_))
    ));
    assert_eq!(
        list(&dir, true).unwrap().len() as i64,
        validation::MAX_SESSIONS
    );
}

#[test]
fn oversized_persisted_source_is_validated_before_tail_is_removed() {
    let td = TempDir::new("rollback-transcript-cap");
    let dir = td.path().join("sessions");
    let source = create(&dir, Some("source")).unwrap();
    let conn = raw_conn(&dir);
    for ordinal in 0..=500 {
        conn.execute(
            "INSERT INTO chat_messages
             (id,session_id,ordinal,kind,role,content,model_used,duration_ms,
              attachment_rel_path,attachment_start_line,attachment_end_line,
              stats_json,sent_in_mode,created_at_ms)
             VALUES (?1,?2,?3,'message','user','x',NULL,NULL,NULL,NULL,NULL,NULL,'chat',1)",
            params![format!("m{ordinal:032x}"), source.id, ordinal],
        )
        .unwrap();
    }
    drop(conn);
    assert!(matches!(
        rollback(&dir, &source.id, 1, false),
        Err(SessionStoreError::Corrupt(_))
    ));
    assert_eq!(list(&dir, true).unwrap().len(), 1);
}
