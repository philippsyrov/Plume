// D30: trusted-project workspace shell — column widths + visibility.
//
// The trusted view is a three-column grid (left navigation, center
// agent workspace, right inspector). Before D30 the widths were
// pinned at 260 / 1fr / 340 px; D30 makes the side widths user-
// adjustable via drag handles, exposes show/hide controls, and
// persists the four-field shape to `localStorage` so the layout
// survives a reload.
//
// The hook deliberately owns ALL of the layout state in one place
// (vs. one piece in CSS vars, one piece in React state, one piece
// in DOM dataset attributes). A future drag-anywhere panel system
// (D32 in the roadmap) will replace this hook with a richer layout
// tree — keeping the current state self-contained makes that swap
// cheap.

import { useCallback, useEffect, useState } from 'react';

/// Schema version baked into the storage key. Bump alongside any
/// non-additive change to `Persisted` so an older Plume can't
/// hydrate a shape its parser doesn't understand.
const STORAGE_KEY = 'plume:workspace-layout-v1';

const DEFAULT_LEFT_WIDTH = 260;
const DEFAULT_RIGHT_WIDTH = 340;

/// Min widths are tuned so each side panel stays useful at its
/// minimum: 200 px holds the provider rows + the file-tree depth-1
/// labels without truncating; 260 px keeps the inspector's
/// CodeMirror line numbers + a readable column. Max widths cap the
/// drag so a user can't accidentally swallow the entire window —
/// the center column always retains at least ~280 px on a 900 px
/// window (the configured Tauri minimum).
const MIN_LEFT_WIDTH = 200;
const MAX_LEFT_WIDTH = 480;
const MIN_RIGHT_WIDTH = 260;
const MAX_RIGHT_WIDTH = 640;

type Persisted = {
  leftWidth: number;
  rightWidth: number;
  leftVisible: boolean;
  rightVisible: boolean;
};

export type WorkspaceLayout = Persisted & {
  setLeftWidth: (next: number) => void;
  setRightWidth: (next: number) => void;
  toggleLeft: () => void;
  toggleRight: () => void;
  /// Exposed so the resize handles can read the same clamps the
  /// hook applies. Static constants today; if a future slice lets
  /// the user reconfigure them, these become live values.
  readonly LEFT_MIN: number;
  readonly LEFT_MAX: number;
  readonly RIGHT_MIN: number;
  readonly RIGHT_MAX: number;
};

function defaults(): Persisted {
  return {
    leftWidth: DEFAULT_LEFT_WIDTH,
    rightWidth: DEFAULT_RIGHT_WIDTH,
    leftVisible: true,
    rightVisible: true,
  };
}

function clampLeft(value: number): number {
  if (!Number.isFinite(value)) return DEFAULT_LEFT_WIDTH;
  return Math.max(MIN_LEFT_WIDTH, Math.min(MAX_LEFT_WIDTH, Math.round(value)));
}

function clampRight(value: number): number {
  if (!Number.isFinite(value)) return DEFAULT_RIGHT_WIDTH;
  return Math.max(MIN_RIGHT_WIDTH, Math.min(MAX_RIGHT_WIDTH, Math.round(value)));
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
    return {
      leftWidth: clampLeft(
        typeof cast.leftWidth === 'number' ? cast.leftWidth : DEFAULT_LEFT_WIDTH,
      ),
      rightWidth: clampRight(
        typeof cast.rightWidth === 'number' ? cast.rightWidth : DEFAULT_RIGHT_WIDTH,
      ),
      leftVisible: cast.leftVisible !== false,
      rightVisible: cast.rightVisible !== false,
    };
  } catch (err) {
    console.error(
      'plume workspace-layout load failed:',
      err instanceof Error ? err.message : String(err),
    );
    return defaults();
  }
}

/// Hook returns the current layout + setters. The keydown listener
/// is registered once and routes Cmd+Shift+[ / Cmd+Shift+] to the
/// toggle handlers — `event.code` so the binding works on keyboard
/// layouts where `[` requires a dead-key combination.
export function useWorkspaceLayout(): WorkspaceLayout {
  const [state, setState] = useState<Persisted>(() => load());

  useEffect(() => {
    if (typeof window === 'undefined') return;
    try {
      window.localStorage.setItem(STORAGE_KEY, JSON.stringify(state));
    } catch (err) {
      console.error(
        'plume workspace-layout persist failed:',
        err instanceof Error ? err.message : String(err),
      );
    }
  }, [state]);

  const setLeftWidth = useCallback((next: number) => {
    setState((prev) => {
      const clamped = clampLeft(next);
      if (clamped === prev.leftWidth) return prev;
      return { ...prev, leftWidth: clamped };
    });
  }, []);

  const setRightWidth = useCallback((next: number) => {
    setState((prev) => {
      const clamped = clampRight(next);
      if (clamped === prev.rightWidth) return prev;
      return { ...prev, rightWidth: clamped };
    });
  }, []);

  const toggleLeft = useCallback(() => {
    setState((prev) => ({ ...prev, leftVisible: !prev.leftVisible }));
  }, []);

  const toggleRight = useCallback(() => {
    setState((prev) => ({ ...prev, rightVisible: !prev.rightVisible }));
  }, []);

  useEffect(() => {
    if (typeof window === 'undefined') return;
    const onKey = (event: KeyboardEvent) => {
      if (!event.metaKey || !event.shiftKey) return;
      if (event.code === 'BracketLeft') {
        event.preventDefault();
        toggleLeft();
      } else if (event.code === 'BracketRight') {
        event.preventDefault();
        toggleRight();
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [toggleLeft, toggleRight]);

  return {
    ...state,
    setLeftWidth,
    setRightWidth,
    toggleLeft,
    toggleRight,
    LEFT_MIN: MIN_LEFT_WIDTH,
    LEFT_MAX: MAX_LEFT_WIDTH,
    RIGHT_MIN: MIN_RIGHT_WIDTH,
    RIGHT_MAX: MAX_RIGHT_WIDTH,
  };
}

/// Helper used by `App.tsx` to compute the `grid-template-columns`
/// string for `.plume-workspace`. Exported so the same rule can be
/// unit-tested or reused if the shell grows a fourth column.
export function workspaceGridTemplate(layout: WorkspaceLayout): string {
  const parts: string[] = [];
  if (layout.leftVisible) {
    parts.push(`${layout.leftWidth}px`);
    parts.push('auto'); // resize handle between left and center
  }
  parts.push('minmax(0, 1fr)');
  if (layout.rightVisible) {
    parts.push('auto'); // resize handle between center and right
    parts.push(`${layout.rightWidth}px`);
  }
  return parts.join(' ');
}
