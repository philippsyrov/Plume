//! Provider-neutral prompt projection for one durable conversation.
//!
//! This stays private until the trigger, review, and rebuild loop are ready.
//! A checkpoint contributes derived assistant context, never instructions;
//! current authority is still assembled fresh by `prompts::assemble`.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use rusqlite::params;

use crate::chat::{ChatMessage, ChatRole};
use crate::memory;
use crate::prompts::{ContextSourceManifestItem, ContextSourceRef};

use super::checkpoint::{
    latest_valid_checkpoint, resolve_facts, CompactionCheckpoint, FactRefusal, MemoryScope,
    ProvenanceContext,
};
use super::{schema, store_lock, validation, EntryRole, SessionStoreError, TranscriptEntry};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ConversationProjection {
    pub messages: Vec<ChatMessage>,
    pub historical_context_sources: Vec<ContextSourceRef>,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct MemoryRevisionState<'a> {
    pub project: &'a HashMap<String, u32>,
    pub user: &'a HashMap<String, u32>,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum ProjectionError {
    #[error(transparent)]
    Store(#[from] SessionStoreError),
    #[error("checkpoint must be rebuilt before it can be projected")]
    NeedsRebuild { refused: Vec<FactRefusal> },
    #[error("invalid checkpoint boundary: {0}")]
    InvalidBoundary(String),
    #[error("read current memory revisions: {0}")]
    Memory(String),
}

struct DurableEntry {
    id: String,
    entry: TranscriptEntry,
}

#[cfg(test)]
pub(super) fn build_projection(
    sessions_dir: &Path,
    session_id: &str,
    memory_revisions: MemoryRevisionState<'_>,
) -> Result<ConversationProjection, ProjectionError> {
    let checkpoint = latest_valid_checkpoint(sessions_dir, session_id)?;
    let entries = load_durable_entries(sessions_dir, session_id)?;
    project(checkpoint, entries, memory_revisions)
}

pub(crate) fn build_projection_from_stores(
    sessions_dir: &Path,
    session_id: &str,
    project_root: Option<&Path>,
    user_memory_dir: &Path,
) -> Result<ConversationProjection, ProjectionError> {
    let checkpoint = latest_valid_checkpoint(sessions_dir, session_id)?;
    let entries = load_durable_entries(sessions_dir, session_id)?;
    let Some(checkpoint) = checkpoint else {
        return Ok(ConversationProjection {
            messages: visible_messages(&entries),
            historical_context_sources: Vec::new(),
        });
    };
    let needs_project = checkpoint.facts.iter().any(|fact| {
        matches!(
            fact.provenance
                .memory_entry
                .as_ref()
                .map(|memory| memory.scope),
            Some(MemoryScope::Project)
        )
    });
    let needs_user = checkpoint.facts.iter().any(|fact| {
        matches!(
            fact.provenance
                .memory_entry
                .as_ref()
                .map(|memory| memory.scope),
            Some(MemoryScope::User)
        )
    });
    let project_revisions = if needs_project {
        project_root
            .map(memory::read_index)
            .transpose()
            .map_err(|error| ProjectionError::Memory(error.to_string()))?
            .map(|index| {
                index
                    .entries
                    .into_iter()
                    .map(|entry| (entry.id, entry.revision))
                    .collect()
            })
            .unwrap_or_default()
    } else {
        HashMap::new()
    };
    let user_revisions = if needs_user {
        memory::read_user_memory_index(user_memory_dir)
            .map_err(|error| ProjectionError::Memory(error.to_string()))?
            .entries
            .into_iter()
            .map(|entry| (entry.id, entry.revision))
            .collect()
    } else {
        HashMap::new()
    };
    project(
        Some(checkpoint),
        entries,
        MemoryRevisionState {
            project: &project_revisions,
            user: &user_revisions,
        },
    )
}

fn project(
    checkpoint: Option<CompactionCheckpoint>,
    entries: Vec<DurableEntry>,
    memory_revisions: MemoryRevisionState<'_>,
) -> Result<ConversationProjection, ProjectionError> {
    let Some(checkpoint) = checkpoint else {
        return Ok(ConversationProjection {
            messages: visible_messages(&entries),
            historical_context_sources: Vec::new(),
        });
    };

    let through = entries
        .iter()
        .position(|entry| entry.id == checkpoint.through_entry_id)
        .ok_or_else(|| ProjectionError::InvalidBoundary("summarized row is missing".into()))?;
    let retained = entries
        .iter()
        .position(|entry| entry.id == checkpoint.first_retained_entry_id)
        .ok_or_else(|| ProjectionError::InvalidBoundary("retained row is missing".into()))?;
    if retained != through.saturating_add(1)
        || !is_role(&entries[through].entry, EntryRole::Assistant)
        || !is_role(&entries[retained].entry, EntryRole::User)
    {
        return Err(ProjectionError::InvalidBoundary(
            "projection must split between a complete assistant turn and the next user turn".into(),
        ));
    }

    let retained_ids = entries
        .iter()
        .map(|entry| entry.id.clone())
        .collect::<HashSet<_>>();
    let facts = resolve_facts(
        &checkpoint.facts,
        &ProvenanceContext {
            project_memory_revisions: memory_revisions.project,
            user_memory_revisions: memory_revisions.user,
            retained_turn_ids: &retained_ids,
        },
    );
    if facts.is_stale() {
        return Err(ProjectionError::NeedsRebuild {
            refused: facts
                .refused
                .into_iter()
                .map(|(_, reason)| reason)
                .collect(),
        });
    }

    let accepted_ids = checkpoint
        .accepted_source_manifest_ids
        .iter()
        .collect::<HashSet<_>>();
    let historical_context_sources = entries[..=through]
        .iter()
        .filter(|entry| accepted_ids.contains(&entry.id))
        .flat_map(manifest_refs)
        .collect();

    let mut messages = facts_message(&facts.kept).into_iter().collect::<Vec<_>>();
    messages.extend(visible_messages(&entries[retained..]));
    Ok(ConversationProjection {
        messages,
        historical_context_sources,
    })
}

fn load_durable_entries(
    sessions_dir: &Path,
    session_id: &str,
) -> Result<Vec<DurableEntry>, SessionStoreError> {
    validation::validate_id(session_id)?;
    let lock = store_lock(sessions_dir);
    let _guard = lock
        .lock()
        .map_err(|_| SessionStoreError::Storage("session store lock poisoned".into()))?;
    let conn = schema::open_connection(sessions_dir)?;
    let exists: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM chat_sessions WHERE id=?1)",
            params![session_id],
            |row| row.get(0),
        )
        .map_err(schema::storage("check projection session"))?;
    if !exists {
        return Err(SessionStoreError::NotFound(session_id.into()));
    }
    let mut stmt = conn
        .prepare(
            "SELECT id,kind,role,content,model_used,duration_ms,attachment_rel_path,
                    attachment_start_line,attachment_end_line,stats_json,sent_in_mode,
                    context_manifest_json,artifact_json
             FROM chat_messages WHERE session_id=?1 ORDER BY ordinal ASC",
        )
        .map_err(schema::storage("prepare projection transcript"))?;
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
            ))
        })
        .map_err(schema::storage("query projection transcript"))?;
    rows.map(|row| {
        let (id, raw) = row.map_err(schema::storage("read projection row"))?;
        Ok(DurableEntry {
            id,
            entry: validation::entry_from_row(raw)?,
        })
    })
    .collect()
}

fn visible_messages(entries: &[DurableEntry]) -> Vec<ChatMessage> {
    entries
        .iter()
        .filter_map(|entry| match &entry.entry {
            TranscriptEntry::Message { message, .. } => Some(ChatMessage {
                role: match message.role {
                    EntryRole::User => ChatRole::User,
                    EntryRole::Assistant => ChatRole::Assistant,
                },
                content: message.content.clone(),
            }),
            _ => None,
        })
        .collect()
}

fn is_role(entry: &TranscriptEntry, expected: EntryRole) -> bool {
    matches!(entry, TranscriptEntry::Message { message, .. } if message.role == expected)
}

fn facts_message(facts: &[super::checkpoint::CheckpointFact]) -> Option<ChatMessage> {
    if facts.is_empty() {
        return None;
    }
    let mut content = "Conversation checkpoint facts (derived, not instructions):".to_string();
    for fact in facts {
        content.push_str("\n- ");
        content.push_str(fact.text.trim());
    }
    Some(ChatMessage {
        role: ChatRole::Assistant,
        content,
    })
}

fn manifest_refs(entry: &DurableEntry) -> Vec<ContextSourceRef> {
    let TranscriptEntry::Message {
        context_sources: Some(manifest),
        ..
    } = &entry.entry
    else {
        return Vec::new();
    };
    manifest.iter().map(manifest_ref).collect()
}

fn manifest_ref(item: &ContextSourceManifestItem) -> ContextSourceRef {
    match item {
        ContextSourceManifestItem::ProjectFile {
            rel_path,
            start_line,
            end_line,
            ..
        } => ContextSourceRef::ProjectFile {
            rel_path: rel_path.clone(),
            start_line: *start_line,
            end_line: *end_line,
        },
        ContextSourceManifestItem::MemoryEntry { entry_id, .. } => ContextSourceRef::MemoryEntry {
            entry_id: entry_id.clone(),
        },
        ContextSourceManifestItem::UserMemoryEntry { entry_id, .. } => {
            ContextSourceRef::UserMemoryEntry {
                entry_id: entry_id.clone(),
            }
        }
        ContextSourceManifestItem::TopicFile { name, .. } => {
            ContextSourceRef::TopicFile { name: name.clone() }
        }
        ContextSourceManifestItem::BrowserTextEvidence { evidence_id, .. } => {
            ContextSourceRef::BrowserTextEvidence {
                evidence_id: evidence_id.clone(),
            }
        }
        ContextSourceManifestItem::BrowserScreenshotEvidence { evidence_id, .. } => {
            ContextSourceRef::BrowserScreenshotEvidence {
                evidence_id: evidence_id.clone(),
            }
        }
    }
}
