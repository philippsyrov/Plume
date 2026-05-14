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
  load: () => Promise<void>;
};

export function useProviderInventory(): ProviderInventory {
  const [state, setState] = useState<ProviderInventoryState>({ kind: 'loading' });
  const [refreshing, setRefreshing] = useState(false);
  // Generation counter bumped on every `load()`. In-flight calls
  // capture the value at the moment they start and silently drop
  // their result if the generation has moved on. Same race guard
  // as the pre-D32 implementation.
  const generationRef = useRef(0);

  const load = useCallback(async () => {
    const gen = ++generationRef.current;
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

  return { state, refreshing, load };
}

function formatError(err: unknown): string {
  if (isIpcError(err)) return ipcErrorMessage(err);
  if (err instanceof Error) return err.message;
  return 'Failed to load providers.';
}
