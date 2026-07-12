// D63B: one persisted-session row in the unified sidebar.
//
// Reuses the D62 `plume-project-sidebar-action-row` visual system;
// the only new chrome is the row menu (Rename / Archive / Delete)
// behind the existing "…" mini button — a small popover with stable
// accessible names, Escape-to-close, and no native dialogs.

import { useEffect, useLayoutEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';

import type { SessionSummary } from '../../lib/api/sessions';
import { placeSessionMenu, type SessionMenuPosition } from './sessionMenuPlacement';

export type SessionRowProps = {
  session: SessionSummary;
  active: boolean;
  onSelect: () => void;
  onRename: () => void;
  onArchive: () => void;
  onDelete: () => void;
};

export function SessionRow({
  session,
  active,
  onSelect,
  onRename,
  onArchive,
  onDelete,
}: SessionRowProps) {
  const [menuOpen, setMenuOpen] = useState(false);
  const [menuPosition, setMenuPosition] = useState<SessionMenuPosition | null>(null);
  const rootRef = useRef<HTMLDivElement | null>(null);
  const triggerRef = useRef<HTMLButtonElement | null>(null);
  const menuRef = useRef<HTMLDivElement | null>(null);

  useLayoutEffect(() => {
    if (!menuOpen) return;
    const trigger = triggerRef.current;
    const menu = menuRef.current;
    if (trigger === null || menu === null) return;
    const triggerRect = trigger.getBoundingClientRect();
    const menuRect = menu.getBoundingClientRect();
    setMenuPosition(
      placeSessionMenu(
        triggerRect,
        { width: menuRect.width, height: menuRect.height },
        { width: window.innerWidth, height: window.innerHeight },
      ),
    );
  }, [menuOpen]);

  // Close on navigation so the fixed coordinates can never outlive their anchor.
  useEffect(() => {
    if (!menuOpen) return;
    const onKey = (event: globalThis.KeyboardEvent) => {
      if (event.key === 'Escape') setMenuOpen(false);
    };
    const onPress = (event: globalThis.MouseEvent) => {
      const root = rootRef.current;
      const menu = menuRef.current;
      if (
        event.target instanceof Node &&
        !root?.contains(event.target) &&
        !menu?.contains(event.target)
      ) {
        setMenuOpen(false);
      }
    };
    const onViewportChange = () => setMenuOpen(false);
    document.addEventListener('keydown', onKey);
    document.addEventListener('mousedown', onPress);
    window.addEventListener('resize', onViewportChange);
    window.addEventListener('scroll', onViewportChange, true);
    return () => {
      document.removeEventListener('keydown', onKey);
      document.removeEventListener('mousedown', onPress);
      window.removeEventListener('resize', onViewportChange);
      window.removeEventListener('scroll', onViewportChange, true);
    };
  }, [menuOpen]);

  const menuId = `plume-session-menu-${session.id}`;
  const pick = (action: () => void) => () => {
    setMenuOpen(false);
    action();
  };

  return (
    <div
      ref={rootRef}
      className={`plume-project-sidebar-action-row plume-session-row${
        active ? ' plume-project-sidebar-action-row-active' : ''
      }`}
    >
      <button
        type="button"
        className="plume-project-sidebar-action-main"
        onClick={onSelect}
        aria-current={active ? 'page' : undefined}
      >
        <span className="plume-project-sidebar-label">{session.title}</span>
        <span className="plume-project-sidebar-meta">
          {relativeTime(session.updatedAtMs)}
        </span>
      </button>
      <button
        ref={triggerRef}
        type="button"
        className="plume-project-sidebar-mini plume-project-sidebar-mini-menu"
        onClick={() => setMenuOpen((open) => !open)}
        aria-label={`Chat actions for ${session.title}`}
        aria-haspopup="menu"
        aria-expanded={menuOpen}
        aria-controls={menuOpen ? menuId : undefined}
        title={`Chat actions for ${session.title}`}
      >
        <span aria-hidden="true">...</span>
      </button>
      {menuOpen ? createPortal(
        <div
          ref={menuRef}
          id={menuId}
          className="plume-session-menu"
          role="menu"
          aria-label={`Actions for ${session.title}`}
          style={
            menuPosition === null
              ? { visibility: 'hidden' }
              : { left: menuPosition.left, top: menuPosition.top }
          }
        >
          <button type="button" role="menuitem" className="plume-session-menu-item" onClick={pick(onRename)}>
            Rename
          </button>
          <button type="button" role="menuitem" className="plume-session-menu-item" onClick={pick(onArchive)}>
            Archive
          </button>
          <button
            type="button"
            role="menuitem"
            className="plume-session-menu-item plume-session-menu-item-danger"
            onClick={pick(onDelete)}
          >
            Delete
          </button>
        </div>,
        document.body,
      ) : null}
    </div>
  );
}

/** Compact "how fresh" meta, mirroring the D62 placeholder's tone
 * ("now") without pretending to more precision than a sidebar needs. */
export function relativeTime(thenMs: number, nowMs: number = Date.now()): string {
  const delta = Math.max(0, nowMs - thenMs);
  const minutes = Math.floor(delta / 60_000);
  if (minutes < 1) return 'now';
  if (minutes < 60) return `${minutes}m`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours}h`;
  const days = Math.floor(hours / 24);
  return `${days}d`;
}
