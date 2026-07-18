// D6: window-local "currently selected model" state.
//
// No backend persistence yet. The next slice that grows real chat will
// either lift this into a typed session or pair it with a tiny IPC
// (`session.setSelectedModel` / `session.state`); for now a single
// `useState` is enough to wire the picker shell.
//
// The state is hoisted in `App.tsx` so the local and trusted-project
// shells keep one source of truth through a window transition. It remains
// intentionally window-local: the selected snapshot is not persisted into
// a chat or copied into a managed-server handle.
//
// Selectability rules (enforced at the call site, not in the hook):
//
//   * only models from providers whose reachability is `available`
//   * only model ids the runtime actually reported through its list
//     endpoint (so empty / offline providers can never produce one)
//
// Once a model is selected, the snapshot is kept even if a later health
// probe says the provider went offline or stopped serving the model.
// The workspace banner reads the current health to render an "offline"
// / "no longer reported" caveat instead of silently dropping the
// selection out from under the user.
//
// `fit` is captured opportunistically: if the user expanded the Ollama
// model row before clicking Select we already have a fit verdict in
// the provider panel's detail cache and can carry it forward. If not,
// `fit` is `undefined` and the banner just omits the badge. We do not
// fire an IPC on Select to chase a fit — D6 is a picker shell, not a
// new probe path.
//
// See `docs/IPC_ROADMAP.md § Session mode and policy` for where a
// persisted/typed version of this will live.
//
import { useCallback, useRef, useState } from 'react';

import type { FitState, ProviderId } from '../../lib/api/providers';

export type SelectedModel = {
  providerId: ProviderId;
  providerDisplayName: string;
  modelId: string;
  /** Fit verdict captured at click time if known then; undefined otherwise. */
  fit?: FitState;
};

export type SelectedModelApi = {
  selected: SelectedModel | null;
  select: (next: SelectedModel) => void;
  clear: () => void;
  /** Advances synchronously for every selection intent, including direct UI actions. */
  revision: () => number;
};

export function useSelectedModel(): SelectedModelApi {
  const [selected, setSelected] = useState<SelectedModel | null>(null);
  const revisionRef = useRef(0);
  const select = useCallback((next: SelectedModel) => {
    revisionRef.current += 1;
    setSelected(next);
  }, []);
  const clear = useCallback(() => {
    revisionRef.current += 1;
    setSelected(null);
  }, []);
  const revision = useCallback(() => revisionRef.current, []);
  return { selected, select, clear, revision };
}

/// Helper: are the provider + model ids of two selections the same?
/// `undefined` if either side is null.
export function sameSelection(
  a: SelectedModel | null,
  b: { providerId: string; modelId: string } | null,
): boolean {
  if (a === null || b === null) return false;
  return a.providerId === b.providerId && a.modelId === b.modelId;
}
