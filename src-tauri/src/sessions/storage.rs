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
/// Two rules, and the order matters.
///
/// A save that shrinks a conversation, or leaves it the same size, always
/// lands. Otherwise a user who had filled the store could not edit their way
/// back under it, and the only exit would be deleting whole conversations.
///
/// Otherwise the decision is on *projected* usage, not on whether the store is
/// already full. Asking only "is it full yet?" would admit any single write
/// while one page remained — and a transcript may be up to
/// `MAX_TRANSCRIPT_BYTES`, so one save could carry the store megabytes past a
/// cap it was still under a moment earlier.
pub(super) fn admits_write(usage: StorageUsage, existing_bytes: u64, incoming_bytes: u64) -> bool {
    if incoming_bytes <= existing_bytes {
        return true;
    }
    let projected = usage
        .used_bytes
        .saturating_sub(existing_bytes)
        .saturating_add(incoming_bytes);
    projected <= usage.cap_bytes
}

/// The refusal a caller surfaces when [`admits_write`] says no.
///
/// It carries the numbers rather than a finished sentence: the remedy is
/// user-facing copy and belongs with the rest of it in the frontend, while this
/// message is log-grade. What matters at this boundary is the *type* — the
/// surface decides what to say from `SessionStoreError::StorageFull`, never by
/// reading text, and it cannot re-derive the state from a later usage read
/// because this decision was made on projected usage.
pub(super) fn full_store_refusal(usage: StorageUsage) -> SessionStoreError {
    SessionStoreError::StorageFull {
        used_bytes: usage.used_bytes,
        cap_bytes: usage.cap_bytes,
    }
}

/// What the store holds and where its limits sit, for the surface that warns
/// the user before writes stop.
pub fn storage_usage(sessions_dir: &std::path::Path) -> Result<StorageUsage, SessionStoreError> {
    let lock = super::store_lock(sessions_dir);
    let _guard = lock.lock().expect("session store mutex poisoned");
    let conn = schema::open_connection(sessions_dir)?;
    usage(&conn)
}

/// Refuse a branch that would carry the store past its budget.
///
/// A branch copies a transcript into a *new* session, so nothing is freed and
/// the whole copy is growth: `existing_bytes` is 0. That is the one rule a
/// branch does not share with [`admits_transcript`], which replaces a thread
/// and can therefore shrink — which is what lets an unchanged-size save still
/// land at the cap.
///
/// It measures the entries actually copied, not the source's whole transcript.
/// A rewind keeps only a prefix, and charging it for the turns it is about to
/// drop would refuse branches that fit.
pub(super) fn admits_branch(
    usage: StorageUsage,
    entries: &[super::TranscriptEntry],
) -> Result<(), SessionStoreError> {
    let incoming_bytes = entries
        .iter()
        .map(|entry| super::validation::entry_row_len(entry) as u64)
        .try_fold(0_u64, |total, len| total.checked_add(len))
        .ok_or_else(|| SessionStoreError::Limit("branch byte accounting overflow".into()))?;

    if admits_write(usage, 0, incoming_bytes) {
        return Ok(());
    }
    Err(full_store_refusal(usage))
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
