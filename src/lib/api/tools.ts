// Typed wrappers for the `tools.*` IPC family (D92) — the read-only
// tool-catalog surface behind `docs/TOOL_DISCLOSURE.md`.
//
// Two verbs, both pure reads (no trust gate, no execution):
//   - `tools.list`   — every tool in the catalog, each with its `tier`.
//   - `tools.search` — `core` (always) + the ranked `matched` OPTIONAL
//     tools a query hit. `matched` never contains a core tool — that is
//     the progressive-disclosure scoping the catalog promises.
//
// Listing or finding a tool grants visibility, never permission to run
// it. There is no execution verb here; the executor + its approval gate
// are a later slice. Backend-only for now — no panel consumes this yet.

import { invokeIpc } from './ipc';

export type ToolTier = 'core' | 'optional';

export type ToolParam = {
  name: string;
  summary: string;
};

export type ToolSpec = {
  name: string;
  summary: string;
  tier: ToolTier;
  params: ToolParam[];
};

export type ToolListResponse = {
  tools: ToolSpec[];
};

export type ToolSearchHit = {
  spec: ToolSpec;
  /** Higher ranks first. Deterministic weighted name/summary/param match. */
  score: number;
};

export type ToolSearchResponse = {
  query: string;
  /** Always present — these are already in the prompt. */
  core: ToolSpec[];
  /** Ranked optional matches, capped at the requested limit. */
  matched: ToolSearchHit[];
};

/** Backend `TOOLS_SEARCH_MAX_LIMIT` — a request above this is rejected,
 *  not clamped (mirrors `memory.search`). */
export const TOOLS_SEARCH_MAX_LIMIT = 50;
/** Backend `TOOLS_SEARCH_MAX_QUERY_BYTES`. */
export const TOOLS_SEARCH_MAX_QUERY_BYTES = 256;

export function listTools(): Promise<ToolListResponse> {
  return invokeIpc<Record<string, never>, ToolListResponse>('tools_list', {});
}

export function searchTools(query: string, limit: number): Promise<ToolSearchResponse> {
  return invokeIpc<{ query: string; limit: number }, ToolSearchResponse>('tools_search', {
    query,
    limit,
  });
}
