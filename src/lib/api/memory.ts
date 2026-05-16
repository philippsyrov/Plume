// Typed wrappers for the `memory.*` IPC family (D37).
//
// Three verbs:
//   - `memory.index` — read the current entries + limits + size.
//   - `memory.remember` — add a new entry; backend redacts secrets
//     and caps size before writing.
//   - `memory.forget` — remove an entry by opaque id; idempotent.
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
