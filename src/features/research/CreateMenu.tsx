import { useEffect, useLayoutEffect, useRef, useState } from 'react';

type CreateMenuProps = {
  disabledReason: string | null;
  onResearchNote: () => void;
};

export function CreateMenu({ disabledReason, onResearchNote }: CreateMenuProps) {
  const [open, setOpen] = useState(false);
  const rootRef = useRef<HTMLDivElement | null>(null);
  const triggerRef = useRef<HTMLButtonElement | null>(null);
  const menuRef = useRef<HTMLDivElement | null>(null);

  useLayoutEffect(() => {
    if (open) menuRef.current?.querySelector<HTMLButtonElement>('[role="menuitem"]')?.focus();
  }, [open]);

  useEffect(() => {
    if (!open) return;
    const close = () => {
      setOpen(false);
      triggerRef.current?.focus();
    };
    const onKeyDown = (event: globalThis.KeyboardEvent) => {
      if (event.key !== 'Escape') return;
      event.preventDefault();
      close();
    };
    const onMouseDown = (event: globalThis.MouseEvent) => {
      if (event.target instanceof Node && !rootRef.current?.contains(event.target)) close();
    };
    document.addEventListener('keydown', onKeyDown);
    document.addEventListener('mousedown', onMouseDown);
    return () => {
      document.removeEventListener('keydown', onKeyDown);
      document.removeEventListener('mousedown', onMouseDown);
    };
  }, [open]);

  const closeAndRestoreFocus = () => {
    setOpen(false);
    triggerRef.current?.focus();
  };
  const chooseResearch = () => {
    if (disabledReason !== null) return;
    closeAndRestoreFocus();
    onResearchNote();
  };
  const navigateMenu = (event: React.KeyboardEvent<HTMLDivElement>) => {
    const items = Array.from(
      menuRef.current?.querySelectorAll<HTMLButtonElement>('[role="menuitem"]') ?? [],
    );
    if (items.length === 0) return;
    const current = items.indexOf(document.activeElement as HTMLButtonElement);
    let next = current;
    if (event.key === 'ArrowDown') next = (current + 1) % items.length;
    else if (event.key === 'ArrowUp') next = (current - 1 + items.length) % items.length;
    else if (event.key === 'Home') next = 0;
    else if (event.key === 'End') next = items.length - 1;
    else return;
    event.preventDefault();
    items[next]?.focus();
  };

  return (
    <div ref={rootRef} className="plume-create-menu-root">
      <button
        ref={triggerRef}
        type="button"
        className="ink-button plume-create-menu-trigger"
        aria-haspopup="menu"
        aria-expanded={open}
        onClick={() => setOpen((current) => !current)}
      >
        Create
      </button>
      {open ? (
        <div
          ref={menuRef}
          className="plume-create-menu"
          role="menu"
          aria-label="Create"
          onKeyDown={navigateMenu}
        >
          <button
            type="button"
            role="menuitem"
            aria-label="Research note"
            className="plume-create-menu-item"
            disabled={disabledReason !== null}
            onClick={chooseResearch}
          >
            <span>Research note</span>
            {disabledReason !== null ? <small>{disabledReason}</small> : null}
          </button>
        </div>
      ) : null}
    </div>
  );
}
