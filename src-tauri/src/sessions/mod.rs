//! D63A: durable chat-session persistence — the session spine.
//!
//! One SQLite schema, two physically separate databases:
//!
//! * **Local chats** — `<tauri app data>/sessions/state.sqlite`
//! * **Project chats** — `<trusted project>/.plume/sessions/state.sqlite`
//!
//! The split is load-bearing. This module never chooses between the two
//! roots: every operation takes the sessions *directory* it should act
//! on, and the command layer (`commands::sessions`) is the only place
//! that maps `scope: 'local' | 'project'` onto a directory — local from
//! app data resolved once at startup, project only through the
//! currently open **trusted** project. A mismatched session id against
//! the wrong database is a plain `NotFound`, because the id simply is
//! not in that file.
//!
//! Concurrency: a process-wide mutex per sessions directory serializes
//! open + operate, so two commands cannot interleave initialization or
//! replacement writes. Connections are short-lived; `schema.rs` re-runs
//! the per-connection guarantees (foreign keys, version check) on every
//! open. All SQL values are bound parameters.
//!
//! Sessions belong to the Plume application runtime — they are not
//! routed through the model-provider trait and never reach the
//! frontend as a filesystem path.

mod branch;
pub(crate) mod browser_workspace;
mod schema;
// `pub(crate)` so tests (here and in the command layer) can reach the
// snippet-marker constants and `SearchMatchKind` without a bin-unused
// re-export; non-test code uses the two re-exports below.
pub(crate) mod search;
mod validation;

#[cfg(test)]
#[path = "tests.rs"]
mod tests;

#[cfg(test)]
#[path = "context_tests.rs"]
mod context_tests;

#[cfg(test)]
#[path = "fork_tests.rs"]
mod fork_tests;

#[cfg(test)]
#[path = "rollback_tests.rs"]
mod rollback_tests;

#[cfg(test)]
#[path = "search_tests.rs"]
mod search_tests;

#[cfg(test)]
#[path = "browser_workspace_tests.rs"]
mod browser_workspace_tests;

pub use search::{search, SearchHit};

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::prompts::{ContextSourceManifestItem, ContextSourceRef};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

pub use validation::parse_entries;

/// Store-level error. The command layer maps these onto the IPC error
/// model (`NotFound` → `NotFound`, `Invalid` → `BadArgument`,
/// `Limit`/`Refused` → `Blocked`, `Corrupt`/`Storage` → `Internal`).
#[derive(Debug, thiserror::Error)]
pub enum SessionStoreError {
    #[error("session not found: {0}")]
    NotFound(String),
    #[error("{0}")]
    Invalid(String),
    #[error("{0}")]
    Limit(String),
    #[error("{0}")]
    Refused(String),
    #[error("corrupt session store: {0}")]
    Corrupt(String),
    #[error("session storage failure: {0}")]
    Storage(String),
}

/// List-row shape shared by every verb that returns a session without
/// its transcript. `archivedAtMs` is `null` for a live session.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SessionSummary {
    pub id: String,
    pub title: String,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    pub archived_at_ms: Option<i64>,
    pub forked_from_session_id: Option<String>,
    pub forked_through_entry_id: Option<String>,
}

/// `sessions.load` shape: the summary fields plus the persisted
/// transcript in visible-chat form.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SessionRecord {
    pub id: String,
    pub title: String,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    pub archived_at_ms: Option<i64>,
    pub forked_from_session_id: Option<String>,
    pub forked_through_entry_id: Option<String>,
    pub entries: Vec<TranscriptEntry>,
    pub context_sources: Vec<ContextSourceRef>,
}

/// One persisted transcript entry, mirroring the frontend's visible
/// `ChatEntry` shape minus the `streaming` variant — streaming
/// placeholders are never persisted, and the enum having no such
/// variant makes that a parse error rather than a convention.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum TranscriptEntry {
    #[serde(rename_all = "camelCase")]
    Message {
        message: EntryMessage,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        model_used: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        duration_ms: Option<u64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        attachment_rel_path: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        attachment_line_range: Option<LineRange>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        stats: Option<EntryStats>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        sent_in_mode: Option<SentMode>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        context_sources: Option<Vec<ContextSourceManifestItem>>,
    },
    #[serde(rename_all = "camelCase")]
    Cancelled {
        partial: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        model_used: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        duration_ms: Option<u64>,
    },
    Error {
        message: String,
    },
}

/// `{ role, content }` — the persisted subset of the frontend's
/// `ChatMessage`. Only `user` and `assistant` are representable:
/// `system` and `tool` turns are transport detail, not transcript.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct EntryMessage {
    pub role: EntryRole,
    pub content: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum EntryRole {
    User,
    Assistant,
}

/// 1-based inclusive attachment line range; the object form makes
/// half-a-range unrepresentable on the wire (D10 semantics).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LineRange {
    pub start_line: u32,
    pub end_line: u32,
}

/// Wire mirror of `crate::chat::ChatStats` (D9 telemetry), kept local
/// with `Deserialize` + `deny_unknown_fields` so persisted stats are
/// exactly the bounded known shape — arbitrary frontend state is not
/// accepted — and so this module does not depend on the chat transport.
/// If `ChatStats` ever gains a field, extend this mirror in the same
/// slice.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EntryStats {
    pub output_tokens: Option<u64>,
    pub eval_ms: Option<u64>,
    pub tokens_per_second: Option<f32>,
    pub prompt_tokens: Option<u64>,
    pub prompt_ms: Option<u64>,
}

/// Mode a user turn was sent with — the persisted form of the
/// frontend's `ChatMode` (`'chat' | 'proposeDiff'`, D15). Unknown modes
/// fail entry parsing.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum SentMode {
    Chat,
    ProposeDiff,
}

/// Where the LOCAL session database lives. Resolved from Tauri's app
/// data directory once at startup; deliberately not derivable from any
/// project so opening or closing a project cannot change which database
/// backs local chat.
pub fn local_sessions_dir(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join("sessions")
}

/// Where a project's session database lives. Refuses a symlinked
/// `.plume` up front; on every open, the sessions directory and
/// database file get the same symlink check and the database file
/// additionally gets a hardlink-alias check — same defensive posture
/// as the memory store, patch checkpoints, and `safety::path`.
pub fn project_sessions_dir(project_root: &Path) -> Result<PathBuf, SessionStoreError> {
    let plume_dir = project_root.join(".plume");
    schema::refuse_symlink(&plume_dir, ".plume")?;
    Ok(plume_dir.join("sessions"))
}

/// Create a session. `title: None` gets the default title; a provided
/// title is trimmed and bounds-checked. Fails with `Limit` once the
/// database holds the maximum number of non-deleted sessions (archived
/// ones count — archive is not a quota escape).
pub fn create(
    sessions_dir: &Path,
    title: Option<&str>,
) -> Result<SessionSummary, SessionStoreError> {
    let title = match title {
        Some(raw) => validation::validate_title(raw)?,
        None => validation::DEFAULT_TITLE.to_string(),
    };
    let lock = store_lock(sessions_dir);
    let _guard = lock.lock().expect("session store mutex poisoned");
    let conn = schema::open_connection(sessions_dir)?;

    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM chat_sessions", [], |row| row.get(0))
        .map_err(schema::storage("count sessions"))?;
    if count >= validation::MAX_SESSIONS {
        return Err(SessionStoreError::Limit(format!(
            "this chat store already holds {count} sessions (cap {}); delete some before creating more",
            validation::MAX_SESSIONS
        )));
    }

    let id = validation::mint_session_id();
    let now = now_ms();
    conn.execute(
        "INSERT INTO chat_sessions (id, title, created_at_ms, updated_at_ms, archived_at_ms)
         VALUES (?1, ?2, ?3, ?3, NULL)",
        params![id, title, now],
    )
    .map_err(schema::storage("insert session"))?;
    Ok(SessionSummary {
        id,
        title,
        created_at_ms: now,
        updated_at_ms: now,
        archived_at_ms: None,
        forked_from_session_id: None,
        forked_through_entry_id: None,
    })
}

/// List sessions, most recently updated first. Archived sessions are
/// hidden unless `include_archived`.
pub fn list(
    sessions_dir: &Path,
    include_archived: bool,
) -> Result<Vec<SessionSummary>, SessionStoreError> {
    let lock = store_lock(sessions_dir);
    let _guard = lock.lock().expect("session store mutex poisoned");
    let conn = schema::open_connection(sessions_dir)?;

    let sql = if include_archived {
        "SELECT id, title, created_at_ms, updated_at_ms, archived_at_ms,
                forked_from_session_id, forked_through_entry_id
         FROM chat_sessions ORDER BY updated_at_ms DESC, id DESC"
    } else {
        "SELECT id, title, created_at_ms, updated_at_ms, archived_at_ms,
                forked_from_session_id, forked_through_entry_id
         FROM chat_sessions WHERE archived_at_ms IS NULL
         ORDER BY updated_at_ms DESC, id DESC"
    };
    let mut stmt = conn.prepare(sql).map_err(schema::storage("prepare list"))?;
    let rows = stmt
        .query_map([], summary_from_row)
        .map_err(schema::storage("query sessions"))?;
    let mut sessions = Vec::new();
    for row in rows {
        sessions.push(row.map_err(schema::storage("read session row"))?);
    }
    Ok(sessions)
}

/// Load one session with its full persisted transcript. Malformed
/// persisted rows are rejected (`Corrupt`), not coerced.
pub fn load(sessions_dir: &Path, session_id: &str) -> Result<SessionRecord, SessionStoreError> {
    validation::validate_id(session_id)?;
    let lock = store_lock(sessions_dir);
    let _guard = lock.lock().expect("session store mutex poisoned");
    let conn = schema::open_connection(sessions_dir)?;

    let summary = fetch_summary(&conn, session_id)?;
    let mut stmt = conn
        .prepare(
            "SELECT kind, role, content, model_used, duration_ms, attachment_rel_path,
                    attachment_start_line, attachment_end_line, stats_json, sent_in_mode,
                    context_manifest_json
             FROM chat_messages WHERE session_id = ?1 ORDER BY ordinal ASC",
        )
        .map_err(schema::storage("prepare transcript load"))?;
    let rows = stmt
        .query_map(params![session_id], |row| {
            Ok(validation::RawMessageRow {
                kind: row.get(0)?,
                role: row.get(1)?,
                content: row.get(2)?,
                model_used: row.get(3)?,
                duration_ms: row.get(4)?,
                attachment_rel_path: row.get(5)?,
                attachment_start_line: row.get(6)?,
                attachment_end_line: row.get(7)?,
                stats_json: row.get(8)?,
                sent_in_mode: row.get(9)?,
                context_manifest_json: row.get(10)?,
            })
        })
        .map_err(schema::storage("query transcript"))?;
    let mut entries = Vec::new();
    for row in rows {
        let raw = row.map_err(schema::storage("read transcript row"))?;
        entries.push(validation::entry_from_row(raw)?);
    }
    Ok(SessionRecord {
        id: summary.id,
        title: summary.title,
        created_at_ms: summary.created_at_ms,
        updated_at_ms: summary.updated_at_ms,
        archived_at_ms: summary.archived_at_ms,
        forked_from_session_id: summary.forked_from_session_id,
        forked_through_entry_id: summary.forked_through_entry_id,
        entries,
        context_sources: fetch_context_sources(&conn, session_id)?,
    })
}

/// Check only whether the owning session row exists. This deliberately does
/// not deserialize transcript/context rows, so cleanup can still remove a
/// session whose child data is corrupt.
pub(crate) fn session_exists(
    sessions_dir: &Path,
    session_id: &str,
) -> Result<bool, SessionStoreError> {
    validation::validate_id(session_id)?;
    let lock = store_lock(sessions_dir);
    let _guard = lock.lock().expect("session store mutex poisoned");
    let conn = schema::open_connection(sessions_dir)?;
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM chat_sessions WHERE id = ?1)",
        params![session_id],
        |row| row.get(0),
    )
    .map_err(schema::storage("check session existence"))
}

/// Rename a session. The stored title is the trimmed form; renames bump
/// `updated_at_ms` (a rename is a user touch, and the sidebar sorts by
/// latest update).
pub fn rename(
    sessions_dir: &Path,
    session_id: &str,
    title: &str,
) -> Result<SessionSummary, SessionStoreError> {
    validation::validate_id(session_id)?;
    let title = validation::validate_title(title)?;
    let lock = store_lock(sessions_dir);
    let _guard = lock.lock().expect("session store mutex poisoned");
    let conn = schema::open_connection(sessions_dir)?;

    let changed = conn
        .execute(
            "UPDATE chat_sessions SET title = ?2, updated_at_ms = ?3 WHERE id = ?1",
            params![session_id, title, now_ms()],
        )
        .map_err(schema::storage("rename session"))?;
    if changed == 0 {
        return Err(SessionStoreError::NotFound(session_id.to_string()));
    }
    fetch_summary(&conn, session_id)
}

/// Archive or unarchive. Idempotent: asking for the state the session
/// is already in returns it unchanged. Deliberately does NOT bump
/// `updated_at_ms`, so an unarchived session reappears at its
/// historical position instead of jumping to the top.
pub fn set_archived(
    sessions_dir: &Path,
    session_id: &str,
    archived: bool,
) -> Result<SessionSummary, SessionStoreError> {
    validation::validate_id(session_id)?;
    let lock = store_lock(sessions_dir);
    let _guard = lock.lock().expect("session store mutex poisoned");
    let conn = schema::open_connection(sessions_dir)?;

    let current = fetch_summary(&conn, session_id)?;
    if current.archived_at_ms.is_some() == archived {
        return Ok(current);
    }
    let stamp: Option<i64> = archived.then(now_ms);
    conn.execute(
        "UPDATE chat_sessions SET archived_at_ms = ?2 WHERE id = ?1",
        params![session_id, stamp],
    )
    .map_err(schema::storage("archive session"))?;
    fetch_summary(&conn, session_id)
}

/// Delete a session and its messages. Explicit idempotency contract:
/// the first delete succeeds; an unknown or already-deleted id returns
/// `NotFound`.
///
/// D66: the messages are deleted by an explicit statement (in one
/// transaction with the session row) rather than left to `ON DELETE
/// CASCADE`. The explicit DELETE is guaranteed to fire the FTS
/// delete-triggers; cascade-driven deletions firing triggers is a
/// SQLite subtlety we refuse to depend on. A stale `messages_fts` row
/// would be a *correctness* bug, not just bloat: SQLite reuses rowids,
/// so a ghost index row could later point at an unrelated message.
pub fn delete(sessions_dir: &Path, session_id: &str) -> Result<(), SessionStoreError> {
    validation::validate_id(session_id)?;
    let lock = store_lock(sessions_dir);
    let _guard = lock.lock().expect("session store mutex poisoned");
    let mut conn = schema::open_connection(sessions_dir)?;

    let tx = conn
        .transaction()
        .map_err(schema::storage("begin session delete"))?;
    tx.execute(
        "DELETE FROM chat_messages WHERE session_id = ?1",
        params![session_id],
    )
    .map_err(schema::storage("delete session messages"))?;
    let deleted = tx
        .execute(
            "DELETE FROM chat_sessions WHERE id = ?1",
            params![session_id],
        )
        .map_err(schema::storage("delete session"))?;
    if deleted == 0 {
        // Roll back the (empty) message delete too.
        return Err(SessionStoreError::NotFound(session_id.to_string()));
    }
    tx.commit()
        .map_err(schema::storage("commit session delete"))?;
    Ok(())
}

/// Replace a session's persisted transcript with a validated complete
/// snapshot, atomically. Delete-old + insert-new + `updated_at_ms` bump
/// run in one transaction: any failure rolls the whole replacement
/// back, leaving the previous transcript intact.
///
/// `allow_attachments` is the scope rule: project sessions may carry
/// attachment metadata, local sessions may not. Enforced here — at the
/// store boundary — so the rule holds no matter who calls.
#[allow(dead_code)]
pub fn save_transcript(
    sessions_dir: &Path,
    session_id: &str,
    entries: &[TranscriptEntry],
    allow_attachments: bool,
) -> Result<SessionSummary, SessionStoreError> {
    save_transcript_with_context(sessions_dir, session_id, entries, &[], allow_attachments)
}

pub fn save_transcript_with_context(
    sessions_dir: &Path,
    session_id: &str,
    entries: &[TranscriptEntry],
    context_sources: &[ContextSourceRef],
    allow_attachments: bool,
) -> Result<SessionSummary, SessionStoreError> {
    validation::validate_id(session_id)?;
    validation::validate_entries(entries, allow_attachments)?;
    validation::validate_context_sources(context_sources, allow_attachments)?;
    let lock = store_lock(sessions_dir);
    let _guard = lock.lock().expect("session store mutex poisoned");
    let mut conn = schema::open_connection(sessions_dir)?;

    let tx = conn
        .transaction()
        .map_err(schema::storage("begin transcript replacement"))?;
    let exists: Option<String> = tx
        .query_row(
            "SELECT id FROM chat_sessions WHERE id = ?1",
            params![session_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(schema::storage("look up session"))?;
    if exists.is_none() {
        return Err(SessionStoreError::NotFound(session_id.to_string()));
    }
    tx.execute(
        "DELETE FROM chat_messages WHERE session_id = ?1",
        params![session_id],
    )
    .map_err(schema::storage("clear previous transcript"))?;
    let now = now_ms();
    for (ordinal, entry) in entries.iter().enumerate() {
        let row = validation::row_from_entry(entry)?;
        tx.execute(
            "INSERT INTO chat_messages (
               id, session_id, ordinal, kind, role, content, model_used, duration_ms,
               attachment_rel_path, attachment_start_line, attachment_end_line,
               stats_json, sent_in_mode, context_manifest_json, created_at_ms
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
            params![
                validation::mint_message_id(),
                session_id,
                ordinal as i64,
                row.kind,
                row.role,
                row.content,
                row.model_used,
                row.duration_ms,
                row.attachment_rel_path,
                row.attachment_start_line,
                row.attachment_end_line,
                row.stats_json,
                row.sent_in_mode,
                row.context_manifest_json,
                now,
            ],
        )
        .map_err(schema::storage("insert transcript entry"))?;
    }
    tx.execute(
        "UPDATE chat_sessions SET updated_at_ms = ?2, context_sources_json = ?3 WHERE id = ?1",
        params![
            session_id,
            now,
            validation::serialize_context_sources(context_sources)?
        ],
    )
    .map_err(schema::storage("stamp session update"))?;
    tx.commit()
        .map_err(schema::storage("commit transcript replacement"))?;
    fetch_summary(&conn, session_id)
}

fn fetch_summary(conn: &Connection, session_id: &str) -> Result<SessionSummary, SessionStoreError> {
    conn.query_row(
        "SELECT id, title, created_at_ms, updated_at_ms, archived_at_ms,
                forked_from_session_id, forked_through_entry_id
         FROM chat_sessions WHERE id = ?1",
        params![session_id],
        summary_from_row,
    )
    .optional()
    .map_err(schema::storage("read session"))?
    .ok_or_else(|| SessionStoreError::NotFound(session_id.to_string()))
}

fn fetch_context_sources(
    conn: &Connection,
    session_id: &str,
) -> Result<Vec<ContextSourceRef>, SessionStoreError> {
    let json: Option<String> = conn
        .query_row(
            "SELECT context_sources_json FROM chat_sessions WHERE id = ?1",
            params![session_id],
            |row| row.get(0),
        )
        .map_err(schema::storage("read session context shelf"))?;
    validation::parse_context_sources(json.as_deref())
}

fn summary_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<SessionSummary> {
    Ok(SessionSummary {
        id: row.get(0)?,
        title: row.get(1)?,
        created_at_ms: row.get(2)?,
        updated_at_ms: row.get(3)?,
        archived_at_ms: row.get(4)?,
        forked_from_session_id: row.get(5)?,
        forked_through_entry_id: row.get(6)?,
    })
}

/// Atomically copy a persisted thread into a new live session. The mutex is
/// acquired exactly once and every read/validation/insert stays in the same
/// IMMEDIATE transaction, so a corrupt row or quota race leaves no child.
pub fn fork(
    sessions_dir: &Path,
    source_id: &str,
    allow_attachments: bool,
) -> Result<SessionRecord, SessionStoreError> {
    branch::fork(sessions_dir, source_id, allow_attachments)
}

/// Create a non-destructive branch omitting the last `turn_count` user turns.
pub fn rollback(
    sessions_dir: &Path,
    source_id: &str,
    turn_count: u32,
    allow_attachments: bool,
) -> Result<SessionRecord, SessionStoreError> {
    branch::rollback(sessions_dir, source_id, turn_count, allow_attachments)
}

fn load_unlocked(conn: &Connection, session_id: &str) -> Result<SessionRecord, SessionStoreError> {
    let summary = fetch_summary(conn, session_id)?;
    let mut stmt = conn
        .prepare(
            "SELECT kind, role, content, model_used, duration_ms, attachment_rel_path,
                attachment_start_line, attachment_end_line, stats_json, sent_in_mode,
                context_manifest_json
         FROM chat_messages WHERE session_id = ?1 ORDER BY ordinal ASC",
        )
        .map_err(schema::storage("prepare transcript load"))?;
    let rows = stmt
        .query_map(params![session_id], |row| {
            Ok(validation::RawMessageRow {
                kind: row.get(0)?,
                role: row.get(1)?,
                content: row.get(2)?,
                model_used: row.get(3)?,
                duration_ms: row.get(4)?,
                attachment_rel_path: row.get(5)?,
                attachment_start_line: row.get(6)?,
                attachment_end_line: row.get(7)?,
                stats_json: row.get(8)?,
                sent_in_mode: row.get(9)?,
                context_manifest_json: row.get(10)?,
            })
        })
        .map_err(schema::storage("query transcript"))?;
    let mut entries = Vec::new();
    for row in rows {
        entries.push(validation::entry_from_row(
            row.map_err(schema::storage("read transcript row"))?,
        )?);
    }
    Ok(SessionRecord {
        id: summary.id,
        title: summary.title,
        created_at_ms: summary.created_at_ms,
        updated_at_ms: summary.updated_at_ms,
        archived_at_ms: summary.archived_at_ms,
        forked_from_session_id: summary.forked_from_session_id,
        forked_through_entry_id: summary.forked_through_entry_id,
        entries,
        context_sources: fetch_context_sources(conn, session_id)?,
    })
}

/// Per-directory store mutex. Bounded per the repo's collection rule:
/// past a small threshold, entries nobody currently holds are swept.
/// The map only ever holds one entry per distinct sessions directory
/// touched this process (local + one per opened project).
fn store_lock(sessions_dir: &Path) -> Arc<Mutex<()>> {
    static LOCKS: OnceLock<Mutex<HashMap<PathBuf, Arc<Mutex<()>>>>> = OnceLock::new();
    const SWEEP_THRESHOLD: usize = 64;
    let registry = LOCKS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut map = registry.lock().expect("session lock registry poisoned");
    if map.len() > SWEEP_THRESHOLD {
        map.retain(|_, lock| Arc::strong_count(lock) > 1);
    }
    map.entry(sessions_dir.to_path_buf())
        .or_insert_with(|| Arc::new(Mutex::new(())))
        .clone()
}

/// Wall-clock milliseconds, nudged monotonic: two calls never return
/// the same value, so `updated_at_ms` ordering is total even for
/// operations landing within one millisecond.
fn now_ms() -> i64 {
    static LAST: AtomicI64 = AtomicI64::new(0);
    let real = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    let prev = LAST
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |last| {
            Some(real.max(last + 1))
        })
        .unwrap_or(real);
    real.max(prev + 1)
}
