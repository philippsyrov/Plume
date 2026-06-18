// Typed wrappers for the `memory.*` IPC family (D37 + D43).
//
// Four verbs:
//   - `memory.index` — read the current entries + limits + size.
//   - `memory.remember` — add a new entry; backend redacts secrets
//     and caps size before writing.
//   - `memory.forget` — remove an entry by opaque id; idempotent.
//   - `memory.search` (D43) — read-only substring search across the
//     redacted entries; capped 256-byte query and 1..=50 limit.
//
// Surface rule (same as patch / chat): structured outcomes come
// back IN-BAND. The `Promise` only rejects for IPC-shape problems
// (`Version`) or trust gating (`NeedsApproval`). Empty text, length
// caps, capacity caps, store-write failures — all in-band on the
// response shape.

import { invokeIpc } from './ipc';

export type MemoryEntry = {
  /** Opaque id; pass to `forgetMemory` to remove. */
  id: string;
  /** Unix epoch milliseconds when the entry was remembered. */
  createdMs: number;
  /**
   * Redacted text. Original pre-redaction bytes never reach disk;
   * any secret pattern is replaced with `[REDACTED:<kind>]`.
   */
  text: string;
  /** Number of secret-pattern matches the redactor caught. */
  redactionCount: number;
};

export type MemoryLimits = {
  maxEntries: number;
  maxBytesPerEntry: number;
  maxBytesTotal: number;
};

export type MemoryIndex = {
  entries: MemoryEntry[];
  limits: MemoryLimits;
  /** On-disk size of `entries.jsonl`; `0` if no file yet. */
  totalBytes: number;
};

export type MemoryRememberFailure =
  | 'empty'
  | 'tooLong'
  | 'redactedToEmpty'
  | 'capacityReached'
  | 'storeFailed';

export type MemoryRememberResponse =
  | { ok: true; entry: MemoryEntry }
  | { ok: false; reason: MemoryRememberFailure; message: string };

export type MemoryForgetFailure = 'badId' | 'storeFailed';

export type MemoryForgetResponse =
  | { ok: true; removed: boolean }
  | { ok: false; reason: MemoryForgetFailure; message: string };

export function getMemoryIndex(): Promise<MemoryIndex> {
  return invokeIpc<Record<string, never>, MemoryIndex>('memory_index', {});
}

export function rememberMemory(text: string): Promise<MemoryRememberResponse> {
  return invokeIpc<{ text: string }, MemoryRememberResponse>('memory_remember', { text });
}

export function forgetMemory(entryId: string): Promise<MemoryForgetResponse> {
  return invokeIpc<{ entryId: string }, MemoryForgetResponse>('memory_forget', { entryId });
}

/**
 * D43: search the project memory store. Backend caps:
 *  - query: 256 bytes max, non-empty after trim.
 *  - limit: 1..=50 results.
 *
 * Results are ranked by shorter-entry-first then newest-first.
 * Returns IN-BAND failures the same way `rememberMemory` does —
 * the Promise only rejects on IPC-shape or trust-gate errors.
 */
export type MemorySearchHit = {
  entry: MemoryEntry;
  /** Number of times the query occurs in `entry.text`. */
  matchCount: number;
  /** Byte offset of the first match. UI can scroll/highlight here. */
  firstMatchIndex: number;
};

export type MemorySearchFailure = 'emptyQuery' | 'queryTooLong' | 'badLimit' | 'storeFailed';

export type MemorySearchResponse =
  | { ok: true; hits: MemorySearchHit[]; truncated: boolean; query: string }
  | { ok: false; reason: MemorySearchFailure; message: string };

/** Maximum query length accepted by the backend (D43). Mirrors
 * `SEARCH_MAX_QUERY_BYTES` so the input can guard on the frontend
 * before round-tripping. */
export const MEMORY_SEARCH_MAX_QUERY_BYTES = 256;
/** Backend `SEARCH_MAX_LIMIT`. */
export const MEMORY_SEARCH_MAX_LIMIT = 50;

export function searchMemory(query: string, limit: number): Promise<MemorySearchResponse> {
  return invokeIpc<{ query: string; limit: number }, MemorySearchResponse>('memory_search', {
    query,
    limit,
  });
}

/**
 * D54: read-only distillation preview. Returns the duplicate groups
 * an `apply` step would compact (no apply yet — that's roadmap), the
 * total entry count, and the number of entries an "accept every
 * group" apply would remove. Pure scan — the verb never mutates
 * `entries.jsonl`. Same trust gate as `memory.index`.
 *
 * Two entries are duplicates iff their normalized text is byte-equal:
 *   * Trim leading + trailing whitespace.
 *   * Collapse internal whitespace runs to a single space.
 *   * Lowercase via `.to_lowercase()`.
 *
 * The redaction marker syntax `[REDACTED:<kind>]` survives
 * normalization, so a fact remembered twice with the same secret in
 * the same place still groups as one duplicate set even though the
 * raw secret bytes never reached disk.
 */
export type MemoryDuplicateGroup = {
  /**
   * Opaque group id. Stable across calls while the store hasn't
   * changed; future `memory.distillApply` round-trips this.
   */
  id: string;
  /** Entries in the group, newest first. By default `entries[0]`
   *  would survive an apply; the rest would be removed. */
  entries: MemoryEntry[];
  /** `entries.length - 1`. Pre-computed so the UI doesn't have to
   *  remember "minus one for the survivor". */
  removableCount: number;
};

export type MemoryDistillPreview = {
  duplicateGroups: MemoryDuplicateGroup[];
  /** Total entries in the store at preview time. */
  totalEntries: number;
  /** Sum of `removableCount` across all groups — "how many entries
   *  an apply that accepts every group would remove". */
  wouldRemove: number;
};

export function getMemoryDistillPreview(): Promise<MemoryDistillPreview> {
  return invokeIpc<Record<string, never>, MemoryDistillPreview>('memory_distill_preview', {});
}

export type MemoryDistillApplyFailure = 'storeFailed';

export type MemoryDistillApplyResponse =
  | {
      ok: true;
      /** Duplicate entries actually removed. `0` when every requested
       *  group id was stale (the store changed since the preview). */
      removedEntryCount: number;
      /** Entries left after the rewrite — lets the UI update its
       *  "N of 100" header without a second `memory.index`. */
      remainingEntryCount: number;
      /** Requested group ids that no longer match a live duplicate
       *  group. Each is a no-op; surfaced so the UI can hint a re-scan. */
      unmatchedGroupIds: string[];
    }
  | { ok: false; reason: MemoryDistillApplyFailure; message: string };

/**
 * D64: apply the rule-based (exact-after-normalization) dedupe pass for
 * the confirmed `groupIds` — the first writing verb of the distillation
 * track. The backend re-derives the live groups under the memory mutex
 * and only compacts ids that still match the on-disk store; stale ids
 * are no-ops returned in `unmatchedGroupIds`, never errors. For each
 * matched group the newest entry (`entries[0]`) survives and the rest
 * are removed; the JSONL is rewritten atomically.
 *
 * No undo in v1 — the store is plain JSONL the user can also edit by
 * hand. Returns IN-BAND failures the same way `rememberMemory` does;
 * the Promise only rejects on IPC-shape or trust-gate errors.
 */
export function applyMemoryDistill(groupIds: string[]): Promise<MemoryDistillApplyResponse> {
  return invokeIpc<{ groupIds: string[] }, MemoryDistillApplyResponse>('memory_distill_apply', {
    groupIds,
  });
}

/**
 * D69/D70: one compaction record from the append-only distillation
 * audit log (`.plume/memory/distill-log.jsonl`). Surfaced so the one
 * memory verb that deletes un-named data leaves a visible trail.
 */
export type MemoryDistillLogEntry = {
  /** Unix epoch ms when the compaction was applied. */
  tsMs: number;
  /** Which rule produced it — `"dedupeExact"` in v1. */
  rule: string;
  /** Older duplicate ids removed (sorted). */
  removedIds: string[];
  /** One survivor id kept per compacted group. */
  keptIds: string[];
};

/**
 * D69: read the distillation audit log, newest record first. Bounded
 * on disk to the newest 50 records. Same trust gate as `getMemoryIndex`
 * — the Promise rejects only on IPC-shape or trust-gate errors.
 */
export function getMemoryDistillLog(): Promise<MemoryDistillLogEntry[]> {
  return invokeIpc<Record<string, never>, MemoryDistillLogEntry[]>('memory_distill_log', {});
}
