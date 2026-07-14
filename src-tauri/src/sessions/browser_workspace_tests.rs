//! Browser-workspace schema and store regressions.
//!
//! The Browser workspace belongs to a persisted chat session in the
//! same physically-separated local or project database. These tests
//! begin at the SQLite boundary so migrations and cascade ownership
//! cannot silently drift behind a friendly Rust mapper.

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
fn v4_migration_preserves_chat_state_and_starts_without_browser_rows() {
    let td = TempDir::new("browser-v4-migrate");
    let dir = td.path();
    let conn = raw_conn(dir);
    conn.execute_batch(
        "CREATE TABLE chat_sessions (
           id TEXT PRIMARY KEY NOT NULL, title TEXT NOT NULL,
           created_at_ms INTEGER NOT NULL, updated_at_ms INTEGER NOT NULL,
           archived_at_ms INTEGER, forked_from_session_id TEXT,
           forked_through_entry_id TEXT, context_sources_json TEXT);
         CREATE TABLE chat_messages (
           id TEXT PRIMARY KEY NOT NULL,
           session_id TEXT NOT NULL REFERENCES chat_sessions(id) ON DELETE CASCADE,
           ordinal INTEGER NOT NULL, kind TEXT NOT NULL, role TEXT, content TEXT NOT NULL,
           model_used TEXT, duration_ms INTEGER, attachment_rel_path TEXT,
           attachment_start_line INTEGER, attachment_end_line INTEGER, stats_json TEXT,
           sent_in_mode TEXT, context_manifest_json TEXT, created_at_ms INTEGER NOT NULL,
           UNIQUE(session_id, ordinal));
         INSERT INTO chat_sessions VALUES (
           's00000000000000000000000000000004','legacy-v4',1,2,NULL,
           's00000000000000000000000000000001','m00000000000000000000000000000001',
           '[{\"kind\":\"topicFile\",\"name\":\"topics/testing.md\"}]');
         INSERT INTO chat_messages VALUES (
           'm00000000000000000000000000000004','s00000000000000000000000000000004',0,
           'message','user','kept through migration',NULL,NULL,NULL,NULL,NULL,NULL,'chat',
           '[{\"kind\":\"topicFile\",\"name\":\"topics/testing.md\",\"bytes\":42}]',1);
         PRAGMA user_version=4;",
    )
    .expect("build real v4 fixture");
    drop(conn);

    let migrated = load(dir, "s00000000000000000000000000000004").expect("load migrated chat");
    assert_eq!(migrated.entries.len(), 1);
    let TranscriptEntry::Message {
        message,
        context_sources,
        ..
    } = &migrated.entries[0]
    else {
        panic!("v4 user message changed variant during migration");
    };
    assert_eq!(message.content, "kept through migration");
    assert_eq!(context_sources.as_ref().map(Vec::len), Some(1));
    assert_eq!(
        migrated.forked_from_session_id.as_deref(),
        Some("s00000000000000000000000000000001")
    );
    assert_eq!(
        migrated.forked_through_entry_id.as_deref(),
        Some("m00000000000000000000000000000001")
    );
    assert_eq!(migrated.context_sources.len(), 1);

    let conn = raw_conn(dir);
    let version: i64 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .expect("read migrated version");
    assert_eq!(version, 5);
    for table in ["browser_workspaces", "browser_tabs", "browser_history"] {
        assert!(table_exists(&conn, table), "missing {table}");
        let rows: i64 = conn
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                row.get(0)
            })
            .expect("count empty browser table");
        assert_eq!(rows, 0, "migration invented rows in {table}");
    }
}

#[test]
fn fresh_schema_pins_browser_uniqueness_order_and_delete_cascade() {
    let td = TempDir::new("browser-fresh-schema");
    let dir = td.path().join("sessions");
    let session = create(&dir, Some("Browser owner")).expect("create owner session");
    let conn = raw_conn(&dir);

    let version: i64 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .expect("read schema version");
    assert_eq!(version, 5);
    conn.execute(
        "INSERT INTO browser_workspaces
         (session_id,layout_mode,split_width_px,active_tab_id,updated_at_ms)
         VALUES (?1,'split',560,NULL,10)",
        params![session.id],
    )
    .expect("insert workspace");
    conn.execute(
        "INSERT INTO browser_tabs
         (id,session_id,position,current_history_index,manual_reopen_required)
         VALUES ('bt_first',?1,0,1,0)",
        params![session.id],
    )
    .expect("insert first tab");
    conn.execute(
        "INSERT INTO browser_tabs
         (id,session_id,position,current_history_index,manual_reopen_required)
         VALUES ('bt_second',?1,1,0,1)",
        params![session.id],
    )
    .expect("insert second tab");
    conn.execute(
        "UPDATE browser_workspaces SET active_tab_id='bt_second' WHERE session_id=?1",
        params![session.id],
    )
    .expect("select active tab");
    conn.execute(
        "INSERT INTO browser_history (tab_id,position,url,recorded_at_ms)
         VALUES ('bt_first',0,'https://example.com/one',10),
                ('bt_first',1,'https://example.com/two',20),
                ('bt_second',0,'http://localhost:3000/',30)",
        [],
    )
    .expect("insert ordered history");

    let duplicate_tab_position = conn.execute(
        "INSERT INTO browser_tabs
         (id,session_id,position,current_history_index,manual_reopen_required)
         VALUES ('bt_duplicate',?1,1,0,0)",
        params![session.id],
    );
    assert!(
        duplicate_tab_position.is_err(),
        "tab positions must be unique per session"
    );
    let duplicate_history_position = conn.execute(
        "INSERT INTO browser_history (tab_id,position,url,recorded_at_ms)
         VALUES ('bt_first',1,'https://example.com/duplicate',40)",
        [],
    );
    assert!(
        duplicate_history_position.is_err(),
        "history positions must be unique per tab"
    );
    drop(conn);

    delete(&dir, &session.id).expect("delete owning session");
    let conn = raw_conn(&dir);
    for table in ["browser_workspaces", "browser_tabs", "browser_history"] {
        let rows: i64 = conn
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                row.get(0)
            })
            .expect("count cascaded browser rows");
        assert_eq!(rows, 0, "delete left owned rows in {table}");
    }
}

#[test]
fn browser_domain_contract_has_stable_ids_bounds_and_camel_case_wire_shape() {
    use super::browser_workspace::{
        mint_tab_id, mint_workspace_id, validate_split_width_px, BrowserHistoryRecord,
        BrowserLayoutMode, BrowserRestorationStatus, BrowserTabRecord, BrowserWorkspaceRecord,
        BrowserWorkspaceScope, MAX_SPLIT_WIDTH_PX, MIN_SPLIT_WIDTH_PX,
    };

    let workspace_id = mint_workspace_id();
    let tab_id = mint_tab_id();
    assert!(workspace_id.starts_with("bw_"));
    assert!(tab_id.starts_with("bt_"));
    assert_ne!(mint_tab_id(), tab_id);
    assert_eq!(
        validate_split_width_px(MIN_SPLIT_WIDTH_PX).unwrap(),
        MIN_SPLIT_WIDTH_PX
    );
    assert_eq!(
        validate_split_width_px(MAX_SPLIT_WIDTH_PX).unwrap(),
        MAX_SPLIT_WIDTH_PX
    );
    assert!(validate_split_width_px(MIN_SPLIT_WIDTH_PX - 1).is_err());
    assert!(validate_split_width_px(MAX_SPLIT_WIDTH_PX + 1).is_err());

    let record = BrowserWorkspaceRecord {
        session_id: "s00000000000000000000000000000005".into(),
        scope: BrowserWorkspaceScope::Project,
        layout_mode: BrowserLayoutMode::Expanded,
        split_width_px: 560,
        active_tab_id: Some(tab_id.clone()),
        tabs: vec![BrowserTabRecord {
            id: tab_id,
            position: 0,
            current_history_index: 0,
            manual_reopen_required: true,
            restoration_status: BrowserRestorationStatus::ManualReopenRequired,
            history: vec![BrowserHistoryRecord {
                position: 0,
                url: "https://example.com/private".into(),
                recorded_at_ms: 42,
            }],
        }],
        recovery: None,
    };
    let value = serde_json::to_value(record).expect("serialize browser workspace");
    assert_eq!(value["sessionId"], "s00000000000000000000000000000005");
    assert_eq!(value["scope"], "project");
    assert_eq!(value["layoutMode"], "expanded");
    assert_eq!(value["splitWidthPx"], 560);
    assert_eq!(value["tabs"][0]["currentHistoryIndex"], 0);
    assert_eq!(value["tabs"][0]["manualReopenRequired"], true);
    assert_eq!(
        value["tabs"][0]["restorationStatus"],
        "manualReopenRequired"
    );
}
