//! Atomic transcript branching shared by whole-thread fork and rewind.

use std::path::Path;

use rusqlite::{params, OptionalExtension, TransactionBehavior};

use super::{load_unlocked, now_ms, schema, store_lock, summary_from_row, validation};
use super::{EntryRole, SessionRecord, SessionStoreError, TranscriptEntry};

pub(super) fn fork(
    sessions_dir: &Path,
    source_id: &str,
    allow_attachments: bool,
) -> Result<SessionRecord, SessionStoreError> {
    branch(sessions_dir, source_id, allow_attachments, None)
}

pub(super) fn rollback(
    sessions_dir: &Path,
    source_id: &str,
    turn_count: u32,
    allow_attachments: bool,
) -> Result<SessionRecord, SessionStoreError> {
    if !(1..=20).contains(&turn_count) {
        return Err(SessionStoreError::Invalid(
            "turnCount must be between 1 and 20".into(),
        ));
    }
    branch(sessions_dir, source_id, allow_attachments, Some(turn_count))
}

fn branch(
    sessions_dir: &Path,
    source_id: &str,
    allow_attachments: bool,
    rollback_turns: Option<u32>,
) -> Result<SessionRecord, SessionStoreError> {
    validation::validate_id(source_id)?;
    let lock = store_lock(sessions_dir);
    let _guard = lock.lock().expect("session store mutex poisoned");
    let mut conn = schema::open_connection(sessions_dir)?;
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(schema::storage("begin session branch"))?;
    let source = tx
        .query_row(
            "SELECT id, title, created_at_ms, updated_at_ms, archived_at_ms,
                    forked_from_session_id, forked_through_entry_id
             FROM chat_sessions WHERE id = ?1",
            params![source_id],
            summary_from_row,
        )
        .optional()
        .map_err(schema::storage("read branch source"))?
        .ok_or_else(|| SessionStoreError::NotFound(source_id.to_string()))?;
    let count: i64 = tx
        .query_row("SELECT COUNT(*) FROM chat_sessions", [], |row| row.get(0))
        .map_err(schema::storage("count sessions for branch"))?;
    if count >= validation::MAX_SESSIONS {
        return Err(SessionStoreError::Limit(format!(
            "this chat store already holds {count} sessions (cap {})",
            validation::MAX_SESSIONS
        )));
    }

    let mut stmt = tx
        .prepare(
            "SELECT id, kind, role, content, model_used, duration_ms,
                    attachment_rel_path, attachment_start_line, attachment_end_line,
                    stats_json, sent_in_mode, context_manifest_json, artifact_json
             FROM chat_messages WHERE session_id = ?1 ORDER BY ordinal ASC",
        )
        .map_err(schema::storage("prepare branch transcript"))?;
    let rows = stmt
        .query_map(params![source_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
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
            ))
        })
        .map_err(schema::storage("query branch transcript"))?;
    let mut source_rows = Vec::new();
    for row in rows {
        let (id, raw) = row.map_err(schema::storage("read branch transcript row"))?;
        validation::validate_id(&id).map_err(as_corrupt)?;
        source_rows.push((id, validation::entry_from_row(raw)?));
    }
    drop(stmt);
    let all_entries: Vec<_> = source_rows.iter().map(|(_, entry)| entry.clone()).collect();
    validation::validate_entries(&all_entries, allow_attachments).map_err(as_corrupt)?;

    let keep = match rollback_turns {
        None => source_rows.len(),
        Some(turns) => rollback_cutoff(&source_rows, turns)?,
    };
    let retained = &source_rows[..keep];
    let through = retained.last().map(|(id, _)| id.clone());
    let suffix = rollback_turns
        .map(|turns| format!(" (rewound {turns})"))
        .unwrap_or_else(|| " (continued)".into());
    let max_base = 120usize.saturating_sub(suffix.chars().count());
    let mut base: String = source.title.trim().chars().take(max_base).collect();
    if base.is_empty() {
        base = validation::DEFAULT_TITLE.to_string();
    }
    let title = format!("{base}{suffix}");
    let id = validation::mint_session_id();
    let now = now_ms();
    tx.execute(
        "INSERT INTO chat_sessions
         (id, title, created_at_ms, updated_at_ms, archived_at_ms,
          forked_from_session_id, forked_through_entry_id)
         VALUES (?1, ?2, ?3, ?3, NULL, ?4, ?5)",
        params![id, title, now, source_id, through],
    )
    .map_err(schema::storage("insert branched session"))?;
    for (ordinal, (_, entry)) in retained.iter().enumerate() {
        let row = validation::row_from_entry(entry)?;
        tx.execute(
            "INSERT INTO chat_messages
             (id, session_id, ordinal, kind, role, content, model_used, duration_ms,
              attachment_rel_path, attachment_start_line, attachment_end_line,
              stats_json, sent_in_mode, context_manifest_json, artifact_json, created_at_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
            params![
                validation::mint_message_id(),
                id,
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
                now
            ],
        )
        .map_err(schema::storage("copy branch transcript entry"))?;
    }
    tx.commit()
        .map_err(schema::storage("commit session branch"))?;
    load_unlocked(&conn, &id)
}

fn rollback_cutoff(
    rows: &[(String, TranscriptEntry)],
    turn_count: u32,
) -> Result<usize, SessionStoreError> {
    if !rows.is_empty() && !is_user(&rows[0].1) {
        return Err(SessionStoreError::Corrupt(
            "rollback source transcript starts before its first user turn".into(),
        ));
    }
    let starts: Vec<_> = rows
        .iter()
        .enumerate()
        .filter_map(|(index, (_, entry))| is_user(entry).then_some(index))
        .collect();
    if starts.len() < turn_count as usize {
        return Err(SessionStoreError::Invalid(format!(
            "source has {} user turns; cannot rewind {turn_count}",
            starts.len()
        )));
    }
    Ok(starts[starts.len() - turn_count as usize])
}

fn is_user(entry: &TranscriptEntry) -> bool {
    matches!(entry, TranscriptEntry::Message { message, .. } if message.role == EntryRole::User)
}

fn as_corrupt(error: SessionStoreError) -> SessionStoreError {
    match error {
        SessionStoreError::Invalid(message) | SessionStoreError::Limit(message) => {
            SessionStoreError::Corrupt(message)
        }
        other => other,
    }
}
