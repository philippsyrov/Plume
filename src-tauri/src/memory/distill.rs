//! D48 / D54 / D64: memory distillation.
//!
//! Compacts the flat JSONL memory store (`super`) by removing exact-
//! after-normalization duplicates. The track shipped in three slices:
//!
//!   * D48 — `distill_preview`: a pure read that groups entries by
//!     exact normalized match and reports what an apply WOULD remove.
//!     Scaffold only; no IPC, no mutation.
//!   * D54 — wired the preview through `memory.distillPreview` plus a
//!     read-only "Find duplicates" panel disclosure.
//!   * D64 — `distill_apply`: the first writing verb. Removes the
//!     non-survivor entries of the confirmed groups.
//!
//! See `docs/MEMORY_DISTILLATION.md` for the full roadmap (rule-based
//! v1 — here — LLM-driven v2, audit trail, redactor re-run policy).
//!
//! **Properties preserved.** These functions reuse the parent module's
//! symlink-safe path resolver, process-wide memory mutex, and atomic
//! temp→rename writer, so the trust/path/concurrency guarantees that
//! `read_index` / `remember` / `forget` hold are inherited unchanged.
//! The IPC layer is expected to have already passed the trust gate;
//! these functions do not duplicate that check.

use std::fs;
use std::path::Path;

use serde::Serialize;

use super::{
    memory_mutex, read_entries, resolve_entries_path, serialize_entries, write_atomic, MemoryEntry,
    MemoryStoreError,
};

/// D48 / D54: preview of what a distillation apply would remove.
/// Today only carries exact-after-normalization duplicate groups;
/// future slices may add near-duplicate clusters and age-out
/// candidates as additional fields.
///
/// D54 wired this through the new `memory.distillPreview` IPC verb,
/// so the type now serializes as camelCase JSON. The wire shape is
/// `{duplicateGroups, totalEntries, wouldRemove}`.
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
/// only — it never reaches disk and is not exposed via any IPC. The
/// tests pin its rules so a future refactor that shifts cluster
/// boundaries fires a test.
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
/// a different group id. The apply step uses the id to check "the
/// cluster you confirmed is still current"; if the hash only
/// encoded normalized key + size (Codex D48 MEDIUM, pre-fix) a
/// member swap would silently re-use the old id and apply could
/// clobber the wrong entries.
///
/// Sorting member ids before hashing makes the id deterministic
/// regardless of input order, so the test pin
/// `distill_preview_group_ids_are_stable_across_calls` survives
/// any future change to bucket iteration order.
///
/// The id is opaque to callers; today's format is `dup_<hex>_<n>`
/// where `n` is the group size — purely for debug readability.
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
