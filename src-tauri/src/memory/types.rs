//! D108: data types for the memory MVP (D37), split out of `mod.rs` by
//! behavior boundary — this file has no logic, only the wire/response
//! shapes the verbs in `mod.rs` build and return. Mirrors the `pub use`
//! re-export pattern already used for `distill.rs` / `topics.rs` in this
//! module: every type here is re-exported from `mod.rs` so `crate::memory::X`
//! keeps resolving unchanged for external callers.

use serde::{Deserialize, Serialize};

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
    /// Curated `topics/*.md` references. Organization metadata only:
    /// prompt assembly deliberately ignores this field.
    #[serde(default)]
    pub links: Vec<String>,
    /// Bumped whenever the user rewrites this entry's text.
    ///
    /// A compaction checkpoint fact that restated revision N is stale at
    /// revision N+1 even though the entry still exists — the user changed
    /// their mind, and the summary quotes what they replaced. See
    /// `crate::sessions::checkpoint`.
    ///
    /// Absent on disk means 0: an entry written before this field existed has
    /// never been revised. Both memory stores are JSONL rewritten whole on
    /// every mutation, so `serde(default)` is the entire migration — the field
    /// reaches disk on the next write with no backfill pass.
    ///
    /// A link edit deliberately does not bump it: prompt assembly ignores
    /// `links`, so the model never saw them, and bumping would invalidate
    /// every fact drawn from this entry for a change that was never in a
    /// prompt.
    #[serde(default)]
    pub revision: u32,
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
            max_entries: super::MAX_ENTRIES as u32,
            max_bytes_per_entry: super::MAX_BYTES_PER_ENTRY as u32,
            max_bytes_total: super::MAX_BYTES_TOTAL as u32,
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

// D80: in-place edit. Mirrors `remember`'s validation + redaction +
// caps, but replaces an existing entry's text by id while preserving
// its `id` and `created_ms` (an edit fixes wording; it doesn't mint a
// new fact or reorder recency).
#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum MemoryUpdateResponse {
    Ok(MemoryUpdateOk),
    Err(MemoryUpdateErr),
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryUpdateOk {
    pub ok: bool,
    pub entry: MemoryEntry,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryUpdateErr {
    pub ok: bool,
    pub reason: MemoryUpdateFailure,
    pub message: String,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum MemoryUpdateFailure {
    /// Entry id failed shape validation (same gate as `forget`).
    BadId,
    /// Id was well-formed but no entry with it exists.
    NotFound,
    /// New text was empty or whitespace-only after trim.
    Empty,
    /// New text exceeded `MAX_BYTES_PER_ENTRY` (before or after redaction).
    TooLong,
    /// New text reduced to empty after redaction.
    RedactedToEmpty,
    /// The edit would push the store past `MAX_BYTES_TOTAL`.
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
