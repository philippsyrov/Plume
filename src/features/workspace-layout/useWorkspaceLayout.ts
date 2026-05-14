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
/// CodeMirror line numbers + a readable column.
///
/// The static max constants are absolute upper bounds — beyond
/// these, a side panel is bigger than it needs to be regardless of
/// available room. The EFFECTIVE max each side accepts at drag
/// time is `dynamicMaxFor`, which subtracts the other side's
/// current width + handles + the reserved center minimum from the
/// live viewport. That's how D30 keeps both sides at their static
/// maxes from collapsing the center on any sane window size.
const MIN_LEFT_WIDTH = 200;
const MAX_LEFT_WIDTH = 480;
const MIN_RIGHT_WIDTH = 260;
const MAX_RIGHT_WIDTH = 640;

/// The center column never shrinks below this. Picked to match the
/// "useful gutter on a 900 px window" intent in the spec: at the
/// 900 px Tauri minimum with both sides at min (200 + 260 = 460),
/// handles (16), and shell padding (48), the center has 376 px —
/// well past 280. The reservation matters at default widths
/// (260 + 340) where one user dragging a panel wider must not
/// drop the center below this floor.
const CENTER_MIN_WIDTH = 280;

/// Width of a single resize handle's grid track. Mirrors the
/// `.plume-resize-handle { width: 8px }` rule in `resize.css`. If
/// the CSS changes, this must change with it — the constant is the
/// JS side of the layout math.
const HANDLE_WIDTH = 8;

/// Horizontal padding `.plume-shell` reserves outside the workspace.
/// Mirrors `padding: var(--space-5)` (24 px) on each side. The
/// workspace itself adds no padding, so this is the entire chrome
/// the viewport loses before the workspace sees its width.
const SHELL_PADDING_X = 48;

/// Best-effort SSR fallback for the viewport width — used only on
/// the initial render before the resize listener has fired.
const SSR_VIEWPORT = 1200;

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
  /// Static absolute min for each side — applied unconditionally
  /// regardless of viewport.
  readonly LEFT_MIN: number;
  readonly RIGHT_MIN: number;
  /// Dynamic max for each side, computed against the live viewport
  /// AND the other side's current width. The ResizeHandle takes
  /// these as its `max` prop so drag is clamped before the center
  /// can be squeezed past `CENTER_MIN_WIDTH`. Re-derived on every
  /// render — pass these (not the static MAX_*) to the handle.
  readonly leftMax: number;
  readonly rightMax: number;
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

/// Room available to BOTH side panels combined, given the live
/// viewport and which sides are visible. Subtracts the shell's
/// horizontal padding, the center-minimum reservation, and one
/// handle per visible side. Floors at 0 — the caller's math has to
/// cope with a too-narrow window (very small windows just clip,
/// they don't crash).
function availableSideWidth(
  leftVisible: boolean,
  rightVisible: boolean,
  viewport: number,
): number {
  const handles = (leftVisible ? HANDLE_WIDTH : 0) + (rightVisible ? HANDLE_WIDTH : 0);
  return Math.max(0, viewport - SHELL_PADDING_X - CENTER_MIN_WIDTH - handles);
}

/// Effective drag-time max for one side. Subtracts the OTHER side's
/// current width from the combined available room and clamps to the
/// static absolute max. The static min is the floor: even when the
/// dynamic math says "no room," each side stays at least at its
/// static min (the center then shrinks past `CENTER_MIN_WIDTH` —
/// the user sees an honest pinch, not invisible content).
function dynamicMaxFor(
  side: 'left' | 'right',
  state: Persisted,
  viewport: number,
): number {
  const room = availableSideWidth(state.leftVisible, state.rightVisible, viewport);
  const otherActive =
    side === 'left'
      ? state.rightVisible
        ? state.rightWidth
        : 0
      : state.leftVisible
        ? state.leftWidth
        : 0;
  const remaining = room - otherActive;
  const absoluteMax = side === 'left' ? MAX_LEFT_WIDTH : MAX_RIGHT_WIDTH;
  const absoluteMin = side === 'left' ? MIN_LEFT_WIDTH : MIN_RIGHT_WIDTH;
  return Math.max(absoluteMin, Math.min(absoluteMax, remaining));
}

/// If the current state oversubscribes the available room (typical
/// causes: viewport shrank, or a saved value from a wider window
/// rehydrated), scale both sides down proportionally until the
/// total fits. Each side still respects its static minimum, so an
/// extreme shrink can leave a residual over-spill — the center is
/// then squeezed below `CENTER_MIN_WIDTH`, which is the right
/// failure mode (visible cramping, not a layout glitch). Returns
/// the original state untouched when nothing needs to change so
/// React's identity check skips the render.
function rebalanceForViewport(state: Persisted, viewport: number): Persisted {
  const available = availableSideWidth(state.leftVisible, state.rightVisible, viewport);
  const leftActive = state.leftVisible ? state.leftWidth : 0;
  const rightActive = state.rightVisible ? state.rightWidth : 0;
  const total = leftActive + rightActive;
  if (total <= available) return state;
  const scale = total === 0 ? 1 : available / total;
  const newLeft = state.leftVisible
    ? Math.max(MIN_LEFT_WIDTH, Math.round(state.leftWidth * scale))
    : state.leftWidth;
  const newRight = state.rightVisible
    ? Math.max(MIN_RIGHT_WIDTH, Math.round(state.rightWidth * scale))
    : state.rightWidth;
  if (newLeft === state.leftWidth && newRight === state.rightWidth) return state;
  return { ...state, leftWidth: newLeft, rightWidth: newRight };
}

function readViewportWidth(): number {
  if (typeof window === 'undefined') return SSR_VIEWPORT;
  return window.innerWidth;
}

/// Hook returns the current layout + setters. The keydown listener
/// is registered once and routes Cmd+Shift+[ / Cmd+Shift+] to the
/// toggle handlers — `event.code` so the binding works on keyboard
/// layouts where `[` requires a dead-key combination.
export function useWorkspaceLayout(): WorkspaceLayout {
  const [state, setState] = useState<Persisted>(() => load());
  // Viewport width drives the dynamic-max math. Initialised
  // synchronously from `window.innerWidth` so the first paint
  // already has a real value; the resize listener keeps it fresh.
  const [viewportWidth, setViewportWidth] = useState<number>(() => readViewportWidth());

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

  // Track viewport width. Rebalance runs in a separate effect so
  // the listener stays cheap (just a number setter); the rebalance
  // observes viewportWidth and current state together.
  useEffect(() => {
    if (typeof window === 'undefined') return;
    const onResize = () => setViewportWidth(window.innerWidth);
    window.addEventListener('resize', onResize);
    return () => window.removeEventListener('resize', onResize);
  }, []);

  // If the viewport (or visibility) changed in a way that
  // oversubscribes the available room, scale both sides
  // proportionally until they fit. The center-min reservation is
  // honoured before the static maxes — Codex's D30 review caught
  // the case where 480 + 640 > 900 - shell padding, which would
  // either overflow horizontally or collapse the center to a few
  // dozen pixels.
  useEffect(() => {
    setState((prev) => rebalanceForViewport(prev, viewportWidth));
  }, [viewportWidth]);

  const setLeftWidth = useCallback((next: number) => {
    setState((prev) => {
      const dynMax = dynamicMaxFor('left', prev, viewportWidth);
      const clamped = Math.max(MIN_LEFT_WIDTH, Math.min(dynMax, Math.round(next)));
      if (clamped === prev.leftWidth) return prev;
      return { ...prev, leftWidth: clamped };
    });
  }, [viewportWidth]);

  const setRightWidth = useCallback((next: number) => {
    setState((prev) => {
      const dynMax = dynamicMaxFor('right', prev, viewportWidth);
      const clamped = Math.max(MIN_RIGHT_WIDTH, Math.min(dynMax, Math.round(next)));
      if (clamped === prev.rightWidth) return prev;
      return { ...prev, rightWidth: clamped };
    });
  }, [viewportWidth]);

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
    RIGHT_MIN: MIN_RIGHT_WIDTH,
    leftMax: dynamicMaxFor('left', state, viewportWidth),
    rightMax: dynamicMaxFor('right', state, viewportWidth),
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
