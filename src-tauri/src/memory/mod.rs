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

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

use crate::prompts::redact::redact;

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

const ENTRIES_FILE_NAME: &str = "entries.jsonl";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MemoryEntry {
    /// Opaque id minted at remember time. Used by `memory.forget`.
    pub id: String,
    /// Unix epoch milliseconds when the entry was remembered.
    /// `u64` so a future "sort by recency" view is straightforward.
    pub created_ms: u64,
    /// Redacted text. The original, pre-redaction string never
    /// reaches disk.
    pub text: String,
    /// Number of secret-pattern matches the redactor caught. `0`
    /// means the user's text had no obvious secrets. Carried for
    /// the panel to surface a "1 value redacted" badge.
    pub redaction_count: u32,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MemoryLimits {
    pub max_entries: u32,
    pub max_bytes_per_entry: u32,
    pub max_bytes_total: u32,
}

impl Default for MemoryLimits {
    fn default() -> Self {
        Self {
            max_entries: MAX_ENTRIES as u32,
            max_bytes_per_entry: MAX_BYTES_PER_ENTRY as u32,
            max_bytes_total: MAX_BYTES_TOTAL as u32,
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MemoryIndex {
    pub entries: Vec<MemoryEntry>,
    pub limits: MemoryLimits,
    /// On-disk byte size of `entries.jsonl`. `0` if the file does
    /// not exist yet.
    pub total_bytes: u64,
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum MemoryRememberResponse {
    Ok(MemoryRememberOk),
    Err(MemoryRememberErr),
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryRememberOk {
    pub ok: bool,
    pub entry: MemoryEntry,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryRememberErr {
    pub ok: bool,
    pub reason: MemoryRememberFailure,
    pub message: String,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum MemoryRememberFailure {
    /// Submitted text was empty or whitespace-only after trim.
    Empty,
    /// Text exceeded `MAX_BYTES_PER_ENTRY` bytes (counted after
    /// trim, before redaction). The user can shorten it and retry.
    TooLong,
    /// Text reduced to empty after redaction — every byte that
    /// would have made it onto disk was a redactor marker.
    RedactedToEmpty,
    /// Entry count or total-byte cap would be exceeded by adding
    /// this entry. `memory.forget` first to free space.
    CapacityReached,
    /// Read or write of the on-disk store failed.
    StoreFailed,
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum MemoryForgetResponse {
    Ok(MemoryForgetOk),
    Err(MemoryForgetErr),
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryForgetOk {
    pub ok: bool,
    /// `true` if an entry with that id was present and removed;
    /// `false` if no entry matched (the verb is idempotent).
    pub removed: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryForgetErr {
    pub ok: bool,
    pub reason: MemoryForgetFailure,
    pub message: String,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum MemoryForgetFailure {
    /// Entry id failed shape validation (empty / non-ascii / wrong
    /// length). The wire id must match the shape `mint_entry_id`
    /// produces.
    BadId,
    /// Read or write of the on-disk store failed.
    StoreFailed,
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum MemorySearchResponse {
    Ok(MemorySearchOk),
    Err(MemorySearchErr),
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemorySearchOk {
    pub ok: bool,
    /// Hits ranked by shorter-entry-first then newest-first. Up to
    /// `limit` items; `truncated` flags when the underlying store
    /// had more matches that didn't fit.
    pub hits: Vec<MemorySearchHit>,
    pub truncated: bool,
    /// Trimmed query the search actually ran. Lets the UI render
    /// "0 results for 'foo'" with the exact text the backend used
    /// (so an accidental leading space doesn't surface in the
    /// "no results" message).
    pub query: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemorySearchHit {
    /// The full matched entry. The panel re-uses the same row
    /// renderer as the index list — entry id, text, redaction
    /// count, created ms.
    pub entry: MemoryEntry,
    /// Number of times `query` occurs in `entry.text`
    /// (case-insensitive). Useful for the UI's "5 matches" hint.
    pub match_count: u32,
    /// Byte offset of the FIRST match in `entry.text`. Caller can
    /// scroll a highlight here. Zero is meaningful (the match
    /// starts at the beginning); we'd only need a sentinel if the
    /// no-match case could escape, and it can't — `filter_map`
    /// drops misses up front.
    pub first_match_index: u32,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemorySearchErr {
    pub ok: bool,
    pub reason: MemorySearchFailure,
    pub message: String,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum MemorySearchFailure {
    /// Query was empty after trim. Distinct from "no results";
    /// the panel renders this as a hint to type something.
    EmptyQuery,
    /// Query exceeded `SEARCH_MAX_QUERY_BYTES`.
    QueryTooLong,
    /// Limit was `0` or > `SEARCH_MAX_LIMIT`.
    BadLimit,
    /// Read of the on-disk store failed (planted symlink, etc).
    StoreFailed,
}

#[derive(Debug)]
pub struct MemoryStoreError(pub String);

impl std::fmt::Display for MemoryStoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for MemoryStoreError {}

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

/// Output of `read_for_prompt`. Carries the picked entries plus the
/// summary numbers the chat preview and the chat-send response echo
/// to the frontend. `truncated` is `true` when at least one entry
/// was skipped to stay within `byte_cap`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryPromptRead {
    pub entries: Vec<MemoryEntry>,
    pub used_bytes: usize,
    pub byte_cap: usize,
    pub truncated: bool,
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

// ─── Internals ──────────────────────────────────────────────────────────────

/// Symlink-safe entries-path resolver shared by every memory verb
/// (Codex D37 HIGH). Refuses if `.plume/` or `.plume/memory/`
/// exists as a symlink — same guard the patch checkpoint uses.
/// Does NOT create the directories; the missing-path case is
/// fine (read_index / forget treat a missing file as "no
/// entries"). `remember` calls `ensured_entries_path` instead,
/// which adds the `create_dir_all` step on top of this check.
fn resolve_entries_path(project_root: &Path) -> Result<PathBuf, MemoryStoreError> {
    let plume_dir = project_root.join(".plume");
    refuse_symlink(&plume_dir, ".plume")?;
    let memory_dir = plume_dir.join("memory");
    refuse_symlink(&memory_dir, ".plume/memory")?;
    Ok(memory_dir.join(ENTRIES_FILE_NAME))
}

/// Same as `resolve_entries_path` plus ensures the directories
/// exist. Only `remember` needs the create step; read and forget
/// tolerate a missing tree (treated as empty / no-op).
fn ensured_entries_path(project_root: &Path) -> Result<PathBuf, MemoryStoreError> {
    let plume_dir = project_root.join(".plume");
    refuse_symlink(&plume_dir, ".plume")?;
    fs::create_dir_all(&plume_dir)
        .map_err(|e| MemoryStoreError(format!("create .plume/: {}", e)))?;

    let memory_dir = plume_dir.join("memory");
    refuse_symlink(&memory_dir, ".plume/memory")?;
    fs::create_dir_all(&memory_dir)
        .map_err(|e| MemoryStoreError(format!("create .plume/memory/: {}", e)))?;

    Ok(memory_dir.join(ENTRIES_FILE_NAME))
}

/// Reject any pre-existing path that's a symlink — `fs::create_dir_all`
/// would follow it and write memory files outside the project
/// root. Missing (NotFound) is fine; we'll create the path as a
/// regular directory. Local copy of the same guard
/// `patch::checkpoint::ensure_not_symlink` uses; kept local so the
/// memory module doesn't depend on a `pub(crate)` from patch.
fn refuse_symlink(path: &Path, label: &str) -> Result<(), MemoryStoreError> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => Err(MemoryStoreError(format!(
            "{label} is a symlink; refusing to write memory through it"
        ))),
        Ok(_) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(MemoryStoreError(format!("stat {label}: {e}"))),
    }
}

/// Read every JSONL line in `path` as a `MemoryEntry`. Missing
/// file → empty list. Malformed lines are dropped (the panel
/// stays usable when a user hand-edits the file and fat-fingers
/// one line). Oversize files are rejected: if the on-disk file is
/// past `MAX_BYTES_TOTAL`, we refuse to parse it rather than
/// surface arbitrarily many entries.
fn read_entries(path: &Path) -> Result<(Vec<MemoryEntry>, u64), MemoryStoreError> {
    let raw = match fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok((Vec::new(), 0));
        }
        Err(e) => {
            return Err(MemoryStoreError(format!("read {}: {}", path.display(), e)));
        }
    };
    let total_bytes = raw.len() as u64;
    if total_bytes > MAX_BYTES_TOTAL {
        return Err(MemoryStoreError(format!(
            "memory store {} is {} bytes; max is {} (delete the file or trim it manually)",
            path.display(),
            total_bytes,
            MAX_BYTES_TOTAL
        )));
    }
    let mut entries = Vec::new();
    for line in raw.lines() {
        if line.trim().is_empty() {
            continue;
        }
        if let Ok(entry) = serde_json::from_str::<MemoryEntry>(line) {
            entries.push(entry);
        }
    }
    Ok((entries, total_bytes))
}

fn serialize_entries(entries: &[MemoryEntry]) -> Result<String, MemoryStoreError> {
    let mut out = String::new();
    for e in entries {
        let line = serde_json::to_string(e)
            .map_err(|err| MemoryStoreError(format!("serialise entry: {}", err)))?;
        out.push_str(&line);
        out.push('\n');
    }
    Ok(out)
}

/// Sibling-tempfile + atomic rename. Same pattern as
/// `patch::apply::write_atomic`; reimplemented locally so the
/// memory module doesn't depend on a `pub(crate)` from patch.
fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), MemoryStoreError> {
    let parent = path
        .parent()
        .ok_or_else(|| MemoryStoreError(format!("memory path {} has no parent", path.display())))?;
    let file_name = path.file_name().and_then(|n| n.to_str()).ok_or_else(|| {
        MemoryStoreError(format!("memory path {} has no filename", path.display()))
    })?;
    let nanos = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let tmp_path = parent.join(format!(".{}.plume-mem-{}.tmp", file_name, nanos));
    {
        let mut f = fs::File::create(&tmp_path)
            .map_err(|e| MemoryStoreError(format!("create temp {}: {}", tmp_path.display(), e)))?;
        if let Err(e) = f.write_all(bytes) {
            let _ = fs::remove_file(&tmp_path);
            return Err(MemoryStoreError(format!(
                "write temp {}: {}",
                tmp_path.display(),
                e
            )));
        }
        if let Err(e) = f.sync_all() {
            let _ = fs::remove_file(&tmp_path);
            return Err(MemoryStoreError(format!(
                "sync temp {}: {}",
                tmp_path.display(),
                e
            )));
        }
    }
    fs::rename(&tmp_path, path).map_err(|e| {
        let _ = fs::remove_file(&tmp_path);
        MemoryStoreError(format!("rename -> {}: {}", path.display(), e))
    })
}

/// Mint a 32-hex-char entry id. Time-sortable so newest entries
/// sort to the bottom of the file naturally. Same shape and
/// generator as `checkpoint::checkpoint_id` — kept independent so
/// the memory module doesn't depend on a `pub(crate)` from
/// checkpoint.
fn mint_entry_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let pid = std::process::id() as u128;
    let combined = (nanos << 32) | (pid & 0xFFFFFFFF);
    format!("m_{:032x}", combined)
}

/// Wire-shape gate for entry ids. Production-minted ids match
/// `m_[0-9a-f]{32}`; rejecting anything else stops a tampered
/// payload from sneaking a path-like id past the trust gate.
fn is_valid_entry_id(id: &str) -> bool {
    if id.len() != 34 {
        return false;
    }
    if !id.starts_with("m_") {
        return false;
    }
    id[2..].chars().all(|c| c.is_ascii_hexdigit())
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn err_remember(reason: MemoryRememberFailure, message: String) -> MemoryRememberResponse {
    MemoryRememberResponse::Err(MemoryRememberErr {
        ok: false,
        reason,
        message,
    })
}

// --- D48: distillation preview (read-only, no IPC) ----------------------
//
// `distill_preview` is the smallest safe scaffold for memory
// distillation: a pure read that groups entries by exact normalized
// match and reports what an `apply` step WOULD remove. There is no
// `apply` yet; there is no IPC verb wiring this function up. Future
// slices wire `memory.distillPreview` / `memory.distillApply` once
// the approval flow + JSONL rewrite live behind a UI button.
//
// See `docs/MEMORY_DISTILLATION.md` for the full roadmap (rule-based
// v1, LLM-driven v2, audit trail, redactor re-run policy).
//
// **Properties preserved.** Same trust contract as `read_index` /
// `read_for_prompt`: the caller is expected to have already passed
// the trust gate at the IPC layer; this Rust function does not
// duplicate that check. Symlink defense and process-wide mutex are
// the same. No mutation, no on-disk write — the function reads
// `entries.jsonl` and returns a structured preview.

/// D48 / D54: preview of what a distillation apply would remove.
/// Today only carries exact-after-normalization duplicate groups;
/// future slices may add near-duplicate clusters and age-out
/// candidates as additional fields.
///
/// D54 wired this through the new `memory.distillPreview` IPC verb,
/// so the type now serializes as camelCase JSON. The wire shape is
/// `{duplicateGroups, totalEntries, wouldRemove}`. Apply / rewrite is
/// still future work — the preview verb is read-only and never
/// mutates `entries.jsonl`.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DistillPreview {
    /// One group per duplicate set. Each group has 2+ entries.
    pub duplicate_groups: Vec<DuplicateGroup>,
    /// Total entries in the store at preview time.
    pub total_entries: u32,
    /// Sum of `group.removable_count` across all groups. Equivalent
    /// to "how many entries an apply that accepts every group
    /// would remove". The frontend renders "would compact from N
    /// to (N - wouldRemove)".
    pub would_remove: u32,
}

/// D48 / D54: one duplicate set. Entries are sorted newest-first so
/// the would-be survivor is `entries[0]`. The `id` is opaque so a
/// future apply step can change which entry survives without
/// breaking saved group ids in flight.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DuplicateGroup {
    /// Opaque group id. Stable across calls while the store hasn't
    /// changed; meant to round-trip through a future apply call.
    pub id: String,
    /// Entries in the group, newest first. By default the first
    /// would survive an apply; the rest would be removed.
    pub entries: Vec<MemoryEntry>,
    /// Convenience: `entries.len() - 1`. Pre-computed so callers
    /// don't have to remember "minus one for the survivor".
    pub removable_count: u32,
}

/// D64: response from `memory.distillApply`. Structured-in-band like
/// `remember` / `forget` — the Promise only rejects on IPC-shape or
/// trust-gate errors; store-write failures come back as the `Err`
/// variant.
#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum MemoryDistillApplyResponse {
    Ok(MemoryDistillApplyOk),
    Err(MemoryDistillApplyErr),
}

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MemoryDistillApplyOk {
    pub ok: bool,
    /// How many duplicate entries were actually removed from disk.
    /// `0` when every requested group id was stale (a no-op apply).
    pub removed_entry_count: u32,
    /// Entry count left in the store after the rewrite. Lets the UI
    /// update its "N of 100" header without a second `memory.index`.
    pub remaining_entry_count: u32,
    /// Requested group ids that no longer match a live duplicate
    /// group (the store changed between preview and apply). Each is a
    /// no-op, not an error; surfaced so the UI can hint "re-scan".
    pub unmatched_group_ids: Vec<String>,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MemoryDistillApplyErr {
    pub ok: bool,
    pub reason: MemoryDistillApplyFailure,
    pub message: String,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum MemoryDistillApplyFailure {
    /// Read or write of the on-disk store failed (planted symlink,
    /// rename error, serialise error).
    StoreFailed,
}

/// D48: produce a read-only preview of duplicate groups in the
/// memory store. Pure scan; no mutation. Same symlink-safe path
/// resolver and process-wide mutex as `read_index`.
///
/// Normalization (matches the v1 rule in
/// `docs/MEMORY_DISTILLATION.md`):
///   * Trim leading and trailing whitespace.
///   * Collapse internal whitespace runs to a single space.
///   * Lowercase via `to_lowercase()`.
///
/// Two entries are considered duplicates iff their normalized
/// strings are byte-equal. The redaction marker syntax
/// `[REDACTED:<kind>]` survives normalization unchanged, so a
/// fact remembered twice with the same secret in the same place
/// will still group as one duplicate set even though the raw
/// secret bytes never reached disk.
///
/// D54: now reachable from the `memory.distillPreview` IPC handler;
/// no longer scaffold-only. Pre-D54 the function was gated on
/// `#[allow(dead_code)]` because nothing in production routed to it.
pub fn distill_preview(project_root: &Path) -> Result<DistillPreview, MemoryStoreError> {
    let _guard = memory_mutex().lock().unwrap_or_else(|e| e.into_inner());
    let entries_path = resolve_entries_path(project_root)?;
    let (entries, _total_bytes) = read_entries(&entries_path)?;
    Ok(build_distill_preview(entries))
}

/// Pure grouping pass shared by `distill_preview` (D54) and
/// `distill_apply` (D64). Groups entries by exact normalized text,
/// keeps groups of 2+, sorts each newest-first so the survivor is
/// `entries[0]`, and mints a membership-stable group id. The caller
/// is expected to hold the memory mutex; this function does no I/O.
///
/// Extracted so apply re-derives the SAME groups + ids the preview
/// showed, inside the same lock, and can match the frontend's
/// confirmed ids against the live store.
fn build_distill_preview(entries: Vec<MemoryEntry>) -> DistillPreview {
    let total_entries = entries.len();

    // Group by normalized text. We collect into a BTreeMap to make
    // iteration order deterministic for tests; the ordering itself is
    // not part of the public contract.
    let mut buckets: std::collections::BTreeMap<String, Vec<MemoryEntry>> =
        std::collections::BTreeMap::new();
    for entry in entries.into_iter() {
        let key = normalize_for_distill(&entry.text);
        // Skip entries that normalize to empty — they wouldn't
        // duplicate anything useful and an empty key would cluster
        // unrelated noise together.
        if key.is_empty() {
            continue;
        }
        buckets.entry(key).or_default().push(entry);
    }

    let mut duplicate_groups: Vec<DuplicateGroup> = Vec::new();
    let mut would_remove: u32 = 0;
    for (key, mut group_entries) in buckets {
        if group_entries.len() < 2 {
            continue;
        }
        // Newest-first inside each group so the survivor is the
        // most recently remembered entry. Matches the D42 picker's
        // "newest wins" disposition.
        group_entries.sort_by_key(|e| std::cmp::Reverse(e.created_ms));
        // Saturating because `MAX_ENTRIES = 100` makes overflow
        // impossible in practice, but a future cap bump shouldn't
        // make us panic on a degenerate store.
        let removable_count = (group_entries.len() as u32).saturating_sub(1);
        would_remove = would_remove.saturating_add(removable_count);
        let id = distill_group_id(&key, &group_entries);
        duplicate_groups.push(DuplicateGroup {
            id,
            entries: group_entries,
            removable_count,
        });
    }

    DistillPreview {
        duplicate_groups,
        total_entries: total_entries as u32,
        would_remove,
    }
}

/// D64: apply the rule-based (exact-after-normalization) dedupe pass
/// for the confirmed `group_ids`. The first writing verb of the
/// distillation track.
///
/// Semantics (per `docs/MEMORY_DISTILLATION.md § Apply semantics`):
///
/// * The whole read → re-group → remove → write cycle runs under the
///   memory mutex so a concurrent `remember` / `forget` can't shift
///   the entry set mid-apply.
/// * The preview is RE-COMPUTED inside the lock. Only groups whose id
///   still matches the live store are touched — a `forget` +
///   `remember` between preview and apply changes the group id, so a
///   stale id lands in `unmatched_group_ids` and is a no-op, never an
///   error and never a wrong-entry deletion.
/// * For each matched group the survivor is `entries[0]` (newest);
///   the rest are removed. The JSONL is rewritten atomically (temp →
///   rename) like `forget`; survivors keep their on-disk order.
/// * No undo in v1. The store is plain JSONL the user can also edit
///   by hand; the LLM v2 will add a pre-apply snapshot.
///
/// An empty `group_ids`, or a list of only stale ids, is a successful
/// no-op (`removed_entry_count == 0`).
pub fn distill_apply(project_root: &Path, group_ids: &[String]) -> MemoryDistillApplyResponse {
    let _guard = memory_mutex().lock().unwrap_or_else(|e| e.into_inner());

    let entries_path = match resolve_entries_path(project_root) {
        Ok(p) => p,
        Err(e) => return err_distill_apply(e.0),
    };
    let (entries, _bytes) = match read_entries(&entries_path) {
        Ok(p) => p,
        Err(e) => return err_distill_apply(e.0),
    };

    // Re-derive the current groups INSIDE the lock so the ids the
    // frontend confirmed are validated against the live store. Clone
    // the entries because `build_distill_preview` consumes them and we
    // still need the originals to filter survivors below.
    let preview = build_distill_preview(entries.clone());

    let requested: std::collections::HashSet<&str> = group_ids.iter().map(|s| s.as_str()).collect();
    let mut matched_ids: std::collections::HashSet<&str> = std::collections::HashSet::new();
    let mut remove_ids: std::collections::HashSet<String> = std::collections::HashSet::new();
    for group in &preview.duplicate_groups {
        if requested.contains(group.id.as_str()) {
            matched_ids.insert(group.id.as_str());
            // entries[0] is the newest survivor; every later entry in
            // the group is a duplicate to drop.
            for entry in group.entries.iter().skip(1) {
                remove_ids.insert(entry.id.clone());
            }
        }
    }

    // Requested ids that didn't match a current group, de-duplicated
    // while preserving the caller's order so the response is stable.
    let mut unmatched_group_ids: Vec<String> = Vec::new();
    for id in group_ids {
        if matched_ids.contains(id.as_str()) {
            continue;
        }
        if unmatched_group_ids.iter().any(|u| u == id) {
            continue;
        }
        unmatched_group_ids.push(id.clone());
    }

    let original_len = entries.len();
    let remaining: Vec<MemoryEntry> = entries
        .into_iter()
        .filter(|e| !remove_ids.contains(&e.id))
        .collect();
    let removed_entry_count = (original_len - remaining.len()) as u32;
    let remaining_entry_count = remaining.len() as u32;

    if removed_entry_count > 0 {
        if remaining.is_empty() {
            // Can't actually happen — every matched group keeps a
            // survivor — but mirror `forget`'s empty-store handling so
            // a future rule that could empty the file stays correct.
            let _ = fs::remove_file(&entries_path);
        } else {
            let serialized = match serialize_entries(&remaining) {
                Ok(s) => s,
                Err(e) => return err_distill_apply(e.0),
            };
            if let Err(e) = write_atomic(&entries_path, serialized.as_bytes()) {
                return err_distill_apply(e.0);
            }
        }
    }

    MemoryDistillApplyResponse::Ok(MemoryDistillApplyOk {
        ok: true,
        removed_entry_count,
        remaining_entry_count,
        unmatched_group_ids,
    })
}

fn err_distill_apply(message: String) -> MemoryDistillApplyResponse {
    MemoryDistillApplyResponse::Err(MemoryDistillApplyErr {
        ok: false,
        reason: MemoryDistillApplyFailure::StoreFailed,
        message,
    })
}

/// Normalize a memory entry's text for distill comparison. Trim,
/// collapse whitespace runs, lowercase. The output is informational
/// only — it never reaches disk and is not exposed via any IPC.
/// `#[allow(dead_code)]`: same D48 scaffold rationale; the tests
/// pin its rules so a future refactor that shifts cluster
/// boundaries fires a test.
#[allow(dead_code)]
pub(crate) fn normalize_for_distill(text: &str) -> String {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    let mut out = String::with_capacity(trimmed.len());
    let mut last_was_space = false;
    for ch in trimmed.chars() {
        if ch.is_whitespace() {
            if !last_was_space {
                out.push(' ');
                last_was_space = true;
            }
        } else {
            out.push(ch);
            last_was_space = false;
        }
    }
    out.to_lowercase()
}

/// Stable-ish id for a duplicate group. Combines a short hash of
/// the normalized key with the SORTED set of member entry ids so
/// any change to group membership — including a same-size swap
/// (one member forgotten + a different duplicate added) — produces
/// a different group id. The future apply step uses the id to
/// check "the cluster you confirmed is still current"; if the
/// hash only encoded normalized key + size (Codex D48 MEDIUM, pre-
/// fix) a member swap would silently re-use the old id and apply
/// could clobber the wrong entries.
///
/// Sorting member ids before hashing makes the id deterministic
/// regardless of input order, so the test pin
/// `distill_preview_group_ids_are_stable_across_calls` survives
/// any future change to bucket iteration order.
///
/// The id is opaque to callers; today's format is `dup_<hex>_<n>`
/// where `n` is the group size — purely for debug readability.
/// `#[allow(dead_code)]`: same D48 scaffold rationale.
#[allow(dead_code)]
fn distill_group_id(normalized_key: &str, entries: &[MemoryEntry]) -> String {
    // FNV-1a 64-bit. Cheap, no dependency, plenty of bits for the
    // tiny dedup set the memory store can hold. Not used for
    // anything security-sensitive — the redactor already ran on
    // the text before it landed on disk.
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in normalized_key.bytes() {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x100_0000_01b3);
    }
    // NUL byte separates the key from the member-id segment so a
    // shorter key that happens to look like a longer key's prefix
    // can't accidentally hash to the same value as a longer
    // key + different members. NULs never appear in memory entry
    // text (the remember verb's path-safety check rejects them)
    // and never in the FNV state; they're safe as a domain
    // separator.
    hash ^= 0u64;
    hash = hash.wrapping_mul(0x100_0000_01b3);
    let mut member_ids: Vec<&str> = entries.iter().map(|e| e.id.as_str()).collect();
    member_ids.sort_unstable();
    for id in member_ids {
        for byte in id.bytes() {
            hash ^= byte as u64;
            hash = hash.wrapping_mul(0x100_0000_01b3);
        }
        // Per-id separator so id `ab` followed by id `c` doesn't
        // hash like a single id `abc`.
        hash ^= 0u64;
        hash = hash.wrapping_mul(0x100_0000_01b3);
    }
    format!("dup_{hash:016x}_{}", entries.len())
}

#[cfg(test)]
#[path = "memory_tests.rs"]
mod tests;
