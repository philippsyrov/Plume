//! Durable storage policy for a chat session store.
//!
//! Retention is only an honest promise if there is a policy for the moment the
//! disk runs out. The tempting failure — quietly trimming the oldest turns —
//! would break the one guarantee the conversation design rests on, so this
//! module refuses instead. A refusal is a failure the user can see and act on;
//! a silent deletion is one they cannot.
//!
//! See `docs/superpowers/specs/2026-08-27-continuous-chat-folder-grants-design.md`
//! § Durable storage policy.

use rusqlite::Connection;

use super::{schema, SessionStoreError};

/// Byte budget for one session store. Calibrated to be far above ordinary use
/// — a store of text transcripts reaches this only after sustained heavy use —
/// while staying small enough that a runaway writer is stopped long before it
/// fills a disk.
pub(super) const MAX_STORE_BYTES: u64 = 512 * 1024 * 1024;

/// Warn from nine tenths of the budget. Far enough ahead that the user has room
/// to export or delete before writes stop, late enough that the warning is not
/// background noise.
pub(super) const STORE_WARN_BYTES: u64 = MAX_STORE_BYTES / 10 * 9;

/// What the store currently holds, and where its limits sit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageUsage {
    pub used_bytes: u64,
    pub warn_bytes: u64,
    pub cap_bytes: u64,
}

impl StorageUsage {
    pub fn is_full(&self) -> bool {
        self.used_bytes >= self.cap_bytes
    }
}

/// Measure the store as **pages in use**: `page_count - freelist_count`.
///
/// Neither of the obvious alternatives works. File size on disk does not shrink
/// after a delete until the database is vacuumed, and `page_count` alone does
/// not either — SQLite moves emptied pages onto a freelist and reuses them
/// rather than returning them to the filesystem. Either measure would keep
/// refusing writes after the user had already made room by deleting a
/// conversation, which would turn the documented recovery path into a dead end.
/// Subtracting the freelist reflects the deletion immediately.
pub(super) fn usage(conn: &Connection) -> Result<StorageUsage, SessionStoreError> {
    let page_count: i64 = conn
        .query_row("PRAGMA page_count", [], |row| row.get(0))
        .map_err(schema::storage("read store page count"))?;
    let free_pages: i64 = conn
        .query_row("PRAGMA freelist_count", [], |row| row.get(0))
        .map_err(schema::storage("read store freelist"))?;
    let page_size: i64 = conn
        .query_row("PRAGMA page_size", [], |row| row.get(0))
        .map_err(schema::storage("read store page size"))?;

    let used_pages = page_count.saturating_sub(free_pages).max(0);
    let used_bytes = u64::try_from(used_pages)
        .unwrap_or(0)
        .saturating_mul(u64::try_from(page_size).unwrap_or(0));

    Ok(StorageUsage {
        used_bytes,
        warn_bytes: STORE_WARN_BYTES,
        cap_bytes: MAX_STORE_BYTES,
    })
}

/// Whether a write that replaces `existing_bytes` with `incoming_bytes` may
/// proceed.
///
/// Being at the cap does not freeze the store outright. A save that shrinks a
/// conversation, or leaves it the same size, still lands — otherwise a user who
/// had filled the store could not edit their way back under it, and the only
/// exit would be deleting whole conversations. Only growth is refused.
pub(super) fn admits_write(usage: StorageUsage, existing_bytes: u64, incoming_bytes: u64) -> bool {
    if !usage.is_full() {
        return true;
    }
    incoming_bytes <= existing_bytes
}

/// The refusal a caller surfaces when [`admits_write`] says no.
///
/// The message names what is full and what the user can do about it, because
/// this string is what reaches them — `SessionStoreError::Limit` maps to
/// `IpcError::Blocked`, whose details are rendered verbatim.
pub(super) fn full_store_refusal(usage: StorageUsage) -> SessionStoreError {
    SessionStoreError::Limit(format!(
        "this chat store is full ({} MB of {} MB). Nothing has been deleted and \
         your existing chats are still readable. Delete a conversation you no \
         longer need to make room; new messages cannot be saved until then.",
        usage.used_bytes / (1024 * 1024),
        usage.cap_bytes / (1024 * 1024),
    ))
}

/// What the store holds and where its limits sit, for the surface that warns
/// the user before writes stop.
pub fn storage_usage(sessions_dir: &std::path::Path) -> Result<StorageUsage, SessionStoreError> {
    let lock = super::store_lock(sessions_dir);
    let _guard = lock.lock().expect("session store mutex poisoned");
    let conn = schema::open_connection(sessions_dir)?;
    usage(&conn)
}

/// Refuse a transcript replacement that would grow an already-full store.
///
/// Called before any mutation, inside the caller's transaction. A save replaces
/// the whole thread, so the question is not "may we append?" but "does this
/// replacement grow the store?" — which is why shrinking and unchanged saves
/// still land at the cap.
pub(super) fn admits_transcript(
    tx: &Connection,
    session_id: &str,
    entries: &[super::TranscriptEntry],
) -> Result<(), SessionStoreError> {
    let usage = usage(tx)?;
    if !usage.is_full() {
        return Ok(());
    }

    // Every text column the store writes, matching `entry_row_len` on the
    // incoming side. Counting `content` alone would call a save unchanged while
    // its manifests grew.
    let existing_bytes: i64 = tx
        .query_row(
            "SELECT COALESCE(SUM(
                 LENGTH(CAST(content AS BLOB))
                 + LENGTH(CAST(COALESCE(stats_json, '') AS BLOB))
                 + LENGTH(CAST(COALESCE(context_manifest_json, '') AS BLOB))
                 + LENGTH(CAST(COALESCE(artifact_json, '') AS BLOB))
               ), 0) FROM chat_messages WHERE session_id = ?1",
            rusqlite::params![session_id],
            |row| row.get(0),
        )
        .map_err(schema::storage("measure stored transcript"))?;

    let incoming_bytes = entries
        .iter()
        .map(|entry| super::validation::entry_row_len(entry) as u64)
        .try_fold(0_u64, |total, len| total.checked_add(len))
        .ok_or_else(|| SessionStoreError::Limit("transcript byte accounting overflow".into()))?;

    if admits_write(
        usage,
        u64::try_from(existing_bytes).unwrap_or(0),
        incoming_bytes,
    ) {
        return Ok(());
    }
    Err(full_store_refusal(usage))
}
