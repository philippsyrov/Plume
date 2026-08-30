//! Durable compaction-checkpoint store regressions.

use rusqlite::params;

use super::checkpoint::{
    latest_valid_checkpoint, list_checkpoints, save_checkpoint, save_checkpoint_with_cap,
    CheckpointFact, CheckpointValidationStatus, CompactionCheckpoint, FactKind, FactProvenance,
};
use super::tests::{assistant_entry, raw_conn, user_entry, TempDir};
use super::*;

fn table_exists(conn: &rusqlite::Connection, name: &str) -> bool {
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
        params![name],
        |row| row.get(0),
    )
    .expect("inspect table")
}

fn message_ids(sessions_dir: &std::path::Path, session_id: &str) -> Vec<String> {
    let conn = raw_conn(sessions_dir);
    let mut stmt = conn
        .prepare("SELECT id FROM chat_messages WHERE session_id=?1 ORDER BY ordinal")
        .unwrap();
    stmt.query_map(params![session_id], |row| row.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

fn checkpoint(
    id: &str,
    session_id: &str,
    through_entry_id: &str,
    first_retained_entry_id: &str,
    created_at_ms: i64,
    status: CheckpointValidationStatus,
    supersedes_checkpoint_id: Option<&str>,
) -> CompactionCheckpoint {
    CompactionCheckpoint {
        id: id.to_string(),
        session_id: session_id.to_string(),
        through_entry_id: through_entry_id.to_string(),
        first_retained_entry_id: first_retained_entry_id.to_string(),
        summary: "User is fixing durable compaction.".to_string(),
        facts: vec![CheckpointFact {
            kind: FactKind::Goal,
            text: "Ship compaction safely".to_string(),
            provenance: FactProvenance {
                source_turn_ids: vec![through_entry_id.to_string()],
                memory_entry: None,
            },
        }],
        accepted_source_manifest_ids: Vec::new(),
        model_id: "qwen-local".to_string(),
        runtime_id: "mlx-lm".to_string(),
        prompt_version: "compaction-v1".to_string(),
        tokens_before: 8_000,
        tokens_after: 1_200,
        created_at_ms,
        supersedes_checkpoint_id: supersedes_checkpoint_id.map(str::to_string),
        validation_status: status,
    }
}

fn session_with_two_turns(
    label: &str,
) -> (TempDir, std::path::PathBuf, SessionSummary, Vec<String>) {
    let td = TempDir::new(label);
    let dir = td.path().join("sessions");
    let session = create(&dir, Some("Checkpoint owner")).unwrap();
    let mut first_user = user_entry("question");
    let TranscriptEntry::Message {
        context_sources, ..
    } = &mut first_user
    else {
        unreachable!()
    };
    *context_sources = Some(vec![
        crate::prompts::ContextSourceManifestItem::UserMemoryEntry {
            entry_id: "m_0123456789abcdef0123456789abcdef".to_string(),
            created_at_ms: 1,
            bytes: 4,
            preview: "pref".to_string(),
        },
    ]);
    save_transcript(
        &dir,
        &session.id,
        &[
            first_user,
            assistant_entry("answer"),
            user_entry("follow-up"),
            assistant_entry("later answer"),
        ],
        false,
    )
    .unwrap();
    let ids = message_ids(&dir, &session.id);
    (td, dir, session, ids)
}

#[test]
fn v7_migration_preserves_sessions_and_adds_an_empty_checkpoint_store() {
    let td = TempDir::new("checkpoint-v7-migrate");
    let dir = td.path();
    let conn = raw_conn(dir);
    conn.execute_batch(
        "CREATE TABLE chat_sessions (
           id TEXT PRIMARY KEY NOT NULL, title TEXT NOT NULL,
           created_at_ms INTEGER NOT NULL, updated_at_ms INTEGER NOT NULL,
           archived_at_ms INTEGER, forked_from_session_id TEXT,
           forked_through_entry_id TEXT, context_sources_json TEXT,
           is_home INTEGER NOT NULL DEFAULT 0);
         CREATE TABLE chat_messages (
           id TEXT PRIMARY KEY NOT NULL,
           session_id TEXT NOT NULL REFERENCES chat_sessions(id) ON DELETE CASCADE,
           ordinal INTEGER NOT NULL, kind TEXT NOT NULL, role TEXT, content TEXT NOT NULL,
           model_used TEXT, duration_ms INTEGER, attachment_rel_path TEXT,
           attachment_start_line INTEGER, attachment_end_line INTEGER, stats_json TEXT,
           sent_in_mode TEXT, context_manifest_json TEXT, artifact_json TEXT,
           created_at_ms INTEGER NOT NULL, UNIQUE(session_id, ordinal));
         INSERT INTO chat_sessions VALUES (
           's00000000000000000000000000000007','legacy-v7',1,2,NULL,
           NULL,NULL,NULL,0);
         INSERT INTO chat_messages VALUES (
           'm00000000000000000000000000000007','s00000000000000000000000000000007',
           0,'message','user','kept through migration',NULL,NULL,NULL,NULL,NULL,NULL,
           'chat',NULL,NULL,1);
         PRAGMA user_version=7;",
    )
    .expect("build real v7 fixture");
    drop(conn);

    let migrated = load(dir, "s00000000000000000000000000000007").expect("load migrated session");
    assert_eq!(migrated.entries.len(), 1);

    let conn = raw_conn(dir);
    let version: i64 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .expect("read migrated version");
    assert_eq!(version, 8);
    assert!(table_exists(&conn, "compaction_checkpoints"));
    let rows: i64 = conn
        .query_row("SELECT COUNT(*) FROM compaction_checkpoints", [], |row| {
            row.get(0)
        })
        .expect("count checkpoint rows");
    assert_eq!(rows, 0, "migration must not invent derived state");
}

#[test]
fn checkpoint_round_trip_is_typed_and_rows_cannot_be_rewritten() {
    let (_td, dir, session, ids) = session_with_two_turns("checkpoint-roundtrip");
    let stored = checkpoint(
        "c00000000000000000000000000000001",
        &session.id,
        &ids[1],
        &ids[2],
        10,
        CheckpointValidationStatus::Valid,
        None,
    );

    save_checkpoint(&dir, &stored).unwrap();
    assert_eq!(list_checkpoints(&dir, &session.id).unwrap(), vec![stored]);

    let conn = raw_conn(&dir);
    let rewrite = conn.execute(
        "UPDATE compaction_checkpoints SET payload_json='{}' WHERE id=?1",
        params!["c00000000000000000000000000000001"],
    );
    assert!(rewrite.is_err(), "a checkpoint row must be immutable");
    let erase = conn.execute(
        "DELETE FROM compaction_checkpoints WHERE id=?1",
        params!["c00000000000000000000000000000001"],
    );
    assert!(erase.is_err(), "only deleting the owner may erase history");
}

#[test]
fn latest_valid_checkpoint_skips_a_newer_invalid_attempt() {
    let (_td, dir, session, ids) = session_with_two_turns("checkpoint-latest-valid");
    let valid = checkpoint(
        "c00000000000000000000000000000001",
        &session.id,
        &ids[1],
        &ids[2],
        10,
        CheckpointValidationStatus::Valid,
        None,
    );
    let invalid = checkpoint(
        "c00000000000000000000000000000002",
        &session.id,
        &ids[1],
        &ids[2],
        20,
        CheckpointValidationStatus::Invalid,
        Some(&valid.id),
    );
    save_checkpoint(&dir, &valid).unwrap();
    save_checkpoint(&dir, &invalid).unwrap();

    assert_eq!(
        latest_valid_checkpoint(&dir, &session.id).unwrap(),
        Some(valid)
    );
    assert_eq!(list_checkpoints(&dir, &session.id).unwrap().len(), 2);
}

#[test]
fn malformed_checkpoint_payload_is_refused_instead_of_coerced() {
    let (_td, dir, session, ids) = session_with_two_turns("checkpoint-corrupt");
    let conn = raw_conn(&dir);
    conn.execute(
        "INSERT INTO compaction_checkpoints
         (id,session_id,through_entry_id,first_retained_entry_id,payload_json,
          validation_status,created_at_ms,supersedes_checkpoint_id)
         VALUES (?1,?2,?3,?4,'{}','valid',10,NULL)",
        params![
            "c00000000000000000000000000000001",
            session.id,
            ids[1],
            ids[2]
        ],
    )
    .unwrap();
    drop(conn);

    let err = list_checkpoints(&dir, &session.id).expect_err("malformed JSON refused");
    assert!(matches!(err, SessionStoreError::Corrupt(_)));
}

#[test]
fn deleting_a_session_cascades_to_its_checkpoint_history() {
    let (_td, dir, session, ids) = session_with_two_turns("checkpoint-cascade");
    let stored = checkpoint(
        "c00000000000000000000000000000001",
        &session.id,
        &ids[1],
        &ids[2],
        10,
        CheckpointValidationStatus::Valid,
        None,
    );
    save_checkpoint(&dir, &stored).unwrap();

    delete(&dir, &session.id).unwrap();

    let conn = raw_conn(&dir);
    let rows: i64 = conn
        .query_row("SELECT COUNT(*) FROM compaction_checkpoints", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(rows, 0);
}

#[test]
fn checkpoint_facts_require_source_turns_owned_by_the_same_session() {
    let (_td, dir, session, ids) = session_with_two_turns("checkpoint-fact-owner");
    let other = create(&dir, Some("Other conversation")).unwrap();
    save_transcript(&dir, &other.id, &[user_entry("foreign")], false).unwrap();
    let foreign_id = message_ids(&dir, &other.id).remove(0);

    let mut anchorless = checkpoint(
        "c00000000000000000000000000000001",
        &session.id,
        &ids[1],
        &ids[2],
        10,
        CheckpointValidationStatus::Valid,
        None,
    );
    anchorless.facts[0].provenance.source_turn_ids.clear();
    assert!(matches!(
        save_checkpoint(&dir, &anchorless),
        Err(SessionStoreError::Invalid(_))
    ));

    let mut foreign = checkpoint(
        "c00000000000000000000000000000002",
        &session.id,
        &ids[1],
        &ids[2],
        20,
        CheckpointValidationStatus::Valid,
        None,
    );
    foreign.facts[0].provenance.source_turn_ids = vec![foreign_id];
    assert!(matches!(
        save_checkpoint(&dir, &foreign),
        Err(SessionStoreError::Invalid(_))
    ));
}

#[test]
fn checkpoint_payload_is_bounded_before_it_reaches_sqlite() {
    let (_td, dir, session, ids) = session_with_two_turns("checkpoint-payload-cap");
    let mut oversized = checkpoint(
        "c00000000000000000000000000000001",
        &session.id,
        &ids[1],
        &ids[2],
        10,
        CheckpointValidationStatus::Valid,
        None,
    );
    oversized.summary = "x".repeat(1024 * 1024 + 1);

    assert!(matches!(
        save_checkpoint(&dir, &oversized),
        Err(SessionStoreError::Limit(_))
    ));
    let conn = raw_conn(&dir);
    let rows: i64 = conn
        .query_row("SELECT COUNT(*) FROM compaction_checkpoints", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(rows, 0);
}

#[test]
fn checkpoint_boundaries_keep_complete_adjacent_turns() {
    let (_td, dir, session, ids) = session_with_two_turns("checkpoint-boundaries");
    let split_pair = checkpoint(
        "c00000000000000000000000000000001",
        &session.id,
        &ids[0],
        &ids[1],
        10,
        CheckpointValidationStatus::Valid,
        None,
    );
    assert!(matches!(
        save_checkpoint(&dir, &split_pair),
        Err(SessionStoreError::Invalid(_))
    ));

    let skipped_entry = checkpoint(
        "c00000000000000000000000000000002",
        &session.id,
        &ids[0],
        &ids[2],
        20,
        CheckpointValidationStatus::Valid,
        None,
    );
    assert!(matches!(
        save_checkpoint(&dir, &skipped_entry),
        Err(SessionStoreError::Invalid(_))
    ));
}

#[test]
fn accepted_manifest_ids_resolve_to_summarized_turns_in_the_owner() {
    let (_td, dir, session, ids) = session_with_two_turns("checkpoint-manifests");
    let mut valid = checkpoint(
        "c00000000000000000000000000000001",
        &session.id,
        &ids[1],
        &ids[2],
        10,
        CheckpointValidationStatus::Valid,
        None,
    );
    valid.accepted_source_manifest_ids = vec![ids[0].clone()];
    save_checkpoint(&dir, &valid).unwrap();

    let mut outside_boundary = checkpoint(
        "c00000000000000000000000000000002",
        &session.id,
        &ids[1],
        &ids[2],
        20,
        CheckpointValidationStatus::Invalid,
        Some(&valid.id),
    );
    outside_boundary.accepted_source_manifest_ids = vec![ids[2].clone()];
    assert!(matches!(
        save_checkpoint(&dir, &outside_boundary),
        Err(SessionStoreError::Invalid(_))
    ));
}

#[test]
fn checkpoint_append_rolls_back_when_real_store_usage_crosses_the_cap() {
    let (_td, dir, session, ids) = session_with_two_turns("checkpoint-store-cap");
    let before = storage_usage(&dir).unwrap();
    let mut stored = checkpoint(
        "c00000000000000000000000000000001",
        &session.id,
        &ids[1],
        &ids[2],
        10,
        CheckpointValidationStatus::Valid,
        None,
    );
    stored.summary = "x".repeat(512 * 1024);

    assert!(matches!(
        save_checkpoint_with_cap(&dir, &stored, before.used_bytes.saturating_add(4095)),
        Err(SessionStoreError::StorageFull { .. })
    ));
    assert!(list_checkpoints(&dir, &session.id).unwrap().is_empty());
}
