// D102: window-local history of single-step runs. Pure helpers + the record
// shape; the panel owns the state. NOTHING here touches disk or IPC — the
// history lives only for the lifetime of the window, so the user can flip
// between recent attempts and compare them. Superseded runs are frozen
// snapshots; the live run stays interactive in the panel's own state.

import type { AgentEventEnvelope } from '../../lib/api/agentEvents';

export type RunApplyState = 'idle' | 'applying' | 'applied' | 'failed';
export type RunRevertState = 'idle' | 'reverting' | 'reverted' | 'failed';

/** A frozen snapshot of one superseded single-step run. */
export type RunRecord = {
  /** Monotonic per-window id (a counter, not time/random). */
  id: string;
  prompt: string;
  /** e.g. "src/notes.ts" or "src/notes.ts:2-3"; null when nothing attached. */
  attachmentLabel: string | null;
  events: AgentEventEnvelope[];
  /** The validated diff this run produced, or null (invalid / no diff). */
  applicableDiff: string | null;
  applyState: RunApplyState;
  revertState: RunRevertState;
  checkpoint: string | null;
};

/** Newest-first cap. Deliberately small — this is a "compare recent attempts"
 *  affordance, not a full session log. */
export const MAX_RUNS = 5;

/** A run's outcome status, for the list chip. Priority: a revert shadows an
 *  apply, which shadows the diff/validation state. */
export function runStatusLabel(
  run: {
    events: readonly unknown[];
    applicableDiff: string | null;
    applyState: RunApplyState;
    revertState: RunRevertState;
  },
): string {
  if (run.revertState === 'reverted') return 'reverted';
  if (run.applyState === 'applied') return 'applied';
  if (run.applyState === 'failed') return 'apply failed';
  if (run.applicableDiff) return 'diff ready';
  if (run.events.length > 0) return 'no diff';
  return '—';
}

/** Read-only one-liner under a past run's diff, summarizing what became of it.
 *  A historical run is never re-appliable from the list, so the note explains
 *  the frozen outcome and points back to the live run for new edits. */
export function historicalRunNote(run: RunRecord): string {
  if (run.revertState === 'reverted') return 'Applied, then reverted to the pre-apply state.';
  if (run.applyState === 'applied') {
    return run.checkpoint
      ? `Applied · checkpoint ${run.checkpoint.slice(0, 8)}. Read-only here.`
      : 'Applied. Read-only here.';
  }
  if (run.applyState === 'failed') return 'Apply failed — nothing changed on disk.';
  return 'Not applied. Switch to the live run to apply a diff.';
}

/** Stable label for an attachment chip — path, plus a line range when set. */
export function attachmentLabelOf(
  chip: { relPath: string; lineRange: { startLine: number; endLine: number } | null } | null,
): string | null {
  if (!chip) return null;
  return chip.lineRange
    ? `${chip.relPath}:${chip.lineRange.startLine}-${chip.lineRange.endLine}`
    : chip.relPath;
}

/** Trim a prompt for the compact run chip without dropping meaning entirely. */
export function truncatePrompt(prompt: string, max = 32): string {
  const clean = prompt.trim();
  if (clean.length <= max) return clean;
  return `${clean.slice(0, max - 1)}…`;
}
