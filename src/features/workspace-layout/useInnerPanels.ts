// D32: per-column inner-panel visibility for the trusted-project
// workspace shell.
//
// D30 made the OUTER columns (left navigation, right inspector)
// independently show/hide-able. D32 layers ONE level deeper:
// within each visible column, the user can now toggle individual
// panels.
//
//   left column → Files, Providers, Local models
//   right column → Inspector
//
// The state is intentionally a flat record of booleans rather than
// a nested structure. A flat shape is easy to migrate when a new
// inner panel arrives (D33+ Diff / Preview slots), and it keeps
// the localStorage payload trivially diffable from devtools.
//
// Both columns persist together at `plume:inner-panels-v1`. The
// hook is deliberately NOT a kitchen-sink merge with
// `useWorkspaceLayout` — the outer column shape (widths, outer
// visibility, the resize handles) is a different concern with
// different invariants. A future drag-anywhere layout system will
// likely replace BOTH hooks at once, so keeping them separate but
// peer-shaped (`load`, `defaults`, single storage key) makes that
// swap predictable.
//
// Show/hide here is purely cosmetic: the underlying React subtree
// is unmounted when a panel is hidden. That's intentional — keeps
// IPC load light when a user has decided they don't want a panel,
// and makes "all panels hidden" a real empty state. The shared
// `useProviderInventory` hook lives in App-level state so a hidden
// Providers panel doesn't double-fetch when re-shown moments later.

import { useCallback, useEffect, useState } from 'react';

/// Schema version baked into the storage key, same convention as
/// `plume:workspace-layout-v1`. Bump alongside any non-additive
/// change so an older Plume can't hydrate a shape its parser
/// doesn't understand.
const STORAGE_KEY = 'plume:inner-panels-v1';

type Persisted = {
  files: boolean;
  providers: boolean;
  localModels: boolean;
  inspector: boolean;
};

export type InnerPanels = Persisted & {
  toggleFiles: () => void;
  toggleProviders: () => void;
  toggleLocalModels: () => void;
  toggleInspector: () => void;
  /// True iff at least one LEFT-column panel is currently visible.
  /// Used by the shell to decide whether to render the column's
  /// content area or the "no panels visible" empty state.
  readonly leftAnyVisible: boolean;
  /// Same idea for the right column. Today there's only one panel,
  /// so this is equivalent to `inspector`; the field is exposed
  /// anyway so callers don't have to know about the 1-vs-N
  /// asymmetry.
  readonly rightAnyVisible: boolean;
};

function defaults(): Persisted {
  // Everything visible on first run — the panel toggles are an
  // opt-out, not an opt-in. A new user shouldn't have to discover
  // the chips before they see their files.
  return {
    files: true,
    providers: true,
    localModels: true,
    inspector: true,
  };
}

function load(): Persisted {
  if (typeof window === 'undefined') {
    return defaults();
  }
  try {
    const raw = window.localStorage.getItem(STORAGE_KEY);
    if (!raw) return defaults();
    const parsed = JSON.parse(raw) as unknown;
    if (!parsed || typeof parsed !== 'object') return defaults();
    const cast = parsed as Record<string, unknown>;
    // Permissive on unknown / non-boolean fields — anything other
    // than an explicit `false` resolves to true. This means a
    // future Plume that learns a new panel name can render it
    // visible by default for users who hydrated state under an
    // older version, without a migration step.
    return {
      files: cast.files !== false,
      providers: cast.providers !== false,
      localModels: cast.localModels !== false,
      inspector: cast.inspector !== false,
    };
  } catch (err) {
    console.error(
      'plume inner-panels load failed:',
      err instanceof Error ? err.message : String(err),
    );
    return defaults();
  }
}

export function useInnerPanels(): InnerPanels {
  const [state, setState] = useState<Persisted>(() => load());

  useEffect(() => {
    if (typeof window === 'undefined') return;
    try {
      window.localStorage.setItem(STORAGE_KEY, JSON.stringify(state));
    } catch (err) {
      console.error(
        'plume inner-panels persist failed:',
        err instanceof Error ? err.message : String(err),
      );
    }
  }, [state]);

  const toggleFiles = useCallback(() => {
    setState((prev) => ({ ...prev, files: !prev.files }));
  }, []);
  const toggleProviders = useCallback(() => {
    setState((prev) => ({ ...prev, providers: !prev.providers }));
  }, []);
  const toggleLocalModels = useCallback(() => {
    setState((prev) => ({ ...prev, localModels: !prev.localModels }));
  }, []);
  const toggleInspector = useCallback(() => {
    setState((prev) => ({ ...prev, inspector: !prev.inspector }));
  }, []);

  const leftAnyVisible = state.files || state.providers || state.localModels;
  const rightAnyVisible = state.inspector;

  return {
    ...state,
    toggleFiles,
    toggleProviders,
    toggleLocalModels,
    toggleInspector,
    leftAnyVisible,
    rightAnyVisible,
  };
}
