//! Opaque identity for an explicit context reference.
//!
//! Split from `explicit_context` for size, but the pairing is the point: the
//! key below is the only definition of when two references are "the same
//! source", and every list that has to be counted against
//! `MAX_EXPLICIT_CONTEXT_SOURCES` is made distinct through it.

use std::collections::HashSet;

use super::explicit_context::ContextSourceRef;

/// Collapse repeated source references, keeping the first of each.
///
/// Identity is the reference itself — a path plus its line range, or an opaque
/// id — never the resolved bytes, so the same file attached twice is one
/// source. Callers that build a list from more than one place need this before
/// they count against `MAX_EXPLICIT_CONTEXT_SOURCES`: the cap is on distinct
/// sources, and a source that appears twice costs the prompt nothing extra
/// because resolution collapses it anyway.
///
/// First-seen order is part of the contract. The accepted manifest a turn is
/// persisted with mirrors this order, and the user reads it as the order they
/// attached things in.
pub fn dedup_source_refs(
    refs: impl IntoIterator<Item = ContextSourceRef>,
) -> Vec<ContextSourceRef> {
    let mut seen = HashSet::new();
    let mut distinct = Vec::new();
    for source in refs {
        if seen.insert(source_key(&source)) {
            distinct.push(source);
        }
    }
    distinct
}

pub(super) fn source_key(source: &ContextSourceRef) -> String {
    match source {
        ContextSourceRef::ProjectFile {
            rel_path,
            start_line,
            end_line,
        } => format!("file:{rel_path}:{start_line:?}:{end_line:?}"),
        ContextSourceRef::MemoryEntry { entry_id } => format!("memory:{entry_id}"),
        ContextSourceRef::UserMemoryEntry { entry_id } => format!("user-memory:{entry_id}"),
        ContextSourceRef::TopicFile { name } => format!("topic:{name}"),
        ContextSourceRef::BrowserTextEvidence { evidence_id } => {
            format!("browser-text:{evidence_id}")
        }
        ContextSourceRef::BrowserScreenshotEvidence { evidence_id } => {
            format!("browser-screenshot:{evidence_id}")
        }
    }
}
