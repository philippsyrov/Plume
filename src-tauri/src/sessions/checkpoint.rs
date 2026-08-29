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

// Phase 2B wires these into the projection builder and the store. Until then
// the rule is exercised by its tests only, which is deliberate: the resolution
// policy is worth settling and reviewing before anything depends on it.
#![allow(dead_code)]

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

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
