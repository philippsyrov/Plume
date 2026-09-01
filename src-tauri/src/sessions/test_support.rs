//! Cross-module fixtures for command tests that exercise private session state.

use std::path::Path;

use rusqlite::params;

pub(crate) use super::checkpoint::{
    CheckpointFact, CheckpointValidationStatus, CompactionCheckpoint, FactKind, FactProvenance,
    MemoryProvenance, MemoryScope,
};
use super::{checkpoint, schema, store_lock, validation, SessionStoreError};

pub(crate) fn transcript_entry_ids(
    sessions_dir: &Path,
    session_id: &str,
) -> Result<Vec<String>, SessionStoreError> {
    validation::validate_id(session_id)?;
    let lock = store_lock(sessions_dir);
    let _guard = lock
        .lock()
        .map_err(|_| SessionStoreError::Storage("session store lock poisoned".into()))?;
    let conn = schema::open_connection(sessions_dir)?;
    let mut statement = conn
        .prepare("SELECT id FROM chat_messages WHERE session_id=?1 ORDER BY ordinal")
        .map_err(|error| {
            SessionStoreError::Storage(format!("prepare test transcript ids: {error}"))
        })?;
    let rows = statement
        .query_map(params![session_id], |row| row.get(0))
        .map_err(|error| {
            SessionStoreError::Storage(format!("query test transcript ids: {error}"))
        })?;
    rows.map(|row| {
        row.map_err(|error| SessionStoreError::Storage(format!("read test transcript id: {error}")))
    })
    .collect()
}

pub(crate) fn save_checkpoint(
    sessions_dir: &Path,
    value: &CompactionCheckpoint,
) -> Result<(), SessionStoreError> {
    checkpoint::save_checkpoint(sessions_dir, value)
}
