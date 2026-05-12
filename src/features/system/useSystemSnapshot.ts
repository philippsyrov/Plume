// Slow-poll hook around the `system.snapshot` IPC verb. Cadence is
// deliberately conservative — D5's spec calls for 5–10 s — because
// the data only needs to feel "live enough" for the status strip,
// not to drive any decision the user is racing against.
//
// Visibility-gated: when the window is hidden (Cmd-Tab / minimised)
// we skip ticks entirely. macOS WebKit fires `visibilitychange`
// reliably; the gate keeps a backgrounded Plume from spawning
// vm_stat every 7 s while the user is doing other things.

import { useEffect, useRef, useState } from 'react';

import { getSystemSnapshot, type MachineSnapshot } from '../../lib/api/system';
import { ipcErrorMessage, isIpcError } from '../../lib/api/errors';

const DEFAULT_INTERVAL_MS = 7_000;

export type SystemSnapshotState =
  | { kind: 'loading' }
  | { kind: 'ready'; snapshot: MachineSnapshot; lastErrorMessage: string | null }
  | { kind: 'error'; message: string };

export function useSystemSnapshot(intervalMs: number = DEFAULT_INTERVAL_MS): SystemSnapshotState {
  const [state, setState] = useState<SystemSnapshotState>({ kind: 'loading' });
  // Generation counter rolls forward whenever a fetch starts; older
  // resolves no-op so a slow probe can't overwrite a newer one.
  const generationRef = useRef(0);

  useEffect(() => {
    let cancelled = false;
    let timer: ReturnType<typeof setTimeout> | undefined;

    // Single source of truth for queuing the next tick. Every path
    // that wants to schedule (the initial fire, the hidden-skip
    // branch, the post-fetch finally, the visibility handler) goes
    // through here so we always cancel the previous timer first.
    // Codex found a bug where `onVisible` fired `tick` immediately
    // without clearing the still-queued hidden-skip timer, leaving
    // two loops alive after one hide/show cycle.
    const scheduleNext = (delay: number) => {
      if (timer !== undefined) clearTimeout(timer);
      timer = setTimeout(tick, delay);
    };

    const tick = async () => {
      if (cancelled) return;
      if (typeof document !== 'undefined' && document.hidden) {
        // Skip while hidden; re-fire on visibility restore below.
        scheduleNext(intervalMs);
        return;
      }
      const gen = ++generationRef.current;
      try {
        const snapshot = await getSystemSnapshot();
        if (cancelled || gen !== generationRef.current) return;
        setState({ kind: 'ready', snapshot, lastErrorMessage: null });
      } catch (err) {
        if (cancelled || gen !== generationRef.current) return;
        const message = formatError(err);
        // Once we've had at least one successful read, keep showing
        // the last good snapshot and tuck the error message into a
        // sidecar field. A transient sysctl hiccup shouldn't make
        // the chips disappear.
        setState((prev) =>
          prev.kind === 'ready'
            ? { kind: 'ready', snapshot: prev.snapshot, lastErrorMessage: message }
            : { kind: 'error', message },
        );
      } finally {
        if (!cancelled) {
          scheduleNext(intervalMs);
        }
      }
    };

    const onVisible = () => {
      if (cancelled || typeof document === 'undefined' || document.hidden) {
        return;
      }
      // Bring the strip back up to date right after the window
      // comes back; don't wait for the next interval edge. Cancel
      // whatever the hidden-skip branch queued so we don't end up
      // with two parallel loops — `tick`'s finally will queue the
      // next interval itself.
      if (timer !== undefined) {
        clearTimeout(timer);
        timer = undefined;
      }
      void tick();
    };

    void tick();
    if (typeof document !== 'undefined') {
      document.addEventListener('visibilitychange', onVisible);
    }

    return () => {
      cancelled = true;
      if (timer !== undefined) {
        clearTimeout(timer);
        timer = undefined;
      }
      if (typeof document !== 'undefined') {
        document.removeEventListener('visibilitychange', onVisible);
      }
    };
  }, [intervalMs]);

  return state;
}

function formatError(err: unknown): string {
  if (isIpcError(err)) return ipcErrorMessage(err);
  if (err instanceof Error) return err.message;
  return 'Failed to read host status.';
}
