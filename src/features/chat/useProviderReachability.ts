// D14: provider reachability for the chat panel.
//
// `ProviderPanel` already fetches `providers.health` to render the
// reachability badge on each provider row. The chat panel needs the
// SAME data for the selected model's provider so it can warn the
// user up-front when the daemon isn't running, rather than letting
// them type a long prompt and only learn after Send that "could not
// reach ollama" comes back from the transport.
//
// Why a separate hook instead of lifting `ProviderPanel`'s state:
//   * The provider panel's fetch is interleaved with model details,
//     refresh state, and selection bookkeeping — pulling it up into
//     a context would force the chat panel to consume more than it
//     needs.
//   * `providers.health` is a cheap localhost TCP probe + small
//     HTTP GET; an extra fetch from the chat panel on mount is a
//     few-millisecond cost.
//   * The hook can be reused by future surfaces (model picker, the
//     selected-model banner, the agent-loop status strip) without
//     re-plumbing.
//
// Refresh policy: probe once on mount and whenever `providerId`
// changes (e.g. user switches from an Ollama model to an LM Studio
// model). Manual `refresh()` is exposed so the chat panel can offer
// a "Recheck" button — useful when the user has just started the
// daemon outside Plume and wants to flip the panel from "not
// running" to "Ready" without remounting the project. No polling:
// reachability rarely changes mid-session and a per-second probe
// would just burn cycles for a typically-idle chat panel.

import { useCallback, useEffect, useState } from 'react';

import {
  getProvidersHealth,
  type ProviderHealth,
  type ReachabilityState,
} from '../../lib/api/providers';
import { ipcErrorMessage, isIpcError } from '../../lib/api/errors';

export type ProviderReachabilityStatus =
  /** No providerId yet (no model selected). */
  | 'idle'
  /** First fetch in flight. */
  | 'loading'
  /** Probe completed; `reachability` and `latencyMs` are populated. */
  | 'ready'
  /** Probe itself failed (the `providers.health` IPC errored). The
   *  chat panel treats this as "we don't know" rather than "down". */
  | 'error';

export type ProviderReachabilityState = {
  status: ProviderReachabilityStatus;
  /** The reachability code for `providerId` if the snapshot
   *  contained one. `null` when status is `idle` / `loading` /
   *  `error`, or when the snapshot lacked an entry for the
   *  requested provider. */
  reachability: ReachabilityState | null;
  latencyMs: number | null;
  /** Set when `status === 'error'`. */
  error: string | null;
  /** Force a refetch. Idempotent; safe to call while a probe is
   *  already in flight (a stale response will be discarded). */
  refresh: () => void;
};

const INITIAL: Omit<ProviderReachabilityState, 'refresh'> = {
  status: 'idle',
  reachability: null,
  latencyMs: null,
  error: null,
};

export function useProviderReachability(
  providerId: string | null,
): ProviderReachabilityState {
  const [state, setState] = useState<Omit<ProviderReachabilityState, 'refresh'>>(
    INITIAL,
  );
  /** Bumped on every fetch start; stale responses bail when they
   *  see a newer generation. Avoids the "user switched provider
   *  mid-probe and the old probe writes the wrong state" race. */
  const [generation, setGeneration] = useState(0);

  useEffect(() => {
    if (providerId === null) {
      setState(INITIAL);
      return;
    }
    let cancelled = false;
    setState((prev) => ({ ...prev, status: 'loading', error: null }));
    getProvidersHealth()
      .then((list: ProviderHealth[]) => {
        if (cancelled) return;
        const entry = list.find((h) => h.id === providerId) ?? null;
        setState({
          status: 'ready',
          reachability: entry?.state ?? null,
          latencyMs: entry?.latencyMs ?? null,
          error: null,
        });
      })
      .catch((err: unknown) => {
        if (cancelled) return;
        setState({
          status: 'error',
          reachability: null,
          latencyMs: null,
          error: formatError(err),
        });
      });
    return () => {
      cancelled = true;
    };
    // `generation` is the refetch trigger; `providerId` change
    // also re-runs this effect. The local `cancelled` closure is
    // what blocks stale responses from clobbering newer state —
    // bumping `generation` is what makes React tear down the
    // previous effect (running its cleanup, flipping `cancelled`)
    // and run a fresh one.
  }, [providerId, generation]);

  const refresh = useCallback(() => {
    setGeneration((g) => g + 1);
  }, []);

  return { ...state, refresh };
}

function formatError(err: unknown): string {
  if (isIpcError(err)) return ipcErrorMessage(err);
  if (err instanceof Error) return err.message;
  return 'Could not reach provider health.';
}
