//! D63A: connection initialization and the v1 schema for a chat-session
//! database.
//!
//! Every store operation opens a fresh connection through
//! [`open_connection`], so the guarantees here — symlink refusal,
//! `foreign_keys` ON, schema at version 1 — hold for *every* connection,
//! including ones against a database created by an earlier launch.
//! Connections are short-lived (open → operate → drop) and serialized by
//! the per-path mutex in `mod.rs`, so there is no pooled state to drift.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::Connection;

use super::SessionStoreError;

/// Schema version stamped in `PRAGMA user_version`. Bump only with a
/// migration path; an unknown version is refused, never migrated
/// implicitly.
pub(super) const SCHEMA_VERSION: i64 = 1;

/// Database file name inside a sessions directory. The same file name
/// is used for both scopes; separation comes from the directory
/// (`<app-data>/sessions` vs `<project>/.plume/sessions`), never from
/// the file name.
pub(super) const DB_FILE_NAME: &str = "state.sqlite";

pub(super) fn db_path(sessions_dir: &Path) -> PathBuf {
    sessions_dir.join(DB_FILE_NAME)
}

/// Open (creating directory, file, and schema as needed) the session
/// database under `sessions_dir`.
///
/// Defensive posture matches the memory store and patch checkpoints:
/// a pre-planted symlink at the sessions directory or the database
/// file — or a database file with multiple hardlink aliases — is
/// refused before any filesystem write, so a hostile project cannot
/// redirect session writes outside its own `.plume/` through either
/// alias mechanism.
pub(super) fn open_connection(sessions_dir: &Path) -> Result<Connection, SessionStoreError> {
    refuse_symlink(sessions_dir, "sessions directory")?;
    fs::create_dir_all(sessions_dir).map_err(|e| {
        SessionStoreError::Storage(format!(
            "create sessions directory {}: {e}",
            sessions_dir.display()
        ))
    })?;
    let db = db_path(sessions_dir);
    refuse_symlink(&db, "session database file")?;
    refuse_hardlink_alias(&db, "session database file")?;

    let conn = Connection::open(&db).map_err(storage("open session database"))?;
    // Cross-process insurance only: within one Plume process the
    // per-path mutex in `mod.rs` already serializes access.
    conn.busy_timeout(Duration::from_secs(5))
        .map_err(storage("set busy timeout"))?;
    // Per-connection pragma — SQLite does not persist it, so it must
    // run on every open for ON DELETE CASCADE to hold.
    conn.pragma_update(None, "foreign_keys", true)
        .map_err(storage("enable foreign keys"))?;

    let version: i64 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .map_err(storage("read schema version"))?;
    match version {
        0 => init_schema(&conn)?,
        SCHEMA_VERSION => {}
        other => {
            return Err(SessionStoreError::Corrupt(format!(
                "unsupported session schema version {other} (this build speaks {SCHEMA_VERSION})"
            )));
        }
    }
    Ok(conn)
}

/// Create the v1 tables, index, and version stamp in one transaction,
/// so a crash mid-initialization leaves `user_version = 0` and the
/// next open retries from scratch instead of seeing half a schema.
fn init_schema(conn: &Connection) -> Result<(), SessionStoreError> {
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
         PRAGMA user_version = 1;
         COMMIT;",
    )
    .map_err(storage("initialize session schema v1"))
}

/// Reject any pre-existing path that is a symlink. Same guard as
/// `memory::store::refuse_symlink` and the patch checkpoint's
/// `ensure_not_symlink`; kept local so the sessions module stays
/// independent of both.
pub(super) fn refuse_symlink(path: &Path, label: &str) -> Result<(), SessionStoreError> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => Err(SessionStoreError::Refused(format!(
            "{label} at {} is a symlink; refusing to touch it",
            path.display()
        ))),
        Ok(_) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(SessionStoreError::Storage(format!(
            "inspect {label} {}: {e}",
            path.display()
        ))),
    }
}

/// Reject an existing database file with more than one hard link.
/// `refuse_symlink` blocks the symlink alias; this blocks the hardlink
/// one — a hostile project could `ln` a pre-planted `state.sqlite` to
/// another SQLite file on the same filesystem and turn every session
/// write into a write on that outside file. Delegates to the central
/// `safety::path::ensure_no_hardlink_alias` posture (Unix `nlink > 1`
/// on regular files; no-op on non-Unix, where that helper reserves the
/// platform-specific implementation). A missing file is fine — it will
/// be created fresh by `Connection::open`.
fn refuse_hardlink_alias(path: &Path, label: &str) -> Result<(), SessionStoreError> {
    use crate::safety::path::{ensure_no_hardlink_alias, PathError};
    match ensure_no_hardlink_alias(path) {
        Ok(()) => Ok(()),
        Err(PathError::Hardlink(p)) => Err(SessionStoreError::Refused(format!(
            "{label} at {} has multiple hardlink aliases; refusing to touch an aliased database",
            p.display()
        ))),
        Err(PathError::Io { source, .. }) if source.kind() == std::io::ErrorKind::NotFound => {
            Ok(())
        }
        Err(other) => Err(SessionStoreError::Storage(format!(
            "inspect {label} {}: {other}",
            path.display()
        ))),
    }
}

/// Shared `rusqlite::Error → SessionStoreError::Storage` adapter with a
/// stable context prefix. A storage failure surfaces as a typed error;
/// it must never panic the Tauri process.
pub(super) fn storage(context: &'static str) -> impl Fn(rusqlite::Error) -> SessionStoreError {
    move |e| SessionStoreError::Storage(format!("{context}: {e}"))
}
