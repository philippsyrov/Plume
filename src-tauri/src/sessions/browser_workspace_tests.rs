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
    assert_eq!(version, 7);
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
    assert_eq!(version, 7);
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
            current_history_index: Some(0),
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

fn workspace_for(
    session_id: &str,
    scope: browser_workspace::BrowserWorkspaceScope,
) -> browser_workspace::BrowserWorkspaceRecord {
    use browser_workspace::{
        mint_tab_id, BrowserHistoryRecord, BrowserLayoutMode, BrowserRestorationStatus,
        BrowserTabRecord, BrowserWorkspaceRecord,
    };
    let first = mint_tab_id();
    let second = mint_tab_id();
    BrowserWorkspaceRecord {
        session_id: session_id.into(),
        scope,
        layout_mode: BrowserLayoutMode::Split,
        split_width_px: 560,
        active_tab_id: Some(second.clone()),
        tabs: vec![
            BrowserTabRecord {
                id: first,
                position: 0,
                current_history_index: Some(1),
                manual_reopen_required: false,
                restoration_status: BrowserRestorationStatus::Restorable,
                history: vec![
                    BrowserHistoryRecord {
                        position: 0,
                        url: "https://example.com/one".into(),
                        recorded_at_ms: 10,
                    },
                    BrowserHistoryRecord {
                        position: 1,
                        url: "https://example.com/two?q=safe".into(),
                        recorded_at_ms: 20,
                    },
                ],
            },
            BrowserTabRecord {
                id: second,
                position: 1,
                current_history_index: Some(0),
                manual_reopen_required: false,
                restoration_status: BrowserRestorationStatus::Restorable,
                history: vec![BrowserHistoryRecord {
                    position: 0,
                    url: "http://localhost:3000/".into(),
                    recorded_at_ms: 30,
                }],
            },
        ],
        recovery: None,
    }
}

#[test]
fn workspace_replace_load_and_relaunch_round_trip_in_position_order() {
    use browser_workspace::{
        load_browser_workspace, replace_browser_workspace, BrowserWorkspaceLoad,
        BrowserWorkspaceScope,
    };
    let td = TempDir::new("browser-round-trip");
    let dir = td.path().join("sessions");
    let session = create(&dir, Some("Browser owner")).unwrap();
    assert_eq!(
        load_browser_workspace(&dir, &session.id, BrowserWorkspaceScope::Local).unwrap(),
        BrowserWorkspaceLoad::Missing
    );

    let expected = workspace_for(&session.id, BrowserWorkspaceScope::Local);
    let saved =
        replace_browser_workspace(&dir, &session.id, BrowserWorkspaceScope::Local, &expected)
            .unwrap();
    assert_eq!(saved, expected);
    drop(raw_conn(&dir)); // A later load opens a fresh connection: relaunch semantics.
    assert_eq!(
        load_browser_workspace(&dir, &session.id, BrowserWorkspaceScope::Local).unwrap(),
        BrowserWorkspaceLoad::Ready(expected)
    );
}

#[test]
fn wrong_database_is_not_found_and_failed_replace_is_atomic() {
    use browser_workspace::{
        load_browser_workspace, replace_browser_workspace, BrowserWorkspaceScope,
    };
    let td = TempDir::new("browser-scope-atomic");
    let local = td.path().join("local");
    let project = td.path().join("project");
    let session = create(&local, None).unwrap();
    create(&project, None).unwrap();
    let original = workspace_for(&session.id, BrowserWorkspaceScope::Local);
    replace_browser_workspace(&local, &session.id, BrowserWorkspaceScope::Local, &original)
        .unwrap();

    assert!(matches!(
        load_browser_workspace(&project, &session.id, BrowserWorkspaceScope::Project),
        Err(SessionStoreError::NotFound(_))
    ));
    let mut invalid = original.clone();
    invalid.tabs.push(invalid.tabs[0].clone());
    invalid.tabs.push(invalid.tabs[0].clone());
    invalid.tabs.push(invalid.tabs[0].clone());
    invalid.tabs.push(invalid.tabs[0].clone());
    assert!(matches!(
        replace_browser_workspace(&local, &session.id, BrowserWorkspaceScope::Local, &invalid),
        Err(SessionStoreError::Limit(_))
    ));
    assert_eq!(
        load_browser_workspace(&local, &session.id, BrowserWorkspaceScope::Local).unwrap(),
        browser_workspace::BrowserWorkspaceLoad::Ready(original)
    );
}

#[test]
fn overlong_current_history_is_trimmed_to_the_newest_twenty_rows() {
    use browser_workspace::{
        load_browser_workspace, replace_browser_workspace, BrowserHistoryRecord,
        BrowserWorkspaceLoad, BrowserWorkspaceScope, MAX_HISTORY_ROWS,
    };
    let td = TempDir::new("browser-history-cap");
    let dir = td.path().join("sessions");
    let session = create(&dir, None).unwrap();
    let mut workspace = workspace_for(&session.id, BrowserWorkspaceScope::Local);
    workspace.tabs.truncate(1);
    workspace.active_tab_id = Some(workspace.tabs[0].id.clone());
    workspace.tabs[0].history = (0..=MAX_HISTORY_ROWS)
        .map(|position| BrowserHistoryRecord {
            position,
            url: format!("https://example.com/{position}"),
            recorded_at_ms: position as i64,
        })
        .collect();
    workspace.tabs[0].current_history_index = Some(MAX_HISTORY_ROWS);

    let saved =
        replace_browser_workspace(&dir, &session.id, BrowserWorkspaceScope::Local, &workspace)
            .unwrap();
    assert_eq!(saved.tabs[0].history.len(), MAX_HISTORY_ROWS);
    assert_eq!(saved.tabs[0].history[0].url, "https://example.com/1");
    assert_eq!(saved.tabs[0].history[0].position, 0);
    assert_eq!(
        saved.tabs[0].current_history_index,
        Some(MAX_HISTORY_ROWS - 1)
    );
    assert_eq!(
        load_browser_workspace(&dir, &session.id, BrowserWorkspaceScope::Local).unwrap(),
        BrowserWorkspaceLoad::Ready(saved)
    );
}

#[test]
fn corrupt_browser_rows_reset_without_losing_the_transcript() {
    use browser_workspace::{
        load_browser_workspace, replace_browser_workspace, BrowserWorkspaceLoad,
        BrowserWorkspaceScope,
    };
    let td = TempDir::new("browser-corrupt-reset");
    let dir = td.path().join("sessions");
    let session = create(&dir, None).unwrap();
    save_transcript(
        &dir,
        &session.id,
        &[super::tests::user_entry("keep chat")],
        false,
    )
    .unwrap();
    replace_browser_workspace(
        &dir,
        &session.id,
        BrowserWorkspaceScope::Local,
        &workspace_for(&session.id, BrowserWorkspaceScope::Local),
    )
    .unwrap();
    raw_conn(&dir)
        .execute(
            "UPDATE browser_workspaces SET layout_mode='sideways' WHERE session_id=?1",
            params![session.id],
        )
        .unwrap();

    assert!(matches!(
        load_browser_workspace(&dir, &session.id, BrowserWorkspaceScope::Local).unwrap(),
        BrowserWorkspaceLoad::ResetCorrupt { .. }
    ));
    assert_eq!(
        load_browser_workspace(&dir, &session.id, BrowserWorkspaceScope::Local).unwrap(),
        BrowserWorkspaceLoad::Missing
    );
    assert_eq!(load(&dir, &session.id).unwrap().entries.len(), 1);
}

#[test]
fn persisted_history_over_the_cap_is_reset_instead_of_silently_coerced() {
    use browser_workspace::{
        load_browser_workspace, replace_browser_workspace, BrowserWorkspaceLoad,
        BrowserWorkspaceScope, MAX_HISTORY_ROWS,
    };
    let td = TempDir::new("browser-persisted-history-cap");
    let dir = td.path().join("sessions");
    let session = create(&dir, None).unwrap();
    let mut workspace = workspace_for(&session.id, BrowserWorkspaceScope::Local);
    workspace.tabs.truncate(1);
    workspace.active_tab_id = Some(workspace.tabs[0].id.clone());
    let tab_id = workspace.tabs[0].id.clone();
    replace_browser_workspace(&dir, &session.id, BrowserWorkspaceScope::Local, &workspace).unwrap();
    let conn = raw_conn(&dir);
    for position in 2..=MAX_HISTORY_ROWS {
        conn.execute(
            "INSERT INTO browser_history (tab_id,position,url,recorded_at_ms)
             VALUES (?1,?2,?3,?2)",
            params![
                tab_id,
                position as i64,
                format!("https://example.com/{position}")
            ],
        )
        .unwrap();
    }
    conn.execute(
        "UPDATE browser_tabs SET current_history_index=?2 WHERE id=?1",
        params![tab_id, MAX_HISTORY_ROWS as i64],
    )
    .unwrap();
    drop(conn);

    assert!(matches!(
        load_browser_workspace(&dir, &session.id, BrowserWorkspaceScope::Local).unwrap(),
        BrowserWorkspaceLoad::ResetCorrupt { .. }
    ));
}

#[test]
fn blank_tab_round_trips_without_inventing_a_history_index_or_url() {
    use browser_workspace::{
        load_browser_workspace, mint_tab_id, replace_browser_workspace, BrowserLayoutMode,
        BrowserRestorationStatus, BrowserTabRecord, BrowserWorkspaceLoad, BrowserWorkspaceRecord,
        BrowserWorkspaceScope,
    };
    let td = TempDir::new("browser-blank-tab");
    let dir = td.path().join("sessions");
    let session = create(&dir, None).unwrap();
    let tab_id = mint_tab_id();
    let workspace = BrowserWorkspaceRecord {
        session_id: session.id.clone(),
        scope: BrowserWorkspaceScope::Local,
        layout_mode: BrowserLayoutMode::Split,
        split_width_px: 560,
        active_tab_id: Some(tab_id.clone()),
        tabs: vec![BrowserTabRecord {
            id: tab_id,
            position: 0,
            current_history_index: None,
            manual_reopen_required: false,
            restoration_status: BrowserRestorationStatus::Blank,
            history: vec![],
        }],
        recovery: None,
    };
    replace_browser_workspace(&dir, &session.id, BrowserWorkspaceScope::Local, &workspace).unwrap();
    assert_eq!(
        load_browser_workspace(&dir, &session.id, BrowserWorkspaceScope::Local).unwrap(),
        BrowserWorkspaceLoad::Ready(workspace)
    );
}

#[test]
fn explicit_reset_replaces_state_with_one_backend_minted_blank_tab() {
    use browser_workspace::{
        load_browser_workspace, replace_browser_workspace, reset_browser_workspace,
        BrowserRestorationStatus, BrowserWorkspaceLoad, BrowserWorkspaceScope,
    };
    let td = TempDir::new("browser-explicit-reset");
    let dir = td.path().join("sessions");
    let session = create(&dir, None).unwrap();
    let original = workspace_for(&session.id, BrowserWorkspaceScope::Local);
    let previous_ids: Vec<_> = original.tabs.iter().map(|tab| tab.id.clone()).collect();
    replace_browser_workspace(&dir, &session.id, BrowserWorkspaceScope::Local, &original).unwrap();

    let reset = reset_browser_workspace(&dir, &session.id, BrowserWorkspaceScope::Local).unwrap();
    assert_eq!(reset.tabs.len(), 1);
    assert!(!previous_ids.contains(&reset.tabs[0].id));
    assert!(reset.tabs[0].id.starts_with("bt_"));
    assert!(reset.tabs[0].history.is_empty());
    assert_eq!(reset.tabs[0].current_history_index, None);
    assert_eq!(
        reset.tabs[0].restoration_status,
        BrowserRestorationStatus::Blank
    );
    assert_eq!(reset.active_tab_id.as_ref(), Some(&reset.tabs[0].id));
    assert_eq!(
        load_browser_workspace(&dir, &session.id, BrowserWorkspaceScope::Local).unwrap(),
        BrowserWorkspaceLoad::Ready(reset)
    );
}

#[test]
fn delete_cascades_but_fork_and_rewind_start_without_browser_state() {
    use browser_workspace::{
        load_browser_workspace, replace_browser_workspace, BrowserWorkspaceLoad,
        BrowserWorkspaceScope,
    };
    let td = TempDir::new("browser-branch-delete");
    let dir = td.path().join("sessions");
    let source = create(&dir, Some("source")).unwrap();
    save_transcript(&dir, &source.id, &[super::tests::user_entry("turn")], false).unwrap();
    let workspace = workspace_for(&source.id, BrowserWorkspaceScope::Local);
    replace_browser_workspace(&dir, &source.id, BrowserWorkspaceScope::Local, &workspace).unwrap();

    let continued = fork(&dir, &source.id, false).unwrap();
    let rewound = rollback(&dir, &source.id, 1, false).unwrap();
    for child in [&continued, &rewound] {
        assert_eq!(
            load_browser_workspace(&dir, &child.id, BrowserWorkspaceScope::Local).unwrap(),
            BrowserWorkspaceLoad::Missing
        );
    }
    assert_eq!(
        load_browser_workspace(&dir, &source.id, BrowserWorkspaceScope::Local).unwrap(),
        BrowserWorkspaceLoad::Ready(workspace)
    );
    delete(&dir, &source.id).unwrap();
    let conn = raw_conn(&dir);
    let rows: i64 = conn
        .query_row("SELECT COUNT(*) FROM browser_workspaces", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(rows, 0);
}

#[test]
fn native_navigation_commits_move_or_append_history_atomically_and_safely() {
    use browser_workspace::{
        commit_browser_navigation, load_browser_workspace, replace_browser_workspace,
        BrowserHistoryNavigation, BrowserWorkspaceLoad, BrowserWorkspaceScope,
    };
    let td = TempDir::new("browser-native-navigation");
    let dir = td.path().join("sessions");
    let session = create(&dir, None).unwrap();
    let mut workspace = workspace_for(&session.id, BrowserWorkspaceScope::Local);
    workspace.tabs.truncate(1);
    workspace.tabs[0].position = 0;
    workspace.active_tab_id = Some(workspace.tabs[0].id.clone());
    let tab_id = workspace.tabs[0].id.clone();
    replace_browser_workspace(&dir, &session.id, BrowserWorkspaceScope::Local, &workspace).unwrap();

    let back = workspace.tabs[0].history[0].url.clone();
    commit_browser_navigation(
        &dir,
        &session.id,
        BrowserWorkspaceScope::Local,
        &tab_id,
        &back,
        BrowserHistoryNavigation::Back,
    )
    .unwrap();
    commit_browser_navigation(
        &dir,
        &session.id,
        BrowserWorkspaceScope::Local,
        &tab_id,
        &workspace.tabs[0].history[1].url,
        BrowserHistoryNavigation::Forward,
    )
    .unwrap();

    let secret = format!("sk-{}", "x".repeat(24));
    commit_browser_navigation(
        &dir,
        &session.id,
        BrowserWorkspaceScope::Local,
        &tab_id,
        &format!("https://example.com/new?token={secret}"),
        BrowserHistoryNavigation::New,
    )
    .unwrap();
    commit_browser_navigation(
        &dir,
        &session.id,
        BrowserWorkspaceScope::Local,
        &tab_id,
        "https://example.com/new",
        BrowserHistoryNavigation::Reload,
    )
    .unwrap();

    let BrowserWorkspaceLoad::Ready(saved) =
        load_browser_workspace(&dir, &session.id, BrowserWorkspaceScope::Local).unwrap()
    else {
        panic!("workspace should remain readable");
    };
    let tab = &saved.tabs[0];
    assert_eq!(tab.history.len(), 3);
    assert_eq!(tab.current_history_index, Some(2));
    assert_eq!(tab.history[2].url, "https://example.com/new");
    assert!(tab.manual_reopen_required);

    commit_browser_navigation(
        &dir,
        &session.id,
        BrowserWorkspaceScope::Local,
        &tab_id,
        "https://example.com/new",
        BrowserHistoryNavigation::Reopen,
    )
    .unwrap();
    let BrowserWorkspaceLoad::Ready(reopened) =
        load_browser_workspace(&dir, &session.id, BrowserWorkspaceScope::Local).unwrap()
    else {
        panic!("reopened workspace should remain readable");
    };
    assert!(!reopened.tabs[0].manual_reopen_required);
    assert_eq!(
        reopened.tabs[0].restoration_status,
        browser_workspace::BrowserRestorationStatus::Restorable,
    );
    assert_eq!(reopened.tabs[0].history, tab.history);

    let before = reopened;
    assert!(commit_browser_navigation(
        &dir,
        &session.id,
        BrowserWorkspaceScope::Local,
        &tab_id,
        "https://wrong.example/",
        BrowserHistoryNavigation::Back,
    )
    .is_err());
    assert_eq!(
        load_browser_workspace(&dir, &session.id, BrowserWorkspaceScope::Local).unwrap(),
        BrowserWorkspaceLoad::Ready(before)
    );
}
