// D65: automatic chat titles, derived locally from the first
// accepted user message. Pure string/entry helpers — no hooks, no
// IPC, and deliberately NO model involvement: the title is a
// deterministic function of the transcript.

import type { ChatEntry } from '../chat/useChat';

/** Backend-minted default title — mirrors
 * `src-tauri/src/sessions/validation.rs::DEFAULT_TITLE`. A session
 * still carrying this title has never been titled by the user, so
 * it is eligible for auto-titling. */
export const DEFAULT_SESSION_TITLE = 'New chat';

/** Cap for derived titles, in Unicode code points. Chosen for
 * sidebar readability; well under the backend's 120-scalar
 * `MAX_TITLE_CHARS` so a derived title can never be rejected for
 * length. */
export const SESSION_TITLE_MAX_CHARS = 60;

/** How far back from the cap a space still counts as a "nearby"
 * word boundary. A break further back than this would waste too
 * much of the cap on a single long word, so we hard-cut instead. */
const WORD_BREAK_WINDOW = 20;

/**
 * Collapse a raw message into sidebar-title shape: whitespace runs
 * (spaces, newlines, tabs) become single spaces, ends are trimmed,
 * and anything longer than the cap is cut — at a nearby word
 * boundary when one exists — with an ellipsis. Returns `null` when
 * nothing displayable is left.
 */
export function normalizeSessionTitle(raw: string): string | null {
  const collapsed = raw.replace(/\s+/g, ' ').trim();
  if (collapsed.length === 0) return null;
  // Slice by code point, not UTF-16 unit, so a cut can never split
  // a surrogate pair (emoji, CJK extensions) into a lone half.
  const points = Array.from(collapsed);
  if (points.length <= SESSION_TITLE_MAX_CHARS) return collapsed;
  const slice = points.slice(0, SESSION_TITLE_MAX_CHARS).join('');
  const lastSpace = slice.lastIndexOf(' ');
  const stem =
    lastSpace >= SESSION_TITLE_MAX_CHARS - WORD_BREAK_WINDOW
      ? slice.slice(0, lastSpace)
      : slice;
  return `${stem.trimEnd()}…`;
}

/**
 * Title for a transcript snapshot: the normalized first user
 * message. `null` when the snapshot has no user message yet (e.g.
 * a restored error-only transcript) — the caller skips the rename
 * and a later boundary retries.
 */
export function deriveSessionTitle(entries: ChatEntry[]): string | null {
  for (const entry of entries) {
    if (entry.kind !== 'message') continue;
    if (entry.message.role !== 'user') continue;
    return normalizeSessionTitle(entry.message.content);
  }
  return null;
}
