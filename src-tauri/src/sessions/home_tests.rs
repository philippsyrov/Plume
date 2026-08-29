//! Home-conversation store tests.
//!
//! What these guard is the part of Home that is *not* ordinary: exactly one
//! exists, it is stable across reopens, and it stays creatable when the store
//! is otherwise full. Ordinary behaviour is covered by `tests.rs`.

use super::tests::{raw_conn, user_entry, TempDir};
use super::*;

#[test]
fn home_is_created_once_and_returned_thereafter() {
    let td = TempDir::new("home-idempotent");
    let first = home(td.path()).expect("first home");
    let second = home(td.path()).expect("second home");

    assert_eq!(first.id, second.id, "Home must be one stable conversation");

    let conn = raw_conn(td.path());
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM chat_sessions WHERE is_home = 1",
            [],
            |row| row.get(0),
        )
        .expect("count home rows");
    assert_eq!(count, 1, "exactly one Home row");
}

#[test]
fn home_survives_reopening_the_store() {
    let td = TempDir::new("home-relaunch");
    let before = home(td.path()).expect("home before");
    // A fresh connection is what a relaunch actually does.
    let after = home(td.path()).expect("home after");
    assert_eq!(before.id, after.id);

    let loaded = load(td.path(), &after.id).expect("home loads like any session");
    assert_eq!(loaded.id, before.id);
}

#[test]
fn a_second_home_row_is_refused_by_the_database() {
    let td = TempDir::new("home-unique");
    home(td.path()).expect("home");
    let created = create(td.path(), Some("ordinary")).expect("ordinary session");

    let conn = raw_conn(td.path());
    let forged = conn.execute(
        "UPDATE chat_sessions SET is_home = 1 WHERE id = ?1",
        rusqlite::params![created.id],
    );
    assert!(
        forged.is_err(),
        "the partial unique index must make a second Home impossible, \
         not merely discouraged by call-site discipline",
    );
}

#[test]
fn home_is_creatable_when_the_store_is_at_its_session_cap() {
    let td = TempDir::new("home-at-cap");
    // Fill the store without going through `create`, which is what the cap
    // guards; the point here is the state, not the path that reaches it.
    schema::open_connection(td.path()).expect("init schema");
    let conn = raw_conn(td.path());
    for index in 0..crate::sessions::validation::MAX_SESSIONS {
        conn.execute(
            "INSERT INTO chat_sessions (id, title, created_at_ms, updated_at_ms, archived_at_ms)
             VALUES (?1, ?2, 1, 1, NULL)",
            rusqlite::params![format!("filler-{index}"), format!("filler {index}")],
        )
        .expect("insert filler session");
    }
    drop(conn);

    assert!(
        create(td.path(), Some("one more")).is_err(),
        "the ordinary cap still applies to ordinary sessions",
    );
    // Refusing the one conversation the app opens into would make a full
    // store unusable rather than merely full.
    let home = home(td.path()).expect("Home is exempt from the session cap");
    assert!(!home.id.is_empty());
}

#[test]
fn home_behaves_like_any_other_session_for_transcripts() {
    let td = TempDir::new("home-transcript");
    let created = home(td.path()).expect("home");
    save_transcript(td.path(), &created.id, &[user_entry("hello")], false)
        .expect("save transcript into Home");

    let loaded = load(td.path(), &created.id).expect("load Home");
    assert_eq!(loaded.entries.len(), 1);
}
