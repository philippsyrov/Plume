export type SessionMenuAnchor = Pick<DOMRect, 'left' | 'right' | 'top' | 'bottom'>;

export type SessionMenuSize = {
  width: number;
  height: number;
};

export type SessionMenuViewport = {
  width: number;
  height: number;
};

export type SessionMenuPosition = {
  left: number;
  top: number;
};

const VIEWPORT_INSET = 8;
const ANCHOR_GAP = 4;

/** Places a fixed menu by its trigger while keeping it inside the viewport. */
export function placeSessionMenu(
  anchor: SessionMenuAnchor,
  menu: SessionMenuSize,
  viewport: SessionMenuViewport,
): SessionMenuPosition {
  const maximumLeft = Math.max(VIEWPORT_INSET, viewport.width - menu.width - VIEWPORT_INSET);
  const left = Math.min(Math.max(anchor.right - menu.width, VIEWPORT_INSET), maximumLeft);
  const belowTop = anchor.bottom + ANCHOR_GAP;
  const aboveTop = anchor.top - ANCHOR_GAP - menu.height;
  const hasRoomBelow = belowTop + menu.height <= viewport.height - VIEWPORT_INSET;
  const preferredTop = hasRoomBelow ? belowTop : aboveTop;
  const maximumTop = Math.max(VIEWPORT_INSET, viewport.height - menu.height - VIEWPORT_INSET);
  const top = Math.min(Math.max(preferredTop, VIEWPORT_INSET), maximumTop);
  return { left, top };
}
