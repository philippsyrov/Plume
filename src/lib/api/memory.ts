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
// (`Version`) or, for project-memory verbs, trust gating
// (`NeedsApproval`). The `memory_user_*` wrappers use a backend-owned
// app-data path and do not require project trust. Empty text, length caps,
// capacity caps, store-write failures — all in-band on the response shape.

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
  /** Curated topic references; organization metadata, not prompt context. */
  links: string[];
  /**
   * Bumped whenever the user rewrites this entry's text. Required, not
   * optional: it is durable state, and an optional field would let a caller
   * treat "absent" as a valid wire shape forever. The backend defaults a
   * legacy on-disk entry to 0 before it ever reaches here.
   *
   * Editing links does not bump it — prompt assembly ignores links.
   */
  revision: number;
};

export type MemorySetLinksFailure =
  | 'badId'
  | 'notFound'
  | 'capacityReached'
  | 'tooMany'
  | 'duplicate'
  | 'invalidTopic'
  | 'topicNotFound'
  | 'storeFailed';

export type MemorySetLinksResponse =
  | { ok: true; entry: MemoryEntry }
  | { ok: false; reason: MemorySetLinksFailure; message: string };

export function setMemoryLinks(id: string, links: string[]): Promise<MemorySetLinksResponse> {
  return invokeIpc<{ id: string; links: string[] }, MemorySetLinksResponse>('memory_set_links', {
    id,
    links,
  });
}

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

export type MemoryUpdateFailure =
  | 'badId'
  | 'notFound'
  | 'empty'
  | 'tooLong'
  | 'redactedToEmpty'
  | 'capacityReached'
  | 'storeFailed';

export type MemoryUpdateResponse =
  | { ok: true; entry: MemoryEntry }
  | { ok: false; reason: MemoryUpdateFailure; message: string };

/**
 * D80: edit an existing memory entry in place. The new text is
 * re-redacted and re-capped server-side exactly like `rememberMemory`;
 * the entry's `id` and `createdMs` are preserved. A well-formed id that
 * matches no entry returns `notFound`. IN-BAND failures like the other
 * write verbs — the Promise only rejects on IPC-shape / trust-gate.
 */
export function updateMemory(entryId: string, text: string): Promise<MemoryUpdateResponse> {
  return invokeIpc<{ entryId: string; text: string }, MemoryUpdateResponse>('memory_update', {
    entryId,
    text,
  });
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
 * App-private memory about the user. This is a distinct wire type from
 * project memory: user entries deliberately have no project-topic links.
 */
export type UserMemoryEntry = {
  id: string;
  createdMs: number;
  text: string;
  redactionCount: number;
  /** See {@link MemoryEntry.revision}. */
  revision: number;
};

export type UserMemoryIndex = {
  entries: UserMemoryEntry[];
  limits: MemoryLimits;
  totalBytes: number;
};

export type UserMemoryRememberResponse =
  | { ok: true; entry: UserMemoryEntry }
  | { ok: false; reason: MemoryRememberFailure; message: string };

export type UserMemoryUpdateResponse =
  | { ok: true; entry: UserMemoryEntry }
  | { ok: false; reason: MemoryUpdateFailure; message: string };

export type UserMemoryForgetResponse = MemoryForgetResponse;

export type UserMemorySearchHit = {
  entry: UserMemoryEntry;
  matchCount: number;
  firstMatchIndex: number;
};

export type UserMemorySearchResponse =
  | { ok: true; hits: UserMemorySearchHit[]; truncated: boolean; query: string }
  | { ok: false; reason: MemorySearchFailure; message: string };

export function getUserMemoryIndex(): Promise<UserMemoryIndex> {
  return invokeIpc<Record<string, never>, UserMemoryIndex>('memory_user_index', {});
}

export function rememberUserMemory(text: string): Promise<UserMemoryRememberResponse> {
  return invokeIpc<{ text: string }, UserMemoryRememberResponse>('memory_user_remember', { text });
}

export function updateUserMemory(
  entryId: string,
  text: string,
): Promise<UserMemoryUpdateResponse> {
  return invokeIpc<{ entryId: string; text: string }, UserMemoryUpdateResponse>(
    'memory_user_update',
    { entryId, text },
  );
}

export function forgetUserMemory(entryId: string): Promise<UserMemoryForgetResponse> {
  return invokeIpc<{ entryId: string }, UserMemoryForgetResponse>('memory_user_forget', {
    entryId,
  });
}

export function searchUserMemory(query: string, limit: number): Promise<UserMemorySearchResponse> {
  return invokeIpc<{ query: string; limit: number }, UserMemorySearchResponse>(
    'memory_user_search',
    { query, limit },
  );
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
  /** Deterministic, deduplicated union of every group entry's topic
   *  links — what an apply would fold into the surviving (newest)
   *  entry so links held only by a removed duplicate are not lost.
   *  Sorted. May exceed the per-entry cap; see `linkCapExceeded`. */
  mergedLinks: string[];
  /** `true` when `mergedLinks` would exceed the per-entry link cap. An
   *  apply refuses such a group (no removal, no link write) rather than
   *  truncating; the UI blocks it and asks the user to prune links
   *  first. */
  linkCapExceeded: boolean;
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
      /** Requested group ids left UNCHANGED because merging their topic
       *  links would exceed the per-entry cap. No entry removed, no link
       *  written; surfaced so the UI can tell the user to prune links
       *  before compacting — never silently dropped. */
      conflictedGroupIds: string[];
      /** D81: whether this compaction was recorded in the append-only
       *  audit log. The deletion commits first and the audit append is
       *  best-effort, so `false` means the entries were removed but the
       *  record could not be written — surfaced rather than hidden. */
      auditLogged: boolean;
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
  /** Topic links folded into a survivor from the duplicates removed
   *  alongside it — one record per compacted group that gained links,
   *  so the transfer is visible in the audit trail. Absent on records
   *  written before this field existed. */
  linkMerges: { survivorId: string; links: string[] }[];
};

/**
 * D69: read the distillation audit log, newest record first. Bounded
 * on disk to the newest 50 records. Same trust gate as `getMemoryIndex`
 * — the Promise rejects only on IPC-shape or trust-gate errors.
 */
export function getMemoryDistillLog(): Promise<MemoryDistillLogEntry[]> {
  return invokeIpc<Record<string, never>, MemoryDistillLogEntry[]>('memory_distill_log', {});
}

/**
 * D71: curated memory topic files. Beyond the flat entries store, the
 * North Star describes human-authored Markdown under `.plume/memory/`:
 * the always-loaded core trio (`INDEX.md` / `USER.md` / `SOUL.md`) and
 * `topics/*.md` reference docs. Read-only and capped; Plume does not
 * write these in D71 (the user authors them in their own editor).
 */
export type MemoryTopicKind = 'index' | 'user' | 'soul' | 'topic';

export type MemoryTopicFile = {
  /** Path relative to `.plume/memory/`, e.g. `"INDEX.md"` or
   *  `"topics/architecture.md"`. */
  name: string;
  kind: MemoryTopicKind;
  exists: boolean;
  /** Full on-disk byte size (before capping); `0` if missing. */
  bytes: number;
  /** Content was longer than its cap and was trimmed. */
  truncated: boolean;
  /** Capped, UTF-8-safe content; empty when missing. */
  content: string;
};

export type MemoryTopicLimits = {
  maxCoreBytes: number;
  maxTopicBytes: number;
  maxTopics: number;
};

export type MemoryTopics = {
  /** Always the core trio in fixed order (index, user, soul), even
   *  when missing. */
  core: MemoryTopicFile[];
  /** `topics/*.md`, sorted by name, capped to `limits.maxTopics`. */
  topics: MemoryTopicFile[];
  /** More than `limits.maxTopics` topic files existed; surplus dropped. */
  topicsTruncated: boolean;
  limits: MemoryTopicLimits;
};

/** D71: read the curated memory topic files. Same trust gate as
 *  `getMemoryIndex`. */
export function getMemoryTopics(): Promise<MemoryTopics> {
  return invokeIpc<Record<string, never>, MemoryTopics>('memory_topics', {});
}
