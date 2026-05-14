// D30: drag handle between two workspace columns.
//
// Each handle sits in its own grid track (`auto` width) between the
// columns it resizes. Three input modes:
//
//   - Mouse drag (primary). Mousedown captures the anchor (clientX
//     + current width), mousemove computes the delta and clamps,
//     mouseup releases. The `document.body.style.cursor` override
//     keeps the col-resize cursor visible even when the pointer
//     drifts over a child element with its own cursor rule.
//
//   - Keyboard nudge. Arrow Left / Arrow Right move the handle in
//     `step` increments (8 px default, 32 px with Shift) so a
//     keyboard-only user can still tune the layout. The element
//     advertises itself as `role="separator"` with the proper
//     `aria-orientation` and `aria-valuenow` so a screen reader
//     reads the current width.
//
//   - Future: touch (post-D30). The structure already routes
//     through React event handlers, so adding `onTouchStart` is a
//     small extension once we know what hardware Plume runs on.
//
// The `edge` prop names which column this handle resizes — left
// or right. Dragging right grows the LEFT column but SHRINKS the
// right column, so the math depends on which side we're on.

import { useCallback, useEffect, useRef } from 'react';

const KEYBOARD_STEP_NORMAL = 8;
const KEYBOARD_STEP_LARGE = 32;

export type ResizeHandleProps = {
  /// Which column this handle controls. `'left'` sits between the
  /// left and center columns; `'right'` sits between center and
  /// right.
  edge: 'left' | 'right';
  /// Current width (in px) of the column being resized.
  current: number;
  min: number;
  max: number;
  onChange: (next: number) => void;
  ariaLabel: string;
};

export function ResizeHandle({
  edge,
  current,
  min,
  max,
  onChange,
  ariaLabel,
}: ResizeHandleProps) {
  // Anchor captured at drag-start. Reading `current` inside the
  // mousemove listener would race against the React state updates
  // the listener itself triggers — so we anchor once at mousedown
  // and compute deltas against the anchor, not the latest state.
  const dragAnchor = useRef<{ startX: number; startWidth: number } | null>(null);
  // Stash the latest props in a ref so the window-level listener
  // (registered once at mousedown) always reads fresh clamps + the
  // current `edge` without re-registering on every render.
  const propsRef = useRef({ edge, min, max, onChange });
  useEffect(() => {
    propsRef.current = { edge, min, max, onChange };
  }, [edge, min, max, onChange]);

  const onMouseDown = useCallback(
    (event: React.MouseEvent<HTMLDivElement>) => {
      if (event.button !== 0) return;
      event.preventDefault();
      dragAnchor.current = { startX: event.clientX, startWidth: current };

      const onMove = (move: MouseEvent) => {
        const anchor = dragAnchor.current;
        if (!anchor) return;
        const { edge: currentEdge, min: currentMin, max: currentMax, onChange: emit } =
          propsRef.current;
        const delta = move.clientX - anchor.startX;
        const raw =
          currentEdge === 'left' ? anchor.startWidth + delta : anchor.startWidth - delta;
        const clamped = Math.max(currentMin, Math.min(currentMax, Math.round(raw)));
        emit(clamped);
      };

      const onUp = () => {
        dragAnchor.current = null;
        window.removeEventListener('mousemove', onMove);
        window.removeEventListener('mouseup', onUp);
        // Restore the page cursor — set on mousedown so the
        // col-resize cursor survives drifting over child elements
        // with their own cursor rules.
        document.body.style.cursor = '';
      };

      window.addEventListener('mousemove', onMove);
      window.addEventListener('mouseup', onUp);
      document.body.style.cursor = 'col-resize';
    },
    [current],
  );

  const onKeyDown = useCallback(
    (event: React.KeyboardEvent<HTMLDivElement>) => {
      const step = event.shiftKey ? KEYBOARD_STEP_LARGE : KEYBOARD_STEP_NORMAL;
      let next = current;
      if (event.code === 'ArrowLeft') {
        next = edge === 'left' ? current - step : current + step;
      } else if (event.code === 'ArrowRight') {
        next = edge === 'left' ? current + step : current - step;
      } else if (event.code === 'Home') {
        next = min;
      } else if (event.code === 'End') {
        next = max;
      } else {
        return;
      }
      event.preventDefault();
      const clamped = Math.max(min, Math.min(max, next));
      onChange(clamped);
    },
    [current, edge, max, min, onChange],
  );

  return (
    <div
      className={`plume-resize-handle plume-resize-handle-${edge}`}
      role="separator"
      tabIndex={0}
      aria-orientation="vertical"
      aria-label={ariaLabel}
      aria-valuemin={min}
      aria-valuemax={max}
      aria-valuenow={current}
      onMouseDown={onMouseDown}
      onKeyDown={onKeyDown}
    />
  );
}
