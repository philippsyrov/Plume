// D63B: pure mappers between the visible chat transcript
// (`ChatEntry`, `useChat`) and the persisted wire shape
// (`SessionTranscriptEntry`, `sessions.*`). Kept free of hooks and
// IPC so the boundary rules are unit-testable in isolation.

import type { ChatEntry } from '../chat/useChat';
import type { SessionTranscriptEntry } from '../../lib/api/sessions';

/**
 * The persistable slice of a visible transcript: everything except
 * the in-progress `streaming` placeholder. Element references are
 * preserved so [`sameEntries`] can detect token-only updates (which
 * replace only the streaming entry) as "no persistable change".
 */
export function persistableOf(entries: ChatEntry[]): ChatEntry[] {
  return entries.filter(
    (entry) =>
      entry.kind !== 'streaming' &&
      !(entry.kind === 'message' && entry.pendingContextStreamId !== undefined),
  );
}

/**
 * Reference-equality comparison of two persistable snapshots. Token
 * frames rebuild the entries array but keep every non-streaming
 * element identity, so this is the cheap per-token "nothing to save"
 * check the spec's no-write-on-token rule rides on.
 */
export function sameEntries(a: ChatEntry[], b: ChatEntry[]): boolean {
  if (a.length !== b.length) return false;
  return a.every((entry, i) => entry === b[i]);
}

/**
 * Visible → wire. `streaming` entries must be filtered out before
 * calling (the wire type cannot represent them). A message entry with
 * a transport role (`system`/`tool`) is unrepresentable in the
 * persisted shape and never appears in the visible transcript today;
 * if one ever does, it is skipped here rather than crashing the save
 * — the backend would reject the whole snapshot otherwise.
 */
export function entriesToWire(entries: ChatEntry[]): SessionTranscriptEntry[] {
  const wire: SessionTranscriptEntry[] = [];
  for (const entry of entries) {
    if (entry.kind === 'streaming') continue;
    if (entry.kind === 'error') {
      wire.push({ kind: 'error', message: entry.message });
      continue;
    }
    if (entry.kind === 'cancelled') {
      wire.push({
        kind: 'cancelled',
        partial: entry.partial,
        ...(entry.modelUsed !== undefined ? { modelUsed: entry.modelUsed } : {}),
        ...(entry.durationMs !== undefined ? { durationMs: entry.durationMs } : {}),
      });
      continue;
    }
    const role = entry.message.role;
    if (role !== 'user' && role !== 'assistant') continue;
    wire.push({
      kind: 'message',
      message: { role, content: entry.message.content },
      ...(entry.modelUsed !== undefined ? { modelUsed: entry.modelUsed } : {}),
      ...(entry.durationMs !== undefined ? { durationMs: entry.durationMs } : {}),
      ...(entry.attachmentRelPath !== undefined
        ? { attachmentRelPath: entry.attachmentRelPath }
        : {}),
      ...(entry.attachmentLineRange !== undefined
        ? { attachmentLineRange: entry.attachmentLineRange }
        : {}),
      ...(entry.stats !== undefined ? { stats: entry.stats } : {}),
      ...(entry.sentInMode !== undefined ? { sentInMode: entry.sentInMode } : {}),
      ...(entry.contextSources !== undefined
        ? { contextSources: entry.contextSources }
        : {}),
    });
  }
  return wire;
}

/** Wire → visible, for restoring a loaded session into `useChat`. */
export function wireToEntries(entries: SessionTranscriptEntry[]): ChatEntry[] {
  return entries.map((entry): ChatEntry => {
    if (entry.kind === 'error') {
      return { kind: 'error', message: entry.message };
    }
    if (entry.kind === 'cancelled') {
      return {
        kind: 'cancelled',
        partial: entry.partial,
        ...(entry.modelUsed !== undefined ? { modelUsed: entry.modelUsed } : {}),
        ...(entry.durationMs !== undefined ? { durationMs: entry.durationMs } : {}),
      };
    }
    return {
      kind: 'message',
      message: { role: entry.message.role, content: entry.message.content },
      ...(entry.modelUsed !== undefined ? { modelUsed: entry.modelUsed } : {}),
      ...(entry.durationMs !== undefined ? { durationMs: entry.durationMs } : {}),
      ...(entry.attachmentRelPath !== undefined
        ? { attachmentRelPath: entry.attachmentRelPath }
        : {}),
      ...(entry.attachmentLineRange !== undefined
        ? { attachmentLineRange: entry.attachmentLineRange }
        : {}),
      ...(entry.stats !== undefined ? { stats: entry.stats } : {}),
      ...(entry.sentInMode !== undefined ? { sentInMode: entry.sentInMode } : {}),
      ...(entry.contextSources !== undefined
        ? { contextSources: entry.contextSources }
        : {}),
    };
  });
}
