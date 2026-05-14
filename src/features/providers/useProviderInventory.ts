// D32: shared provider-inventory loader.
//
// Before D32 the load logic (providers + reachability + local
// models) lived inside `ProviderPanel`, which packaged both
// surfaces into one ink-panel. D32 splits the visual surface
// into two independently-toggleable panels (Providers, Local
// models). To keep them at one IPC fetch (no double-load when
// both are visible, no fresh fetch when one is briefly hidden
// then re-shown), the load state lives in this hook, called
// once at the trusted-view level and passed to whichever
// panels are currently mounted.
//
// `LoadState` is the same discriminated shape the panels read.
// The hook also exposes:
//   - `load()` — manual refresh; the Providers panel's Refresh
//     button binds to this.
//   - `refreshing` — true while an in-flight load is running,
//     for the button's disabled state.
//
// D29's fail-soft contract is preserved: a local-model scan
// rejection surfaces as `localModelError` in the ready state
// while providers + health stay authoritative. Generation
// tracking is unchanged from the pre-D32 implementation —
// stale fetches drop their result so a rapid refresh sequence
// can't race itself.
//
// `revision` is exposed so consumers that hold their own
// per-load derived state (e.g. `ProvidersPanel`'s `details` and
// `expanded` per-model caches) can a) clear that state when a
// new inventory lands, and b) gate their own in-flight side
// fetches so a stale `providers.modelDetails` resolve cannot
// overwrite a freshly-cleared cache. Pre-D32 the same component
// owned both the load state and the per-model caches, so a
// single `generationRef` covered both halves; the D32 split
// needs the revision number on the wire so the panel can stay
// in sync.

import { useCallback, useEffect, useRef, useState } from 'react';

import { ipcErrorMessage, isIpcError } from '../../lib/api/errors';
import {
  getLocalModels,
  getProvidersHealth,
  listProviders,
  type LocalModel,
  type ProviderHealth,
  type ProviderInfo,
} from '../../lib/api/providers';

export type ProviderInventoryState =
  | { kind: 'loading' }
  | {
      kind: 'ready';
      providers: ProviderInfo[];
      healthById: Map<string, ProviderHealth>;
      localModels: LocalModel[];
      /// Failure message from the local-models scan. See D29
      /// fail-soft notes — `null` means the scan succeeded; a
      /// string means scanning rejected and the Local models
      /// panel renders that error inline instead of taking down
      /// the rest of the inventory.
      localModelError: string | null;
    }
  | { kind: 'error'; message: string };

export type ProviderInventory = {
  state: ProviderInventoryState;
  refreshing: boolean;
  /// Monotonically increasing counter. Bumped at the START of
  /// every `load()` so consumers can reset per-load derived
  /// state and gate their own in-flight side fetches. See the
  /// module-level note for the D32 split rationale.
  revision: number;
  load: () => Promise<void>;
};

export function useProviderInventory(): ProviderInventory {
  const [state, setState] = useState<ProviderInventoryState>({ kind: 'loading' });
  const [refreshing, setRefreshing] = useState(false);
  // Observable revision counter. Bumped at the START of every
  // load — synchronous with the load kicking off — so a child's
  // `useEffect(..., [revision])` clears its per-load caches at
  // the same moment the inventory begins re-fetching, not after.
  // The ref tracks the live value for side fetches that resolve
  // after a later load has already started.
  const [revision, setRevision] = useState(0);
  const revisionRef = useRef(0);
  // Generation counter bumped on every `load()`. In-flight calls
  // capture the value at the moment they start and silently drop
  // their result if the generation has moved on. Same race guard
  // as the pre-D32 implementation; kept separate from `revision`
  // because the inventory's internal race guard does NOT need
  // to be exposed and an extra render-triggering useState here
  // would be wasted.
  const generationRef = useRef(0);

  const load = useCallback(async () => {
    const gen = ++generationRef.current;
    revisionRef.current += 1;
    setRevision(revisionRef.current);
    setRefreshing(true);
    try {
      // Critical pair: provider registry + reachability snapshot.
      // These two define the panel's main content. If either
      // fails we surface the panel-wide error state.
      const [providers, health] = await Promise.all([
        listProviders(),
        getProvidersHealth(),
      ]);
      if (gen !== generationRef.current) return;

      // D29: the local-models scan is a secondary surface. A
      // failure here must NOT take down providers + health. Run
      // it after the critical pair resolves and let an error
      // fall through as an inline message in the Local models
      // panel.
      let localModels: LocalModel[] = [];
      let localModelError: string | null = null;
      try {
        localModels = await getLocalModels();
      } catch (err) {
        localModelError = formatError(err);
      }
      if (gen !== generationRef.current) return;

      const healthById = new Map(health.map((h) => [h.id, h]));
      setState({
        kind: 'ready',
        providers,
        healthById,
        localModels,
        localModelError,
      });
    } catch (err) {
      if (gen !== generationRef.current) return;
      setState({ kind: 'error', message: formatError(err) });
    } finally {
      if (gen === generationRef.current) {
        setRefreshing(false);
      }
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  return { state, refreshing, revision, load };
}

function formatError(err: unknown): string {
  if (isIpcError(err)) return ipcErrorMessage(err);
  if (err instanceof Error) return err.message;
  return 'Failed to load providers.';
}
