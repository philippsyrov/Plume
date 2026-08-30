//! Compaction checkpoints: derived, immutable, and never authoritative.
//!
//! A checkpoint summarizes an inclusive range of a conversation so a long
//! thread can keep going without a new chat. The durable transcript stays the
//! source record — a checkpoint is added, never substituted for what it
//! summarizes, and it is deleted with its conversation and no other way.
//!
//! Two rules live here. [`resolve_facts`] keeps a forgotten fact out of the
//! current projection. [`forgotten_turn_ids`] keeps it from coming back on the
//! next rebuild, which is the harder half: history is never deleted, so the
//! turn the fact was summarized from is still sitting there to be summarized
//! again.
//!
//! The first rule alone is not enough. Every fact a
//! checkpoint carries names where it came from, and that provenance is
//! re-checked on every use rather than trusted from the last one. Without that,
//! compaction quietly defeats forget: a fact copied into a checkpoint outlives
//! the memory entry it came from, and because the next compaction summarizes
//! the checkpoint rather than the source, each generation launders the fact
//! further from anything the user can inspect or revoke.
//!
//! See `docs/superpowers/specs/2026-08-27-continuous-chat-folder-grants-design.md`
//! § Conversation projection and compaction.

// Phase 2B has an internal durable store, but no projection or trigger calls it
// yet. Keeping the module private makes that scaffold impossible to mistake
// for reachable compaction behavior.
#![allow(dead_code)]

use std::collections::{HashMap, HashSet};
use std::path::Path;

use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};

use crate::prompts::{validate_context_manifest, ContextSourceManifestItem};

use super::{schema, store_lock, validation, SessionStoreError};

const MAX_CHECKPOINT_PAYLOAD_BYTES: usize = 1024 * 1024;
const MAX_CHECKPOINT_FACTS: usize = 256;
const MAX_SOURCE_TURNS_PER_FACT: usize = 64;
const MAX_ACCEPTED_MANIFEST_IDS: usize = 256;

/// Whether a generated checkpoint passed the validation step that makes it
/// eligible for prompt projection. Invalid attempts remain inspectable history
/// but are never selected by [`latest_valid_checkpoint`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CheckpointValidationStatus {
    Valid,
    Invalid,
}

impl CheckpointValidationStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Valid => "valid",
            Self::Invalid => "invalid",
        }
    }
}

/// One immutable compaction result.
///
/// Boundary ids point into the durable transcript. The payload is derived
/// state: useful for projection, but never a replacement for the transcript or
/// an authority to read anything the accepted-turn manifests did not record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CompactionCheckpoint {
    pub id: String,
    pub session_id: String,
    pub through_entry_id: String,
    pub first_retained_entry_id: String,
    pub summary: String,
    pub facts: Vec<CheckpointFact>,
    /// Transcript entry ids whose persisted, accepted context manifests were
    /// used by this checkpoint. Manifests have no independent durable id, so
    /// their owning transcript rows are the stable private references.
    pub accepted_source_manifest_ids: Vec<String>,
    pub model_id: String,
    pub runtime_id: String,
    pub prompt_version: String,
    pub tokens_before: u64,
    pub tokens_after: u64,
    pub created_at_ms: i64,
    pub supersedes_checkpoint_id: Option<String>,
    pub validation_status: CheckpointValidationStatus,
}

/// Append a checkpoint to one conversation's immutable history.
pub(super) fn save_checkpoint(
    sessions_dir: &Path,
    checkpoint: &CompactionCheckpoint,
) -> Result<(), SessionStoreError> {
    save_checkpoint_with_cap(sessions_dir, checkpoint, super::storage::MAX_STORE_BYTES)
}

pub(super) fn save_checkpoint_with_cap(
    sessions_dir: &Path,
    checkpoint: &CompactionCheckpoint,
    cap_bytes: u64,
) -> Result<(), SessionStoreError> {
    validate_checkpoint_shape(checkpoint)?;
    let payload_json = serde_json::to_string(checkpoint)
        .map_err(|e| SessionStoreError::Invalid(format!("serialize checkpoint: {e}")))?;
    if payload_json.len() > MAX_CHECKPOINT_PAYLOAD_BYTES {
        return Err(SessionStoreError::Limit(format!(
            "checkpoint payload exceeds {MAX_CHECKPOINT_PAYLOAD_BYTES} bytes"
        )));
    }

    let lock = store_lock(sessions_dir);
    let _guard = lock
        .lock()
        .map_err(|_| SessionStoreError::Storage("session store lock poisoned".to_string()))?;
    let mut conn = schema::open_connection(sessions_dir)?;
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(storage("begin checkpoint append"))?;
    let before = checkpoint_usage(&tx, cap_bytes)?;
    if !super::storage::admits_branch_usage(before, before) {
        return Err(super::storage::full_store_refusal(before));
    }
    require_session(&tx, &checkpoint.session_id)?;
    require_ordered_boundaries(&tx, checkpoint)?;
    require_owned_fact_sources(&tx, checkpoint)?;
    require_owned_manifests(&tx, checkpoint)?;
    require_valid_supersedes(&tx, checkpoint)?;

    let duplicate: bool = tx
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM compaction_checkpoints WHERE id=?1)",
            params![checkpoint.id],
            |row| row.get(0),
        )
        .map_err(storage("check checkpoint id"))?;
    if duplicate {
        return Err(SessionStoreError::Invalid(
            "checkpoint id already exists".to_string(),
        ));
    }

    tx.execute(
        "INSERT INTO compaction_checkpoints
         (id,session_id,through_entry_id,first_retained_entry_id,payload_json,
          validation_status,created_at_ms,supersedes_checkpoint_id)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
        params![
            checkpoint.id,
            checkpoint.session_id,
            checkpoint.through_entry_id,
            checkpoint.first_retained_entry_id,
            payload_json,
            checkpoint.validation_status.as_str(),
            checkpoint.created_at_ms,
            checkpoint.supersedes_checkpoint_id,
        ],
    )
    .map_err(storage("insert compaction checkpoint"))?;
    let after = checkpoint_usage(&tx, cap_bytes)?;
    if !super::storage::admits_branch_usage(before, after) {
        return Err(super::storage::full_store_refusal(before));
    }
    tx.commit().map_err(storage("commit checkpoint append"))
}

fn checkpoint_usage(
    conn: &Connection,
    cap_bytes: u64,
) -> Result<super::storage::StorageUsage, SessionStoreError> {
    let mut usage = super::storage::usage(conn)?;
    usage.cap_bytes = cap_bytes;
    usage.warn_bytes = cap_bytes / 10 * 9;
    Ok(usage)
}

/// Return every checkpoint attempt in deterministic creation order.
pub(super) fn list_checkpoints(
    sessions_dir: &Path,
    session_id: &str,
) -> Result<Vec<CompactionCheckpoint>, SessionStoreError> {
    validation::validate_id(session_id)?;
    let lock = store_lock(sessions_dir);
    let _guard = lock
        .lock()
        .map_err(|_| SessionStoreError::Storage("session store lock poisoned".to_string()))?;
    let conn = schema::open_connection(sessions_dir)?;
    require_session(&conn, session_id)?;

    let mut statement = conn
        .prepare(
            "SELECT id,session_id,through_entry_id,first_retained_entry_id,payload_json,
                    validation_status,created_at_ms,supersedes_checkpoint_id
             FROM compaction_checkpoints WHERE session_id=?1
             ORDER BY created_at_ms ASC,id ASC",
        )
        .map_err(storage("prepare checkpoint list"))?;
    let rows = statement
        .query_map(params![session_id], checkpoint_from_row)
        .map_err(storage("query checkpoints"))?;
    rows.map(|row| {
        row.map_err(storage("read checkpoint row"))
            .and_then(parse_checkpoint_row)
    })
    .collect()
}

/// Select the newest valid checkpoint without treating a newer failed attempt
/// as usable state.
pub(super) fn latest_valid_checkpoint(
    sessions_dir: &Path,
    session_id: &str,
) -> Result<Option<CompactionCheckpoint>, SessionStoreError> {
    validation::validate_id(session_id)?;
    let lock = store_lock(sessions_dir);
    let _guard = lock
        .lock()
        .map_err(|_| SessionStoreError::Storage("session store lock poisoned".to_string()))?;
    let conn = schema::open_connection(sessions_dir)?;
    require_session(&conn, session_id)?;

    conn.query_row(
        "SELECT id,session_id,through_entry_id,first_retained_entry_id,payload_json,
                validation_status,created_at_ms,supersedes_checkpoint_id
         FROM compaction_checkpoints
         WHERE session_id=?1 AND validation_status='valid'
         ORDER BY created_at_ms DESC,id DESC LIMIT 1",
        params![session_id],
        checkpoint_from_row,
    )
    .optional()
    .map_err(storage("query latest valid checkpoint"))?
    .map(parse_checkpoint_row)
    .transpose()
}

type CheckpointRow = (
    String,
    String,
    String,
    String,
    String,
    String,
    i64,
    Option<String>,
);

fn checkpoint_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<CheckpointRow> {
    Ok((
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
        row.get(5)?,
        row.get(6)?,
        row.get(7)?,
    ))
}

fn parse_checkpoint_row(row: CheckpointRow) -> Result<CompactionCheckpoint, SessionStoreError> {
    let (id, session_id, through_id, retained_id, payload, status, created_at, supersedes) = row;
    let checkpoint: CompactionCheckpoint = serde_json::from_str(&payload)
        .map_err(|e| SessionStoreError::Corrupt(format!("malformed checkpoint {id}: {e}")))?;
    let expected_status = checkpoint.validation_status.as_str();
    if checkpoint.id != id
        || checkpoint.session_id != session_id
        || checkpoint.through_entry_id != through_id
        || checkpoint.first_retained_entry_id != retained_id
        || expected_status != status
        || checkpoint.created_at_ms != created_at
        || checkpoint.supersedes_checkpoint_id != supersedes
    {
        return Err(SessionStoreError::Corrupt(format!(
            "checkpoint {id} metadata does not match its payload"
        )));
    }
    Ok(checkpoint)
}

fn validate_checkpoint_shape(checkpoint: &CompactionCheckpoint) -> Result<(), SessionStoreError> {
    validation::validate_id(&checkpoint.id)?;
    validation::validate_id(&checkpoint.session_id)?;
    validation::validate_id(&checkpoint.through_entry_id)?;
    validation::validate_id(&checkpoint.first_retained_entry_id)?;
    if let Some(id) = &checkpoint.supersedes_checkpoint_id {
        validation::validate_id(id)?;
    }
    if checkpoint.facts.len() > MAX_CHECKPOINT_FACTS {
        return Err(SessionStoreError::Limit(format!(
            "checkpoint exceeds {MAX_CHECKPOINT_FACTS} facts"
        )));
    }
    if checkpoint.accepted_source_manifest_ids.len() > MAX_ACCEPTED_MANIFEST_IDS {
        return Err(SessionStoreError::Limit(format!(
            "checkpoint exceeds {MAX_ACCEPTED_MANIFEST_IDS} accepted source manifests"
        )));
    }
    let mut seen_manifest_ids = HashSet::new();
    for manifest_id in &checkpoint.accepted_source_manifest_ids {
        validation::validate_id(manifest_id)?;
        if !seen_manifest_ids.insert(manifest_id) {
            return Err(SessionStoreError::Invalid(
                "checkpoint contains a duplicate accepted source manifest".to_string(),
            ));
        }
    }
    for fact in &checkpoint.facts {
        if fact.provenance.source_turn_ids.is_empty() {
            return Err(SessionStoreError::Invalid(
                "checkpoint fact has no source turns".to_string(),
            ));
        }
        if fact.provenance.source_turn_ids.len() > MAX_SOURCE_TURNS_PER_FACT {
            return Err(SessionStoreError::Limit(format!(
                "checkpoint fact exceeds {MAX_SOURCE_TURNS_PER_FACT} source turns"
            )));
        }
        for source_id in &fact.provenance.source_turn_ids {
            validation::validate_id(source_id)?;
        }
        if let Some(memory) = &fact.provenance.memory_entry {
            validation::validate_id(&memory.entry_id)?;
        }
    }
    Ok(())
}

fn require_session(conn: &Connection, session_id: &str) -> Result<(), SessionStoreError> {
    let exists: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM chat_sessions WHERE id=?1)",
            params![session_id],
            |row| row.get(0),
        )
        .map_err(storage("check checkpoint session"))?;
    if exists {
        Ok(())
    } else {
        Err(SessionStoreError::NotFound(session_id.to_string()))
    }
}

fn require_ordered_boundaries(
    conn: &Connection,
    checkpoint: &CompactionCheckpoint,
) -> Result<(), SessionStoreError> {
    let through: Option<i64> = conn
        .query_row(
            "SELECT ordinal FROM chat_messages WHERE id=?1 AND session_id=?2",
            params![checkpoint.through_entry_id, checkpoint.session_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(storage("check checkpoint summarized boundary"))?;
    let retained: Option<(i64, String, Option<String>)> = conn
        .query_row(
            "SELECT ordinal,kind,role FROM chat_messages WHERE id=?1 AND session_id=?2",
            params![checkpoint.first_retained_entry_id, checkpoint.session_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()
        .map_err(storage("check checkpoint retained boundary"))?;
    match (through, retained) {
        (Some(through), Some((retained, kind, Some(role))))
            if retained == through.saturating_add(1) && kind == "message" && role == "user" =>
        {
            Ok(())
        }
        (Some(_), Some(_)) => Err(SessionStoreError::Invalid(
            "checkpoint must retain the next complete user turn after summarized history"
                .to_string(),
        )),
        _ => Err(SessionStoreError::Invalid(
            "checkpoint boundary does not belong to its session".to_string(),
        )),
    }
}

fn require_owned_manifests(
    conn: &Connection,
    checkpoint: &CompactionCheckpoint,
) -> Result<(), SessionStoreError> {
    let through_ordinal: i64 = conn
        .query_row(
            "SELECT ordinal FROM chat_messages WHERE id=?1 AND session_id=?2",
            params![checkpoint.through_entry_id, checkpoint.session_id],
            |row| row.get(0),
        )
        .map_err(storage("read checkpoint manifest boundary"))?;

    for entry_id in &checkpoint.accepted_source_manifest_ids {
        let owner: Option<(i64, Option<String>)> = conn
            .query_row(
                "SELECT ordinal,context_manifest_json FROM chat_messages
                 WHERE id=?1 AND session_id=?2",
                params![entry_id, checkpoint.session_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(storage("check checkpoint accepted manifest"))?;
        let Some((ordinal, Some(raw_manifest))) = owner else {
            return Err(SessionStoreError::Invalid(
                "checkpoint accepted manifest does not belong to its session".to_string(),
            ));
        };
        if ordinal > through_ordinal {
            return Err(SessionStoreError::Invalid(
                "checkpoint accepted manifest lies outside summarized history".to_string(),
            ));
        }
        let manifest: Vec<ContextSourceManifestItem> = serde_json::from_str(&raw_manifest)
            .map_err(|e| SessionStoreError::Corrupt(format!("malformed accepted manifest: {e}")))?;
        if manifest.is_empty() {
            return Err(SessionStoreError::Corrupt(
                "accepted manifest is unexpectedly empty".to_string(),
            ));
        }
        validate_context_manifest(&manifest)
            .map_err(|e| SessionStoreError::Corrupt(format!("invalid accepted manifest: {e}")))?;
    }
    Ok(())
}

fn require_owned_fact_sources(
    conn: &Connection,
    checkpoint: &CompactionCheckpoint,
) -> Result<(), SessionStoreError> {
    let through_ordinal: i64 = conn
        .query_row(
            "SELECT ordinal FROM chat_messages WHERE id=?1 AND session_id=?2",
            params![checkpoint.through_entry_id, checkpoint.session_id],
            |row| row.get(0),
        )
        .map_err(storage("read checkpoint boundary"))?;
    for fact in &checkpoint.facts {
        for source_id in &fact.provenance.source_turn_ids {
            let ordinal: Option<i64> = conn
                .query_row(
                    "SELECT ordinal FROM chat_messages WHERE id=?1 AND session_id=?2",
                    params![source_id, checkpoint.session_id],
                    |row| row.get(0),
                )
                .optional()
                .map_err(storage("check checkpoint fact source"))?;
            match ordinal {
                Some(ordinal) if ordinal <= through_ordinal => {}
                Some(_) => {
                    return Err(SessionStoreError::Invalid(
                        "checkpoint fact source lies outside summarized history".to_string(),
                    ))
                }
                None => {
                    return Err(SessionStoreError::Invalid(
                        "checkpoint fact source does not belong to its session".to_string(),
                    ))
                }
            }
        }
    }
    Ok(())
}

fn require_valid_supersedes(
    conn: &Connection,
    checkpoint: &CompactionCheckpoint,
) -> Result<(), SessionStoreError> {
    let Some(supersedes) = &checkpoint.supersedes_checkpoint_id else {
        return Ok(());
    };
    let owner: Option<String> = conn
        .query_row(
            "SELECT session_id FROM compaction_checkpoints WHERE id=?1",
            params![supersedes],
            |row| row.get(0),
        )
        .optional()
        .map_err(storage("check superseded checkpoint"))?;
    match owner {
        Some(owner) if owner == checkpoint.session_id => Ok(()),
        Some(_) => Err(SessionStoreError::Invalid(
            "superseded checkpoint belongs to another session".to_string(),
        )),
        None => Err(SessionStoreError::Invalid(
            "superseded checkpoint does not exist".to_string(),
        )),
    }
}

fn storage(context: &'static str) -> impl Fn(rusqlite::Error) -> SessionStoreError {
    move |error| SessionStoreError::Storage(format!("{context}: {error}"))
}

/// Where one summarized fact came from.
///
/// A fact with no resolvable provenance is not eligible for a projection at
/// all, so this is not optional metadata — it is what makes the fact usable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FactProvenance {
    /// Transcript entry ids this fact was derived from. Never empty: a fact
    /// with no turn behind it cannot be re-checked once history moves on.
    pub source_turn_ids: Vec<String>,
    /// Set when the fact restates a durable memory entry, with the revision it
    /// restated. A later revision means the user changed their mind, so the
    /// summarized wording is stale even though the entry still exists.
    pub memory_entry: Option<MemoryProvenance>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryProvenance {
    pub entry_id: String,
    pub revision: u32,
}

/// One structured claim inside a checkpoint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckpointFact {
    pub kind: FactKind,
    pub text: String,
    pub provenance: FactProvenance,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FactKind {
    Goal,
    Constraint,
    Progress,
    Decision,
    UnresolvedWork,
    CriticalFact,
}

/// Why a fact was refused for the current projection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FactRefusal {
    /// The memory entry it restated has been forgotten.
    MemoryForgotten,
    /// The entry still exists but the user has since revised it.
    MemoryRevised,
    /// None of its source turns are in retained history any more.
    SourceTurnsGone,
    /// It named no source at all, so nothing can vouch for it.
    Unprovenanced,
}

/// A memory the user asked Plume to forget, and the turns it was drawn from.
///
/// Written when the memory is forgotten, and kept afterwards — this record is
/// the only durable trace that the user said "stop knowing this", because the
/// memory entry itself is gone. Without it a rebuild has nothing to consult.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ForgottenMemory {
    pub entry_id: String,
    /// The turns the forgotten memory was derived from. These stay in history
    /// and stay visible to the user; they are only withheld from summarization.
    pub source_turn_ids: Vec<String>,
    pub forgotten_at_ms: i64,
}

/// Turns a rebuild must not summarize.
///
/// Refusing a stale fact marks its checkpoint for rebuild — and a rebuild reads
/// retained history, where the turn that produced the fact is still present
/// because Plume never deletes history. So the rebuild derives the same fact
/// again, this time with no memory link to refuse it by, and forget lasts
/// exactly one projection.
///
/// The turn is excluded from *summarization only*. It stays in the transcript,
/// stays on screen, and stays exportable: the user asked Plume to stop knowing
/// something, not to erase what they said. Excluding the whole turn is blunt —
/// it may carry unrelated content — but the alternative is asking a model to
/// re-derive "everything except that", which is a judgement call, and whether a
/// forgotten fact stays forgotten cannot be one.
pub fn forgotten_turn_ids(forgotten: &[ForgottenMemory]) -> HashSet<String> {
    forgotten
        .iter()
        .flat_map(|entry| entry.source_turn_ids.iter().cloned())
        .collect()
}

/// The turns a rebuild may summarize: retained history minus what was forgotten.
pub fn rebuildable_turn_ids(
    retained: &HashSet<String>,
    forgotten: &[ForgottenMemory],
) -> HashSet<String> {
    let excluded = forgotten_turn_ids(forgotten);
    retained.difference(&excluded).cloned().collect()
}

/// The live state a checkpoint's facts are re-checked against.
pub struct ProvenanceContext<'a> {
    /// Current revision of every durable memory entry that still exists.
    /// Absent means forgotten.
    pub memory_revisions: &'a HashMap<String, u32>,
    /// Transcript entry ids still present in retained history.
    pub retained_turn_ids: &'a HashSet<String>,
}

/// What a checkpoint contributes to the current projection.
#[derive(Debug, Clone, PartialEq)]
pub struct FactResolution {
    pub kept: Vec<CheckpointFact>,
    pub refused: Vec<(CheckpointFact, FactRefusal)>,
}

impl FactResolution {
    /// A checkpoint that lost facts is stale: it no longer describes current
    /// state, so it must be rebuilt from retained history rather than
    /// re-summarized. Re-summarizing it would carry the loss forward silently.
    pub fn is_stale(&self) -> bool {
        !self.refused.is_empty()
    }
}

/// Re-check every fact against live state.
///
/// This never edits the stored checkpoint. Checkpoints are immutable, so a
/// dropped fact is a decision about *this* projection, recorded here and
/// reflected by rebuilding — not a silent rewrite of history.
pub fn resolve_facts(facts: &[CheckpointFact], context: &ProvenanceContext<'_>) -> FactResolution {
    let mut kept = Vec::new();
    let mut refused = Vec::new();

    for fact in facts {
        match refusal_for(fact, context) {
            Some(reason) => refused.push((fact.clone(), reason)),
            None => kept.push(fact.clone()),
        }
    }

    FactResolution { kept, refused }
}

fn refusal_for(fact: &CheckpointFact, context: &ProvenanceContext<'_>) -> Option<FactRefusal> {
    // Source turns are required, not merely one acceptable kind of provenance.
    // A fact exists because it was summarized out of the transcript, so without
    // them there is nothing to re-check it against once history moves on — a
    // memory-only fact would keep projecting long after every turn behind it
    // was compacted away, which is the anchorless state this module exists to
    // prevent.
    if fact.provenance.source_turn_ids.is_empty() {
        return Some(FactRefusal::Unprovenanced);
    }

    // Memory provenance is checked first and independently of the turns. A fact
    // restating a forgotten memory must not survive merely because the turn
    // that discussed it is still in history — that turn is why the fact exists,
    // not evidence the user still wants it remembered.
    if let Some(memory) = &fact.provenance.memory_entry {
        match context.memory_revisions.get(&memory.entry_id) {
            None => return Some(FactRefusal::MemoryForgotten),
            Some(current) if *current != memory.revision => {
                return Some(FactRefusal::MemoryRevised)
            }
            Some(_) => {}
        }
    }

    if !fact
        .provenance
        .source_turn_ids
        .iter()
        .any(|id| context.retained_turn_ids.contains(id))
    {
        return Some(FactRefusal::SourceTurnsGone);
    }

    None
}
