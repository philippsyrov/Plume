//! The app-private Home conversation.
//!
//! Home is an ordinary `chat_sessions` row carrying a backend-owned marker, so
//! every other path in this module — load, list, fork, rollback, transcript
//! save, delete — works on it unchanged. Only its identity is special, and the
//! partial unique index in `schema.rs` is what keeps "exactly one" true.

use std::path::Path;

use rusqlite::{params, OptionalExtension};

use super::{
    now_ms, schema, store_lock, summary_from_row, validation, SessionStoreError, SessionSummary,
};

/// The one app-private Home conversation, created on first call.
///
/// Home is an ordinary `chat_sessions` row carrying a backend-owned marker, so
/// load, list, fork, rollback, transcript save, and delete all keep working on
/// it with no special-casing. Only its identity is special.
///
/// Idempotent under concurrency: the read-or-create runs inside the store lock
/// and one transaction, so two simultaneous callers cannot produce two Homes —
/// one inserts and the other reads what it inserted. The partial unique index
/// backs that up at the database level.
///
/// Deliberately exempt from `MAX_SESSIONS`. That cap stops unbounded session
/// growth, but refusing to create the one conversation the app opens into would
/// make a full store unusable rather than merely full.
pub fn home(sessions_dir: &Path) -> Result<SessionSummary, SessionStoreError> {
    let lock = store_lock(sessions_dir);
    let _guard = lock.lock().expect("session store mutex poisoned");
    let mut conn = schema::open_connection(sessions_dir)?;
    let tx = conn
        .transaction()
        .map_err(schema::storage("begin home transaction"))?;

    let existing: Option<String> = tx
        .query_row(
            "SELECT id FROM chat_sessions WHERE is_home = 1",
            [],
            |row| row.get(0),
        )
        .optional()
        .map_err(schema::storage("read home session"))?;

    let id = match existing {
        Some(id) => id,
        None => {
            let id = validation::mint_session_id();
            let now = now_ms();
            tx.execute(
                "INSERT INTO chat_sessions
                   (id, title, created_at_ms, updated_at_ms, archived_at_ms, is_home)
                 VALUES (?1, ?2, ?3, ?3, NULL, 1)",
                params![id, validation::HOME_TITLE, now],
            )
            .map_err(schema::storage("create home session"))?;
            id
        }
    };

    let summary = tx
        .query_row(
            "SELECT id, title, created_at_ms, updated_at_ms, archived_at_ms,
                    forked_from_session_id, forked_through_entry_id, is_home
             FROM chat_sessions WHERE id = ?1",
            params![id],
            summary_from_row,
        )
        .map_err(schema::storage("read home summary"))?;
    tx.commit().map_err(schema::storage("commit home"))?;
    Ok(summary)
}
