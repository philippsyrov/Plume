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

use serde::{Deserialize, Serialize};

use super::{
    memory_mutex, now_ms, read_entries, refuse_symlink, resolve_entries_path, resolve_memory_file,
    serialize_entries, write_atomic, MemoryEntry, MemoryStoreError,
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
    /// D81 (Codex review): whether this compaction was recorded in the
    /// append-only audit log (`distill-log.jsonl`). The entries rewrite
    /// commits first and the audit append is best-effort, so a log
    /// failure leaves the deletion done but unrecorded — we surface
    /// `false` here rather than hiding it, keeping the "never hide
    /// memory writes" property honest. `true` when nothing was removed
    /// (no record needed) or the record was written.
    pub audit_logged: bool,
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
    // D81 (Codex review): a no-op apply has nothing to record, so it
    // counts as logged. A real compaction flips this to the audit
    // append's success below.
    let mut audit_logged = true;

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

        // D69: record the compaction in the append-only audit log.
        // Best-effort — the entries rewrite already committed, so a log
        // failure must not undo it or fail the verb; trace and continue.
        // D81: surface the failure as `audit_logged: false` so the
        // unrecorded-but-committed deletion is never silently hidden.
        let kept_ids: Vec<String> = preview
            .duplicate_groups
            .iter()
            .filter(|group| matched_ids.contains(group.id.as_str()))
            .filter_map(|group| group.entries.first().map(|entry| entry.id.clone()))
            .collect();
        let mut removed_ids: Vec<String> = remove_ids.iter().cloned().collect();
        removed_ids.sort();
        let record = DistillLogEntry {
            ts_ms: now_ms(),
            rule: DISTILL_RULE_DEDUPE_EXACT.to_string(),
            removed_ids,
            kept_ids,
        };
        if let Err(e) = append_distill_log(project_root, &record) {
            tracing::warn!(error = %e.0, "failed to append distill audit log");
            audit_logged = false;
        }
    }

    MemoryDistillApplyResponse::Ok(MemoryDistillApplyOk {
        ok: true,
        removed_entry_count,
        remaining_entry_count,
        unmatched_group_ids,
        audit_logged,
    })
}

fn err_distill_apply(message: String) -> MemoryDistillApplyResponse {
    MemoryDistillApplyResponse::Err(MemoryDistillApplyErr {
        ok: false,
        reason: MemoryDistillApplyFailure::StoreFailed,
        message,
    })
}

// ─── D69: distillation audit log ────────────────────────────────────────
//
// Every `distill_apply` that removes ≥1 entry appends a record to
// `<project>/.plume/memory/distill-log.jsonl`. The log is append-only
// from the user's perspective, bounded to the newest
// `DISTILL_LOG_MAX_RECORDS`, symlink-safe, and never read by the hot
// path — `read_distill_log` is the only reader, behind the same trust
// gate as the rest of the memory verbs. It exists so a compaction
// (the one memory verb that deletes data the user didn't name
// individually) leaves a visible, inspectable trail.

const DISTILL_LOG_FILE_NAME: &str = "distill-log.jsonl";

/// Rule tag stored on each audit record. v1 is always exact-after-
/// normalization dedupe; the LLM v2 will add an `"llm"` rule.
const DISTILL_RULE_DEDUPE_EXACT: &str = "dedupeExact";

/// Keep the audit log bounded: only the newest this many compaction
/// records survive each append. The store itself caps at 100 entries,
/// so a deep history adds little signal and only costs disk.
pub(crate) const DISTILL_LOG_MAX_RECORDS: usize = 50;

/// D69: one compaction record. Serialized as a JSONL line under
/// `.plume/memory/distill-log.jsonl`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DistillLogEntry {
    /// Unix epoch milliseconds when the compaction was applied.
    pub ts_ms: u64,
    /// Which rule produced this compaction (`"dedupeExact"` in v1).
    pub rule: String,
    /// Entry ids removed (the older duplicates), sorted.
    pub removed_ids: Vec<String>,
    /// Entry ids kept — one survivor per compacted group.
    pub kept_ids: Vec<String>,
}

/// D69: read the distillation audit log, newest record first. Missing
/// file → empty. Same symlink-safe resolver and process-wide memory
/// mutex as the entries store. The on-disk file is already capped at
/// `DISTILL_LOG_MAX_RECORDS` by `append_distill_log`, so no extra
/// limit argument is needed.
pub fn read_distill_log(project_root: &Path) -> Result<Vec<DistillLogEntry>, MemoryStoreError> {
    let _guard = memory_mutex().lock().unwrap_or_else(|e| e.into_inner());
    let path = resolve_memory_file(project_root, DISTILL_LOG_FILE_NAME)?;
    let mut records = read_distill_log_file(&path)?;
    // Stored oldest-first (append order); newest-first is the useful
    // view for a "recent compactions" surface.
    records.reverse();
    Ok(records)
}

/// Append `entry` to the audit log, keeping only the newest
/// `DISTILL_LOG_MAX_RECORDS`. Atomic rewrite (temp → rename) like the
/// entries store; the caller holds the memory mutex. `pub(crate)` so
/// the cap behavior can be unit-tested without driving a full apply.
pub(crate) fn append_distill_log(
    project_root: &Path,
    entry: &DistillLogEntry,
) -> Result<(), MemoryStoreError> {
    let path = resolve_memory_file(project_root, DISTILL_LOG_FILE_NAME)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| MemoryStoreError(format!("create {}: {}", parent.display(), e)))?;
    }

    let mut records = read_distill_log_file(&path)?;
    records.push(entry.clone());
    let overflow = records.len().saturating_sub(DISTILL_LOG_MAX_RECORDS);
    if overflow > 0 {
        records.drain(0..overflow); // drop the oldest records
    }

    let mut out = String::new();
    for record in &records {
        let line = serde_json::to_string(record)
            .map_err(|e| MemoryStoreError(format!("serialise distill log: {}", e)))?;
        out.push_str(&line);
        out.push('\n');
    }
    write_atomic(&path, out.as_bytes())
}

/// Parse the audit log file, oldest-first (append order). Missing file
/// → empty list; malformed lines are dropped so a hand-edited file
/// stays usable, mirroring `read_entries`.
///
/// D81 (Codex review): refuse a symlinked `distill-log.jsonl` before
/// reading it. The shared resolver only refuses a symlinked `.plume` /
/// `.plume/memory` directory; a symlink planted at the final file would
/// otherwise be dereferenced by `read_to_string`, leaking an arbitrary
/// file's contents (and the write path would follow it too). Both the
/// read verb and the append go through here, so this guards both.
fn read_distill_log_file(path: &Path) -> Result<Vec<DistillLogEntry>, MemoryStoreError> {
    refuse_symlink(path, ".plume/memory/distill-log.jsonl")?;
    let raw = match fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(MemoryStoreError(format!("read {}: {}", path.display(), e))),
    };
    let mut out = Vec::new();
    for line in raw.lines() {
        if line.trim().is_empty() {
            continue;
        }
        if let Ok(record) = serde_json::from_str::<DistillLogEntry>(line) {
            out.push(record);
        }
    }
    Ok(out)
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
