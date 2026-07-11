//! D66 store-level tests: full-text session search, the v2 schema
//! migration, and the FTS index-sync guarantees. Shares the tempdir
//! and entry helpers with `tests.rs`.

use std::fs;
use std::path::Path;

use super::tests::{raw_conn, user_entry, TempDir};
use super::*;

use super::search::{SearchMatchKind, SNIPPET_END, SNIPPET_START};

fn seed_session(dir: &Path, title: &str, contents: &[&str]) -> SessionSummary {
    let session = create(dir, Some(title)).expect("create");
    if !contents.is_empty() {
        let entries: Vec<TranscriptEntry> = contents.iter().map(|c| user_entry(c)).collect();
        save_transcript(dir, &session.id, &entries, false).expect("save");
    }
    session
}

#[test]
fn fresh_database_is_schema_v2_with_fts_objects() {
    let dir = TempDir::new("fts-fresh");
    seed_session(dir.path(), "hello", &[]);
    let conn = raw_conn(dir.path());
    let version: i64 = conn
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap();
    assert_eq!(version, 2);
    let fts_tables: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master
             WHERE type = 'table' AND name IN ('titles_fts', 'messages_fts')",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(fts_tables, 2);
}

#[test]
fn v1_database_migrates_to_v2_and_backfills_existing_rows() {
    let dir = TempDir::new("fts-migrate");
    // Build a genuine v1 database by hand — the exact schema D63A shipped.
    fs::create_dir_all(dir.path()).unwrap();
    let conn = raw_conn(dir.path());
    conn.execute_batch(
        "BEGIN;
         CREATE TABLE chat_sessions (
           id TEXT PRIMARY KEY NOT NULL,
           title TEXT NOT NULL,
           created_at_ms INTEGER NOT NULL,
           updated_at_ms INTEGER NOT NULL,
           archived_at_ms INTEGER
         );
         CREATE TABLE chat_messages (
           id TEXT PRIMARY KEY NOT NULL,
           session_id TEXT NOT NULL REFERENCES chat_sessions(id) ON DELETE CASCADE,
           ordinal INTEGER NOT NULL,
           kind TEXT NOT NULL,
           role TEXT,
           content TEXT NOT NULL,
           model_used TEXT,
           duration_ms INTEGER,
           attachment_rel_path TEXT,
           attachment_start_line INTEGER,
           attachment_end_line INTEGER,
           stats_json TEXT,
           sent_in_mode TEXT,
           created_at_ms INTEGER NOT NULL,
           UNIQUE(session_id, ordinal)
         );
         CREATE INDEX chat_sessions_updated_idx
           ON chat_sessions(archived_at_ms, updated_at_ms DESC);
         INSERT INTO chat_sessions VALUES ('sold1', 'legacy borrow checker chat', 1, 2, NULL);
         INSERT INTO chat_messages VALUES
           ('mold1', 'sold1', 0, 'message', 'user',
            'why does the borrow checker reject this lifetime', NULL, NULL,
            NULL, NULL, NULL, NULL, 'chat', 1);
         PRAGMA user_version = 1;
         COMMIT;",
    )
    .unwrap();
    drop(conn);

    // Any store operation migrates on open; search then finds the
    // backfilled rows by title AND by content.
    let by_title = search(dir.path(), "legacy", None).expect("title search");
    assert_eq!(by_title.len(), 1);
    assert_eq!(by_title[0].id, "sold1");
    assert_eq!(by_title[0].match_kind, SearchMatchKind::Title);

    let by_content = search(dir.path(), "lifetime", None).expect("content search");
    assert_eq!(by_content.len(), 1);
    assert_eq!(by_content[0].match_kind, SearchMatchKind::Content);
    let snippet = by_content[0].snippet.as_deref().expect("snippet");
    assert!(snippet.contains(&format!("{SNIPPET_START}lifetime{SNIPPET_END}")));

    let conn = raw_conn(dir.path());
    let version: i64 = conn
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap();
    assert_eq!(version, 2);
}

#[test]
fn unknown_future_schema_version_is_still_refused() {
    let dir = TempDir::new("fts-future");
    seed_session(dir.path(), "hello", &[]);
    let conn = raw_conn(dir.path());
    conn.execute_batch("PRAGMA user_version = 3").unwrap();
    drop(conn);
    let err = search(dir.path(), "hello", None).unwrap_err();
    assert!(matches!(err, SessionStoreError::Corrupt(_)));
}

#[test]
fn search_matches_titles_and_content_with_title_hits_first() {
    let dir = TempDir::new("fts-basic");
    let titled = seed_session(dir.path(), "gradient descent notes", &[]);
    let by_content = seed_session(
        dir.path(),
        "untitled thoughts",
        &["today I tuned gradient clipping for the run"],
    );
    seed_session(dir.path(), "unrelated", &["nothing to see"]);

    let hits = search(dir.path(), "gradient", None).expect("search");
    assert_eq!(hits.len(), 2);
    assert_eq!(hits[0].id, titled.id);
    assert_eq!(hits[0].match_kind, SearchMatchKind::Title);
    assert!(hits[0].snippet.is_none());
    assert_eq!(hits[1].id, by_content.id);
    assert_eq!(hits[1].match_kind, SearchMatchKind::Content);
    assert!(hits[1]
        .snippet
        .as_deref()
        .unwrap()
        .contains(&format!("{SNIPPET_START}gradient{SNIPPET_END}")));
}

#[test]
fn a_session_matching_both_reports_title_kind_and_keeps_the_snippet() {
    let dir = TempDir::new("fts-both");
    let s = seed_session(
        dir.path(),
        "tokenizer bug",
        &["the tokenizer split the emoji wrong"],
    );
    let hits = search(dir.path(), "tokenizer", None).expect("search");
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].id, s.id);
    assert_eq!(hits[0].match_kind, SearchMatchKind::Title);
    assert!(hits[0].snippet.is_some());
}

#[test]
fn prefix_matching_supports_incremental_typing() {
    let dir = TempDir::new("fts-prefix");
    seed_session(dir.path(), "persistence spine design", &[]);
    let hits = search(dir.path(), "persis", None).expect("search");
    assert_eq!(hits.len(), 1);
}

#[test]
fn search_never_crosses_databases() {
    let dir_a = TempDir::new("fts-scope-a");
    let dir_b = TempDir::new("fts-scope-b");
    seed_session(dir_a.path(), "alpha secret", &["alpha transcript text"]);
    seed_session(dir_b.path(), "beta things", &["beta transcript text"]);

    let hits_a = search(dir_a.path(), "alpha", None).expect("search a");
    assert_eq!(hits_a.len(), 1);
    assert!(search(dir_a.path(), "beta", None)
        .expect("cross")
        .is_empty());
    assert!(search(dir_b.path(), "alpha", None)
        .expect("cross")
        .is_empty());
}

#[test]
fn results_are_bounded_and_limit_is_validated() {
    let dir = TempDir::new("fts-bound");
    for i in 0..25 {
        seed_session(dir.path(), &format!("common topic {i}"), &[]);
    }
    let hits = search(dir.path(), "common", None).expect("search");
    assert_eq!(hits.len(), 20);
    let three = search(dir.path(), "common", Some(3)).expect("limited");
    assert_eq!(three.len(), 3);
    assert!(matches!(
        search(dir.path(), "common", Some(0)),
        Err(SessionStoreError::Invalid(_))
    ));
    assert!(matches!(
        search(dir.path(), "common", Some(21)),
        Err(SessionStoreError::Invalid(_))
    ));
}

#[test]
fn archived_sessions_are_searchable_and_flagged() {
    let dir = TempDir::new("fts-archived");
    let s = seed_session(dir.path(), "archived treasure", &[]);
    set_archived(dir.path(), &s.id, true).expect("archive");
    let hits = search(dir.path(), "treasure", None).expect("search");
    assert_eq!(hits.len(), 1);
    assert!(hits[0].archived_at_ms.is_some());
}

#[test]
fn rename_updates_the_title_index() {
    let dir = TempDir::new("fts-rename");
    let s = seed_session(dir.path(), "old moniker", &[]);
    rename(dir.path(), &s.id, "fresh handle").expect("rename");
    assert!(search(dir.path(), "moniker", None).expect("old").is_empty());
    let hits = search(dir.path(), "handle", None).expect("new");
    assert_eq!(hits.len(), 1);
}

#[test]
fn transcript_replacement_updates_the_content_index() {
    let dir = TempDir::new("fts-resave");
    let s = seed_session(dir.path(), "resave", &["original draughtsman text"]);
    save_transcript(
        dir.path(),
        &s.id,
        &[user_entry("replacement wording entirely")],
        false,
    )
    .expect("resave");
    assert!(search(dir.path(), "draughtsman", None)
        .expect("old")
        .is_empty());
    assert_eq!(
        search(dir.path(), "replacement", None).expect("new").len(),
        1
    );
}

#[test]
fn delete_purges_both_indexes_completely() {
    let dir = TempDir::new("fts-delete");
    let s = seed_session(dir.path(), "doomed chronicle", &["doomed transcript body"]);
    delete(dir.path(), &s.id).expect("delete");
    assert!(search(dir.path(), "doomed", None).expect("t").is_empty());
    assert!(search(dir.path(), "chronicle", None).expect("c").is_empty());
    // The index itself holds zero rows — stale entries would be a
    // rowid-reuse hazard, not just bloat.
    let conn = raw_conn(dir.path());
    let title_rows: i64 = conn
        .query_row("SELECT COUNT(*) FROM titles_fts", [], |r| r.get(0))
        .unwrap();
    let content_rows: i64 = conn
        .query_row("SELECT COUNT(*) FROM messages_fts", [], |r| r.get(0))
        .unwrap();
    assert_eq!(title_rows, 0);
    assert_eq!(content_rows, 0);
}

#[test]
fn fts_operators_in_queries_are_treated_as_literal_text() {
    let dir = TempDir::new("fts-inject");
    seed_session(dir.path(), "operator soup", &["mentions NEAR the docks"]);
    // None of these may error or change semantics — they are text.
    for query in [
        "NEAR(docks",
        "docks OR treasure",
        "\"unbalanced",
        "docks*",
        "-docks",
        "docks AND NOT soup",
        "col:docks",
    ] {
        let result = search(dir.path(), query, None);
        assert!(result.is_ok(), "query {query:?} errored: {result:?}");
    }
    // A quoted operator word still finds the literal text.
    let hits = search(dir.path(), "NEAR", None).expect("near");
    assert_eq!(hits.len(), 1);
}

#[test]
fn empty_and_unsearchable_queries_are_rejected_typed() {
    let dir = TempDir::new("fts-reject");
    seed_session(dir.path(), "anything", &[]);
    for query in ["", "   ", "\n\t", "*** ---", "\"\""] {
        assert!(
            matches!(
                search(dir.path(), query, None),
                Err(SessionStoreError::Invalid(_))
            ),
            "query {query:?} was not rejected"
        );
    }
    let long = "x".repeat(201);
    assert!(matches!(
        search(dir.path(), &long, None),
        Err(SessionStoreError::Invalid(_))
    ));
}

#[test]
fn a_noisy_session_cannot_evict_other_matching_sessions(// Codex P2 on #111: the pre-fix 200-row scan cap applied BEFORE
    // message hits were folded into sessions, so one chat with 201
    // matching messages filled the window and silently hid every
    // other matching chat.
) {
    let dir = TempDir::new("fts-noisy");
    // The quiet chat is saved FIRST: with near-identical bm25 scores
    // the pre-fix tiebreak (`updated_at_ms DESC`) put the noisy
    // chat's 201 rows ahead of it, filling the old 200-row window.
    let quiet = create(dir.path(), Some("quiet chat")).expect("create quiet");
    save_transcript(
        dir.path(),
        &quiet.id,
        &[user_entry("a single lighthouse mention")],
        false,
    )
    .expect("save quiet");

    let noisy = create(dir.path(), Some("noisy chat")).expect("create noisy");
    let entries: Vec<TranscriptEntry> = (0..201)
        .map(|i| user_entry(&format!("lighthouse sighting number {i}")))
        .collect();
    save_transcript(dir.path(), &noisy.id, &entries, false).expect("save noisy");

    let hits = search(dir.path(), "lighthouse", None).expect("search");
    let ids: Vec<&str> = hits.iter().map(|h| h.id.as_str()).collect();
    assert!(
        ids.contains(&noisy.id.as_str()),
        "noisy chat missing: {ids:?}"
    );
    assert!(
        ids.contains(&quiet.id.as_str()),
        "quiet chat evicted: {ids:?}"
    );
    assert_eq!(hits.len(), 2);
    // Still one hit per session — the fold, not the scan window, is
    // what keeps a noisy chat from flooding the results.
    for hit in &hits {
        assert!(hit.snippet.is_some());
    }
}
