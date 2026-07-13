//! D37: local project memory MVP.
//!
//! Stores small text "memories" the user explicitly remembers about
//! a project. The product brief in
//! `docs/LOCAL_AGENT_NORTH_STAR.md § Plume Memory Design Direction`
//! sketches a richer layout (`INDEX.md`, `USER.md`, `SOUL.md`,
//! topic files, SQLite FTS for session search). D37 ships the
//! smallest visible floor of that: a flat JSONL store under
//! `.plume/memory/entries.jsonl` plus three IPC verbs
//! (`memory.index`, `memory.remember`, `memory.forget`).
//!
//! No embeddings. No cloud calls. No session log replay. No
//! distillation. The MVP exists to make a memory chip real on disk
//! so later slices can extend it without churning the wire shape.
//!
//! Safety properties:
//!
//! * **Trust gate.** Every verb is wrapped by the same trusted-
//!   project check the patch verbs use. No trusted project → no
//!   memory reads or writes.
//! * **Path safety.** The store lives at `<project>/.plume/memory/`
//!   only. `.plume/` symlinks are rejected before any write —
//!   mirroring the checkpoint guard.
//! * **Secret redaction.** Every remembered text passes through
//!   `prompts::redact::redact` before being stored. The original
//!   bytes never reach disk. A memory that redacts to empty is
//!   rejected outright.
//! * **Bounded.** Hard caps on per-entry bytes (1 KiB), total file
//!   size (64 KiB), and entry count (100). Remember rejects when
//!   any cap would be exceeded; this is on-disk, the user can
//!   forget to make room.
//! * **Reversible.** `memory.forget(entryId)` removes an entry by
//!   opaque id. Hard delete — no tombstone, no undo. The file is
//!   plain JSONL on disk so the user can also edit it directly.
//!
//! Visible by design: the file is human-readable JSONL, lives
//! inside the project, and the panel shows the full content.
//!
//! D108: split by behavior boundary into three files. `types.rs` holds
//! every wire/response type (no logic); `store.rs` holds the on-disk
//! storage helpers (symlink-safe paths, JSONL read/write, id minting —
//! also no logic beyond IO); this file keeps the module doc, the
//! process-wide mutex, the caps, and the five CRUD verbs
//! (`read_index`/`read_for_prompt`/`remember`/`update`/`forget`/`search`)
//! that tie types + storage together. Every external `crate::memory::X`
//! path is unchanged — see the re-exports below.

use std::fs;
use std::path::Path;
use std::sync::Mutex;

use crate::prompts::redact::redact;

mod distill;
mod links;
mod store;
mod topics;
mod types;

// Re-export the surface production code (commands::memory) consumes.
// Test-only types (DuplicateGroup, MemoryDistillApplyOk /
// MemoryDistillApplyFailure, normalize_for_distill, TopicFile /
// TopicKind / TopicLimits) are imported directly from the submodule
// inside the test module so the non-test bin build doesn't see them as
// unused re-exports.
pub use distill::{
    distill_apply, distill_preview, read_distill_log, DistillLogEntry, DistillPreview,
    MemoryDistillApplyResponse,
};
pub use links::{set_links, MemorySetLinksResponse};
pub use topics::{
    read_core_for_prompt, read_topic_for_prompt, read_topics, MemoryTopics, TopicsPromptRead,
};
pub use types::{
    MemoryEntry, MemoryForgetErr, MemoryForgetFailure, MemoryForgetOk, MemoryForgetResponse,
    MemoryIndex, MemoryLimits, MemoryPromptRead, MemoryRememberErr, MemoryRememberFailure,
    MemoryRememberOk, MemoryRememberResponse, MemorySearchErr, MemorySearchFailure,
    MemorySearchHit, MemorySearchOk, MemorySearchResponse, MemoryStoreError, MemoryUpdateErr,
    MemoryUpdateFailure, MemoryUpdateOk, MemoryUpdateResponse,
};

// Storage-layer helpers stay at their original (private / module-and-
// descendants) visibility — this `use` re-export has the same default
// privacy a bare `fn` defined directly in this file would have, so
// `distill.rs` / `topics.rs`'s existing `use super::{resolve_entries_path,
// refuse_symlink, ...}` keeps resolving unchanged.
use store::{
    ensured_entries_path, is_valid_entry_id, mint_entry_id, now_ms, read_entries, refuse_symlink,
    resolve_entries_path, resolve_memory_file, serialize_entries, write_atomic,
};

/// Process-wide mutex serialising every memory write AND every
/// memory read. Codex's D37 MEDIUM finding: atomic rename only
/// prevents torn files, not lost updates — two concurrent
/// `memory.remember` calls would both `read_entries` against the
/// same baseline and clobber each other's append. The mutex also
/// covers `read_index` so the panel never observes a write
/// half-way through rename. Same `OnceLock` pattern as
/// `patch::apply::apply_mutex`. Memory-local rather than reusing
/// the patch mutex because there's no overlap between the two
/// stores (`.plume/memory/` vs `.plume/checkpoints/`), and
/// blocking a memory remember on an in-flight patch apply would
/// surprise the user.
pub(crate) fn memory_mutex() -> &'static Mutex<()> {
    static MUTEX: std::sync::OnceLock<Mutex<()>> = std::sync::OnceLock::new();
    MUTEX.get_or_init(|| Mutex::new(()))
}

/// Hard cap on total memory entries. Past this, `remember` rejects
/// with `capacityReached` until the user forgets one.
pub const MAX_ENTRIES: usize = 100;

/// Hard cap on the redacted text size of one remembered entry.
/// 1 KiB is plenty for a prose memory; longer-form memories belong
/// in topic files (`docs/LOCAL_AGENT_NORTH_STAR.md`), not the index.
pub const MAX_BYTES_PER_ENTRY: usize = 1024;

/// Hard cap on the on-disk file size of `entries.jsonl`. 64 KiB
/// is well above `MAX_ENTRIES * MAX_BYTES_PER_ENTRY + JSON
/// overhead`; it exists as a defense against external edits that
/// blow the file up.
pub const MAX_BYTES_TOTAL: u64 = 64 * 1024;

// ─── Public API ─────────────────────────────────────────────────────────────

/// Read the current memory index. Missing store file → empty
/// entries, `total_bytes = 0`. Malformed lines are silently
/// dropped so a hand-edited file with a typo doesn't take down
/// the panel.
///
/// Goes through the same symlink-safe path resolver as `remember`
/// and `forget` — a pre-planted `.plume/` symlink causes the read
/// to surface a `StoreFailed` shape rather than silently
/// dereferencing the symlink (Codex D37 HIGH).
pub fn read_index(project_root: &Path) -> Result<MemoryIndex, MemoryStoreError> {
    let _guard = memory_mutex().lock().unwrap_or_else(|e| e.into_inner());
    let entries_path = resolve_entries_path(project_root)?;
    let (entries, total_bytes) = read_entries(&entries_path)?;
    Ok(MemoryIndex {
        entries,
        limits: MemoryLimits::default(),
        total_bytes,
    })
}

/// Resolve one exact opaque memory id for explicit user-selected prompt
/// context. The owning store performs the read under its normal mutex; links
/// remain metadata and are returned only because they are part of the stored
/// entry shape, never because they influence selection.
pub fn read_entry_for_prompt(
    project_root: &Path,
    entry_id: &str,
) -> Result<Option<MemoryEntry>, MemoryStoreError> {
    let _guard = memory_mutex().lock().unwrap_or_else(|e| e.into_inner());
    let entries_path = resolve_entries_path(project_root)?;
    let (entries, _) = read_entries(&entries_path)?;
    Ok(entries.into_iter().find(|entry| entry.id == entry_id))
}

/// D42 chat-context read: return the entries that fit in `byte_cap`,
/// newest first. The byte budget is summed across each entry's
/// `text.len()` only — JSON overhead and the per-entry bullet/
/// newline that the prompt assembler will add do NOT count, because
/// the user-visible "memory contribution" the UI surfaces is the
/// content bytes, not the wire bytes.
///
/// Why newest-first: a recently remembered fact is more likely to
/// reflect the user's current intent than one from last week. When
/// the cap forces a drop, the older entries are the right ones to
/// drop. The store itself stays append-ordered on disk; this
/// function reverses for the prompt projection only.
///
/// Same symlink-safe path resolver and process-wide mutex as
/// `read_index` — concurrent remembers and chat-context reads do
/// not race.
///
/// A missing store, an empty store, or a `byte_cap` of zero all
/// return `MemoryPromptRead { entries: [], used_bytes: 0, byte_cap,
/// truncated: false }`. The caller (assemble) treats the empty
/// case as "no system-message to inject".
pub fn read_for_prompt(
    project_root: &Path,
    byte_cap: usize,
) -> Result<MemoryPromptRead, MemoryStoreError> {
    let _guard = memory_mutex().lock().unwrap_or_else(|e| e.into_inner());
    let entries_path = resolve_entries_path(project_root)?;
    let (mut entries, _total_bytes) = read_entries(&entries_path)?;

    // Newest first. `created_ms` is u64 epoch-ms so a stable sort
    // descending lands the newest entries at the front.
    entries.sort_by_key(|e| std::cmp::Reverse(e.created_ms));

    let mut picked: Vec<MemoryEntry> = Vec::new();
    let mut used_bytes: usize = 0;
    let mut truncated = false;
    for entry in entries.into_iter() {
        let entry_bytes = entry.text.len();
        if used_bytes.saturating_add(entry_bytes) > byte_cap {
            // Skip this entry but keep scanning: a long entry may
            // be followed by a short one that still fits. This
            // matches what a "best-effort, drop oldest first" pass
            // would do once we sorted newest-first.
            truncated = true;
            continue;
        }
        used_bytes += entry_bytes;
        picked.push(entry);
    }

    Ok(MemoryPromptRead {
        entries: picked,
        used_bytes,
        byte_cap,
        truncated,
    })
}

/// Remember `raw_text`. The text is trimmed, length-capped, and
/// passed through the secret redactor before being written. The
/// new entry is appended to the JSONL store; reaching any cap
/// returns the corresponding `MemoryRememberFailure`.
///
/// Takes the memory mutex for the whole read-modify-write cycle
/// (Codex D37 MEDIUM): atomic rename prevents torn files, but two
/// concurrent appends would each read the same baseline and clobber
/// each other's entry. The lock is process-wide and held until the
/// rename returns.
pub fn remember(project_root: &Path, raw_text: &str) -> MemoryRememberResponse {
    let _guard = memory_mutex().lock().unwrap_or_else(|e| e.into_inner());
    let trimmed = raw_text.trim();
    if trimmed.is_empty() {
        return err_remember(
            MemoryRememberFailure::Empty,
            "memory text is empty or whitespace-only".to_string(),
        );
    }
    if trimmed.len() > MAX_BYTES_PER_ENTRY {
        return err_remember(
            MemoryRememberFailure::TooLong,
            format!(
                "memory text is {} bytes; max is {}",
                trimmed.len(),
                MAX_BYTES_PER_ENTRY
            ),
        );
    }

    let (redacted, spans) = redact(trimmed);
    if redacted.trim().is_empty() {
        return err_remember(
            MemoryRememberFailure::RedactedToEmpty,
            "memory text was entirely secret-pattern matches; nothing left after redaction"
                .to_string(),
        );
    }

    // Re-cap after redaction: replacement markers can grow the
    // string (e.g. a 3-char `sk-...` doesn't, but a near-cap
    // input can flip past).
    if redacted.len() > MAX_BYTES_PER_ENTRY {
        return err_remember(
            MemoryRememberFailure::TooLong,
            format!(
                "memory text was {} bytes after redaction; max is {}",
                redacted.len(),
                MAX_BYTES_PER_ENTRY
            ),
        );
    }

    let entries_path = match ensured_entries_path(project_root) {
        Ok(p) => p,
        Err(e) => {
            return err_remember(
                MemoryRememberFailure::StoreFailed,
                format!("prepare memory dir: {}", e),
            );
        }
    };

    let (mut entries, _bytes) = match read_entries(&entries_path) {
        Ok(p) => p,
        Err(e) => {
            return err_remember(MemoryRememberFailure::StoreFailed, e.0);
        }
    };
    if entries.len() >= MAX_ENTRIES {
        return err_remember(
            MemoryRememberFailure::CapacityReached,
            format!(
                "memory holds {} entries; max is {}",
                entries.len(),
                MAX_ENTRIES
            ),
        );
    }

    let entry = MemoryEntry {
        id: mint_entry_id(),
        created_ms: now_ms(),
        text: redacted,
        redaction_count: spans.len() as u32,
        links: Vec::new(),
    };
    entries.push(entry.clone());

    // Project the new total-byte size; reject before writing if
    // the cap would be crossed.
    let serialized = match serialize_entries(&entries) {
        Ok(s) => s,
        Err(e) => {
            return err_remember(MemoryRememberFailure::StoreFailed, e.0);
        }
    };
    if serialized.len() as u64 > MAX_BYTES_TOTAL {
        return err_remember(
            MemoryRememberFailure::CapacityReached,
            format!(
                "memory store would be {} bytes; max is {}",
                serialized.len(),
                MAX_BYTES_TOTAL
            ),
        );
    }
    if let Err(e) = write_atomic(&entries_path, serialized.as_bytes()) {
        return err_remember(MemoryRememberFailure::StoreFailed, e.0);
    }

    MemoryRememberResponse::Ok(MemoryRememberOk { ok: true, entry })
}

/// D80: replace the text of the entry with id `entry_id`. Preserves the
/// entry's `id` and `created_ms`; re-runs the secret redactor and the
/// per-entry / total caps on the new text exactly like `remember`. A
/// well-formed id that matches no entry is `NotFound` (distinct from a
/// malformed id, which is `BadId`).
///
/// Takes the memory mutex around the read-modify-write (same as
/// `remember` / `forget`) and uses the symlink-safe resolver. Does not
/// create the store: editing an entry that can't exist (no store) is
/// `NotFound`.
pub fn update(project_root: &Path, entry_id: &str, raw_text: &str) -> MemoryUpdateResponse {
    if !is_valid_entry_id(entry_id) {
        return err_update(
            MemoryUpdateFailure::BadId,
            format!("invalid memory entry id: {:?}", entry_id),
        );
    }

    let _guard = memory_mutex().lock().unwrap_or_else(|e| e.into_inner());

    let trimmed = raw_text.trim();
    if trimmed.is_empty() {
        return err_update(
            MemoryUpdateFailure::Empty,
            "memory text is empty or whitespace-only".to_string(),
        );
    }
    if trimmed.len() > MAX_BYTES_PER_ENTRY {
        return err_update(
            MemoryUpdateFailure::TooLong,
            format!(
                "memory text is {} bytes; max is {}",
                trimmed.len(),
                MAX_BYTES_PER_ENTRY
            ),
        );
    }

    let (redacted, spans) = redact(trimmed);
    if redacted.trim().is_empty() {
        return err_update(
            MemoryUpdateFailure::RedactedToEmpty,
            "memory text was entirely secret-pattern matches; nothing left after redaction"
                .to_string(),
        );
    }
    if redacted.len() > MAX_BYTES_PER_ENTRY {
        return err_update(
            MemoryUpdateFailure::TooLong,
            format!(
                "memory text was {} bytes after redaction; max is {}",
                redacted.len(),
                MAX_BYTES_PER_ENTRY
            ),
        );
    }

    let entries_path = match resolve_entries_path(project_root) {
        Ok(p) => p,
        Err(e) => return err_update(MemoryUpdateFailure::StoreFailed, e.0),
    };
    let (mut entries, _bytes) = match read_entries(&entries_path) {
        Ok(p) => p,
        Err(e) => return err_update(MemoryUpdateFailure::StoreFailed, e.0),
    };

    let Some(pos) = entries.iter().position(|e| e.id == entry_id) else {
        return err_update(
            MemoryUpdateFailure::NotFound,
            format!("no memory entry with id {:?}", entry_id),
        );
    };

    // Replace text + redaction count; keep id and created_ms.
    entries[pos].text = redacted;
    entries[pos].redaction_count = spans.len() as u32;
    let updated = entries[pos].clone();

    let serialized = match serialize_entries(&entries) {
        Ok(s) => s,
        Err(e) => return err_update(MemoryUpdateFailure::StoreFailed, e.0),
    };
    if serialized.len() as u64 > MAX_BYTES_TOTAL {
        return err_update(
            MemoryUpdateFailure::CapacityReached,
            format!(
                "memory store would be {} bytes; max is {}",
                serialized.len(),
                MAX_BYTES_TOTAL
            ),
        );
    }
    if let Err(e) = write_atomic(&entries_path, serialized.as_bytes()) {
        return err_update(MemoryUpdateFailure::StoreFailed, e.0);
    }

    MemoryUpdateResponse::Ok(MemoryUpdateOk {
        ok: true,
        entry: updated,
    })
}

/// Forget the entry with id `entry_id`. Idempotent: returns
/// `ok: true, removed: false` if no entry matched.
///
/// Takes the memory mutex around the full read-modify-write cycle
/// (Codex D37 MEDIUM) and goes through the symlink-safe path
/// resolver (Codex D37 HIGH) so a pre-planted `.plume/` symlink
/// can't redirect the `remove_file` / atomic-rename to a path
/// outside the project root.
pub fn forget(project_root: &Path, entry_id: &str) -> MemoryForgetResponse {
    if !is_valid_entry_id(entry_id) {
        return MemoryForgetResponse::Err(MemoryForgetErr {
            ok: false,
            reason: MemoryForgetFailure::BadId,
            message: format!("invalid memory entry id: {:?}", entry_id),
        });
    }

    let _guard = memory_mutex().lock().unwrap_or_else(|e| e.into_inner());

    let entries_path = match resolve_entries_path(project_root) {
        Ok(p) => p,
        Err(e) => {
            return MemoryForgetResponse::Err(MemoryForgetErr {
                ok: false,
                reason: MemoryForgetFailure::StoreFailed,
                message: e.0,
            });
        }
    };
    let (mut entries, _bytes) = match read_entries(&entries_path) {
        Ok(p) => p,
        Err(e) => {
            return MemoryForgetResponse::Err(MemoryForgetErr {
                ok: false,
                reason: MemoryForgetFailure::StoreFailed,
                message: e.0,
            });
        }
    };

    let before = entries.len();
    entries.retain(|e| e.id != entry_id);
    let removed = entries.len() != before;

    if removed {
        if entries.is_empty() {
            // No entries left — remove the file entirely so the
            // store presents as "fresh" on next read. Failure here
            // is non-fatal: the empty-file form is a valid state.
            let _ = fs::remove_file(&entries_path);
        } else {
            let serialized = match serialize_entries(&entries) {
                Ok(s) => s,
                Err(e) => {
                    return MemoryForgetResponse::Err(MemoryForgetErr {
                        ok: false,
                        reason: MemoryForgetFailure::StoreFailed,
                        message: e.0,
                    });
                }
            };
            if let Err(e) = write_atomic(&entries_path, serialized.as_bytes()) {
                return MemoryForgetResponse::Err(MemoryForgetErr {
                    ok: false,
                    reason: MemoryForgetFailure::StoreFailed,
                    message: e.0,
                });
            }
        }
    }

    MemoryForgetResponse::Ok(MemoryForgetOk { ok: true, removed })
}

/// D43 search budget — hard caps that bound the worst case:
///
/// - `SEARCH_MAX_QUERY_BYTES`: 256 bytes for the input. Memory
///   entries top out at 1 KiB on the write path; a query bigger
///   than the entries themselves is shape garbage.
/// - `SEARCH_MAX_LIMIT`: 50 results per call. The panel renders an
///   inline list; pagination is not in scope.
pub const SEARCH_MAX_QUERY_BYTES: usize = 256;
pub const SEARCH_MAX_LIMIT: u32 = 50;

/// D43: read-only substring search across the project's memory
/// store. Case-insensitive needle match on each entry's `text`;
/// scoring is "shorter matched entry first" (tie-broken by
/// recency) so a 30-char fact that exactly matches the query
/// ranks above a 1000-char wall of text that happens to contain
/// the same substring as one phrase. Newest-first within a tie.
///
/// `query` is validated for shape: trimmed, non-empty, length
/// within `SEARCH_MAX_QUERY_BYTES`. `limit` is clamped to
/// `1..=SEARCH_MAX_LIMIT`; passing 0 is a request shape error.
///
/// Same symlink-safe path resolver as `read_index` / `forget` —
/// a planted `.plume/` symlink returns the `MemorySearchFailure::
/// StoreFailed` variant rather than dereferencing. No SQLite, no
/// FTS, no embedding model: a flat scan capped at 100 entries × 1
/// KiB = 100 KiB worst case, which is well inside the budget for
/// a synchronous IPC call.
pub fn search(project_root: &Path, query: &str, limit: u32) -> MemorySearchResponse {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return MemorySearchResponse::Err(MemorySearchErr {
            ok: false,
            reason: MemorySearchFailure::EmptyQuery,
            message: "memory.search query is empty or whitespace-only".to_string(),
        });
    }
    if trimmed.len() > SEARCH_MAX_QUERY_BYTES {
        return MemorySearchResponse::Err(MemorySearchErr {
            ok: false,
            reason: MemorySearchFailure::QueryTooLong,
            message: format!(
                "memory.search query is {} bytes; max is {}",
                trimmed.len(),
                SEARCH_MAX_QUERY_BYTES
            ),
        });
    }
    if limit == 0 || limit > SEARCH_MAX_LIMIT {
        return MemorySearchResponse::Err(MemorySearchErr {
            ok: false,
            reason: MemorySearchFailure::BadLimit,
            message: format!(
                "memory.search limit must be between 1 and {SEARCH_MAX_LIMIT}; got {limit}"
            ),
        });
    }

    let _guard = memory_mutex().lock().unwrap_or_else(|e| e.into_inner());
    let entries_path = match resolve_entries_path(project_root) {
        Ok(p) => p,
        Err(err) => {
            return MemorySearchResponse::Err(MemorySearchErr {
                ok: false,
                reason: MemorySearchFailure::StoreFailed,
                message: err.0,
            });
        }
    };
    let entries = match read_entries(&entries_path) {
        Ok((entries, _bytes)) => entries,
        Err(err) => {
            return MemorySearchResponse::Err(MemorySearchErr {
                ok: false,
                reason: MemorySearchFailure::StoreFailed,
                message: err.0,
            });
        }
    };

    // Case-insensitive match on the redacted text. We lowercase the
    // needle once and lowercase each entry once; a small alloc per
    // entry is fine at 100-entry scale.
    let needle = trimmed.to_lowercase();
    let mut hits: Vec<MemorySearchHit> = entries
        .into_iter()
        .filter_map(|entry| {
            let lower = entry.text.to_lowercase();
            if !lower.contains(&needle) {
                return None;
            }
            // Find ALL occurrences for the count; first match
            // index goes on the hit so the UI can highlight it.
            let mut match_count: u32 = 0;
            let mut first_index: Option<u32> = None;
            let mut search_from: usize = 0;
            while let Some(pos) = lower[search_from..].find(&needle) {
                let absolute = search_from + pos;
                if first_index.is_none() {
                    first_index = Some(absolute as u32);
                }
                match_count = match_count.saturating_add(1);
                // Advance past this match's first byte to avoid
                // infinite loop on a zero-width substring (the
                // empty-query case is rejected up top, but
                // defensive).
                search_from = absolute + needle.len().max(1);
                if search_from >= lower.len() {
                    break;
                }
            }
            Some(MemorySearchHit {
                entry,
                match_count,
                first_match_index: first_index.unwrap_or(0),
            })
        })
        .collect();

    // Sort: shorter entries (more likely to be precise matches)
    // first; ties broken by recency (newer `created_ms` first).
    hits.sort_by(|a, b| {
        a.entry
            .text
            .len()
            .cmp(&b.entry.text.len())
            .then_with(|| b.entry.created_ms.cmp(&a.entry.created_ms))
    });

    let truncated = hits.len() > limit as usize;
    hits.truncate(limit as usize);

    MemorySearchResponse::Ok(MemorySearchOk {
        ok: true,
        hits,
        truncated,
        query: trimmed.to_string(),
    })
}

fn err_remember(reason: MemoryRememberFailure, message: String) -> MemoryRememberResponse {
    MemoryRememberResponse::Err(MemoryRememberErr {
        ok: false,
        reason,
        message,
    })
}

fn err_update(reason: MemoryUpdateFailure, message: String) -> MemoryUpdateResponse {
    MemoryUpdateResponse::Err(MemoryUpdateErr {
        ok: false,
        reason,
        message,
    })
}

#[cfg(test)]
#[path = "memory_tests.rs"]
mod tests;
