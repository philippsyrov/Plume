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
               stats_json, sent_in_mode, context_manifest_json, artifact_json, created_at_ms
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
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
                row.artifact_json,
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
