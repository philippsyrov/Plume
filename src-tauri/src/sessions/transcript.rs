//! Replacing a conversation's stored transcript.
//!
//! One write path, and the only one that grows a session: `sessions.save`
//! hands over the whole thread, so this replaces every row rather than
//! appending. The storage cap is consulted before anything is mutated — see
//! [`super::storage::admits_transcript`].

use std::path::Path;

use rusqlite::{params, OptionalExtension};

use super::{
    fetch_summary, now_ms, schema, storage, store_lock, validation, ContextSourceRef,
    SessionStoreError, SessionSummary, TranscriptEntry,
};

struct StoredEntry {
    id: String,
    entry: TranscriptEntry,
    created_at_ms: i64,
}

fn stored_entries(
    tx: &rusqlite::Transaction<'_>,
    session_id: &str,
) -> Result<Vec<StoredEntry>, SessionStoreError> {
    let mut stmt = tx
        .prepare(
            "SELECT id, kind, role, content, model_used, duration_ms, attachment_rel_path,
                    attachment_start_line, attachment_end_line, stats_json, sent_in_mode,
                    context_manifest_json, artifact_json, created_at_ms
             FROM chat_messages WHERE session_id = ?1 ORDER BY ordinal ASC",
        )
        .map_err(schema::storage("prepare existing transcript load"))?;
    let rows = stmt
        .query_map(params![session_id], |row| {
            Ok((
                row.get(0)?,
                validation::RawMessageRow {
                    kind: row.get(1)?,
                    role: row.get(2)?,
                    content: row.get(3)?,
                    model_used: row.get(4)?,
                    duration_ms: row.get(5)?,
                    attachment_rel_path: row.get(6)?,
                    attachment_start_line: row.get(7)?,
                    attachment_end_line: row.get(8)?,
                    stats_json: row.get(9)?,
                    sent_in_mode: row.get(10)?,
                    context_manifest_json: row.get(11)?,
                    artifact_json: row.get(12)?,
                },
                row.get(13)?,
            ))
        })
        .map_err(schema::storage("query existing transcript"))?;
    let mut stored = Vec::new();
    for row in rows {
        let (id, raw, created_at_ms) =
            row.map_err(schema::storage("read existing transcript row"))?;
        stored.push(StoredEntry {
            id,
            entry: validation::entry_from_row(raw)?,
            created_at_ms,
        });
    }
    Ok(stored)
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

    // Refuse before mutating. A transcript save replaces the whole thread, so
    // the question is not "may we append?" but "does this replacement grow a
    // store that is already full?". Shrinking and unchanged saves still land,
    // which is what lets a user edit their way back under the cap instead of
    // having to delete whole conversations. Nothing is ever trimmed to make
    // room: see docs/…-design.md § Durable storage policy.
    storage::admits_transcript(&tx, session_id, entries)?;
    let stored = stored_entries(&tx, session_id)?;

    tx.execute(
        "DELETE FROM chat_messages WHERE session_id = ?1",
        params![session_id],
    )
    .map_err(schema::storage("clear previous transcript"))?;
    let now = now_ms();
    for (ordinal, entry) in entries.iter().enumerate() {
        let row = validation::row_from_entry(entry)?;
        let preserved = stored.get(ordinal).filter(|stored| stored.entry == *entry);
        let id = preserved
            .map(|stored| stored.id.clone())
            .unwrap_or_else(validation::mint_message_id);
        let created_at_ms = preserved.map_or(now, |stored| stored.created_at_ms);
        tx.execute(
            "INSERT INTO chat_messages (
               id, session_id, ordinal, kind, role, content, model_used, duration_ms,
               attachment_rel_path, attachment_start_line, attachment_end_line,
               stats_json, sent_in_mode, context_manifest_json, artifact_json, created_at_ms
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
            params![
                id,
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
                row.artifact_json,
                created_at_ms,
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
