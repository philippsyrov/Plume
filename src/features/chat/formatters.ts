// Chat panel formatting helpers.
//
// D22 extraction: pulled out of `ChatPanel.tsx` so the panel and
// every collaborator (`ChatEntryRow`, `AttachBar`, `ContextPreview`)
// reach for the same byte / duration / stats formatters. Pure
// functions, no React, no IPC.

import type { ChatStats } from '../../lib/api/chat';
import { formatBytesOneDecimal } from '../../lib/format';

/// Render the one-line stats footer. Returns `null` when the stats
/// object has no information worth displaying — that suppresses the
/// `<p>` entirely so a `chat.done` with all-null stats doesn't add
/// noise to the transcript.
///
/// Format keeps the "feel" of a status strip: short numbers, dots
/// between, no labels on the numbers themselves (the title attribute
/// carries the full prompt-eval breakdown for the curious).
export function formatStatsLine(stats: ChatStats): string | null {
  const parts: string[] = [];
  if (typeof stats.outputTokens === 'number') {
    parts.push(`${stats.outputTokens} ${stats.outputTokens === 1 ? 'token' : 'tokens'}`);
  }
  if (typeof stats.tokensPerSecond === 'number') {
    parts.push(`${stats.tokensPerSecond.toFixed(1)} tok/s`);
  }
  if (parts.length === 0) return null;
  return parts.join(' · ');
}

/// Title attribute for the stats footer — pulled out so the
/// hover-state surface stays auditable in one place. Includes the
/// prompt-eval breakdown that doesn't fit on the visible line.
export function formatStatsTitle(stats: ChatStats): string | undefined {
  const lines: string[] = [];
  if (typeof stats.outputTokens === 'number' && typeof stats.evalMs === 'number') {
    lines.push(`Output: ${stats.outputTokens} tokens in ${formatDuration(stats.evalMs)}`);
  }
  if (typeof stats.promptTokens === 'number' && typeof stats.promptMs === 'number') {
    lines.push(`Prompt: ${stats.promptTokens} tokens in ${formatDuration(stats.promptMs)}`);
  }
  return lines.length === 0 ? undefined : lines.join('\n');
}

export function formatDuration(ms: number): string {
  if (ms < 1000) return `${ms} ms`;
  const seconds = ms / 1000;
  if (seconds < 60) return `${seconds.toFixed(1)} s`;
  const minutes = Math.floor(seconds / 60);
  const remaining = Math.round(seconds % 60);
  return `${minutes} m ${remaining} s`;
}

// D113: moved to the shared `lib/format.ts` — `FileBrowser.tsx` had a
// byte-for-byte identical copy. Re-exported under this name so the
// existing `AttachBar` / `ContextPreview` call sites don't need to change.
export const formatBytes = formatBytesOneDecimal;
