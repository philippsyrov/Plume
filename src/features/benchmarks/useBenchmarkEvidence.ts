// D132: one-shot load of the project's benchmark evidence with a
// manual Refresh. No polling — benchmark runs happen in a terminal,
// not inside the app, so "refresh after a run" is a deliberate user
// action, and a background fs walk every few seconds would be noise.

import { useCallback, useEffect, useRef, useState } from 'react';

import { ipcErrorMessage, isIpcError } from '../../lib/api/errors';
import { loadBenchmarkEvidence, type BenchmarkEvidence } from './data';

export type BenchmarkEvidenceState =
  | { kind: 'loading' }
  | { kind: 'ready'; evidence: BenchmarkEvidence }
  | { kind: 'error'; message: string };

export function useBenchmarkEvidence(): {
  state: BenchmarkEvidenceState;
  refresh: () => void;
} {
  const [state, setState] = useState<BenchmarkEvidenceState>({ kind: 'loading' });
  // Generation counter: a stale load resolving after a refresh (or
  // after unmount) must not overwrite the newer state.
  const generationRef = useRef(0);

  const load = useCallback(() => {
    const gen = ++generationRef.current;
    setState({ kind: 'loading' });
    loadBenchmarkEvidence()
      .then((evidence) => {
        if (gen !== generationRef.current) return;
        setState({ kind: 'ready', evidence });
      })
      .catch((err: unknown) => {
        if (gen !== generationRef.current) return;
        setState({ kind: 'error', message: formatError(err) });
      });
  }, []);

  useEffect(() => {
    load();
    return () => {
      // Invalidate in-flight loads on unmount.
      generationRef.current += 1;
    };
  }, [load]);

  return { state, refresh: load };
}

function formatError(err: unknown): string {
  if (isIpcError(err)) return ipcErrorMessage(err);
  if (err instanceof Error) return err.message;
  return 'Failed to load benchmark evidence.';
}
