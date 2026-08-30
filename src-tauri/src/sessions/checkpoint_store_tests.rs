//! Durable compaction-checkpoint store regressions.

use rusqlite::params;

use super::tests::{raw_conn, TempDir};
use super::*;

fn table_exists(conn: &rusqlite::Connection, name: &str) -> bool {
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
        params![name],
        |row| row.get(0),
    )
    .expect("inspect table")
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
