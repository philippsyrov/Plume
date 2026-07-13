// D63B: one persisted-session row in the unified sidebar.
//
// Reuses the D62 `plume-project-sidebar-action-row` visual system;
// the only new chrome is the row menu (Rename / Archive / Delete)
// behind the existing "…" mini button — a small popover with stable
// accessible names, Escape-to-close, and no native dialogs.

import { useEffect, useRef, useState } from 'react';

import type { SessionSummary } from '../../lib/api/sessions';

export type SessionRowProps = {
  session: SessionSummary;
  active: boolean;
  onSelect: () => void;
  onRename: () => void;
  onContinue: () => void;
  onRewind: () => void;
  onArchive: () => void;
  onDelete: () => void;
};

export function SessionRow({
  session,
  active,
  onSelect,
  onRename,
  onContinue,
  onRewind,
  onArchive,
  onDelete,
}: SessionRowProps) {
  const [menuOpen, setMenuOpen] = useState(false);
  const rootRef = useRef<HTMLDivElement | null>(null);

  // Close the menu on Escape or on any pointer press outside the row.
  useEffect(() => {
    if (!menuOpen) return;
    const onKey = (event: globalThis.KeyboardEvent) => {
      if (event.key === 'Escape') setMenuOpen(false);
    };
    const onPress = (event: globalThis.MouseEvent) => {
      const root = rootRef.current;
      if (root !== null && event.target instanceof Node && !root.contains(event.target)) {
        setMenuOpen(false);
      }
    };
    document.addEventListener('keydown', onKey);
    document.addEventListener('mousedown', onPress);
    return () => {
      document.removeEventListener('keydown', onKey);
      document.removeEventListener('mousedown', onPress);
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
      {menuOpen ? (
        <div id={menuId} className="plume-session-menu" role="menu" aria-label={`Actions for ${session.title}`}>
          <button type="button" role="menuitem" className="plume-session-menu-item" onClick={pick(onRename)}>
            Rename
          </button>
          <button type="button" role="menuitem" className="plume-session-menu-item" onClick={pick(onContinue)}>
            Continue in new chat
          </button>
          <button type="button" role="menuitem" className="plume-session-menu-item" onClick={pick(onRewind)}>
            Rewind into new chat…
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
        </div>
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
