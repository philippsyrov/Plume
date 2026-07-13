import { fireEvent, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';

import type { SessionSummary } from '../../lib/api/sessions';
import { SessionRow } from './SessionRow';

const session: SessionSummary = {
  id: 'session-1',
  title: 'Bottom chat',
  createdAtMs: 1,
  updatedAtMs: 2,
  archivedAtMs: null,
  forkedFromSessionId: null,
  forkedThroughEntryId: null,
};

function setup() {
  const actions = {
    onSelect: vi.fn(),
    onRename: vi.fn(),
    onContinue: vi.fn(),
    onArchive: vi.fn(),
    onDelete: vi.fn(),
  };
  const view = render(<SessionRow session={session} active={false} {...actions} />);
  return { ...view, ...actions };
}

describe('SessionRow menu', () => {
  it('positions the portal from the ellipsis actions button', async () => {
    const rect = (values: Partial<DOMRect>): DOMRect =>
      ({
        left: 0,
        right: 0,
        top: 0,
        bottom: 0,
        width: 0,
        height: 0,
        x: 0,
        y: 0,
        toJSON: () => ({}),
        ...values,
      }) as DOMRect;
    const bounds = vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect');
    bounds.mockImplementation(function (this: HTMLElement) {
      if (this.classList.contains('plume-project-sidebar-action-main')) {
        return rect({ left: 20, right: 180, top: 40, bottom: 80, width: 160, height: 40 });
      }
      if (this.classList.contains('plume-project-sidebar-mini-menu')) {
        return rect({ left: 180, right: 220, top: 100, bottom: 124, width: 40, height: 24 });
      }
      if (this.classList.contains('plume-session-menu')) {
        return rect({ width: 132, height: 104 });
      }
      return rect({});
    });
    vi.stubGlobal('innerWidth', 400);
    vi.stubGlobal('innerHeight', 400);
    setup();

    await userEvent.click(screen.getByRole('button', { name: 'Chat actions for Bottom chat' }));

    expect(screen.getByRole('menu')).toHaveStyle({ left: '88px', top: '128px' });
    bounds.mockRestore();
    vi.unstubAllGlobals();
  });

  it('portals the menu to document.body and preserves all actions', async () => {
    const { container, onRename, onArchive, onDelete } = setup();
    await userEvent.click(screen.getByRole('button', { name: 'Chat actions for Bottom chat' }));

    const menu = screen.getByRole('menu', { name: 'Actions for Bottom chat' });
    expect(menu.parentElement).toBe(document.body);
    expect(container).not.toContainElement(menu);

    await userEvent.click(screen.getByRole('menuitem', { name: 'Rename' }));
    expect(onRename).toHaveBeenCalledOnce();

    await userEvent.click(screen.getByRole('button', { name: 'Chat actions for Bottom chat' }));
    await userEvent.click(screen.getByRole('menuitem', { name: 'Archive' }));
    expect(onArchive).toHaveBeenCalledOnce();

    await userEvent.click(screen.getByRole('button', { name: 'Chat actions for Bottom chat' }));
    await userEvent.click(screen.getByRole('menuitem', { name: 'Delete' }));
    expect(onDelete).toHaveBeenCalledOnce();
  });

  it('keeps portal clicks inside, but closes on outside mousedown and Escape', async () => {
    setup();
    const trigger = screen.getByRole('button', { name: 'Chat actions for Bottom chat' });
    await userEvent.click(trigger);

    fireEvent.mouseDown(screen.getByRole('menuitem', { name: 'Rename' }));
    expect(screen.getByRole('menu')).toBeInTheDocument();

    fireEvent.mouseDown(document.body);
    expect(screen.queryByRole('menu')).not.toBeInTheDocument();

    await userEvent.click(trigger);
    fireEvent.keyDown(document, { key: 'Escape' });
    expect(screen.queryByRole('menu')).not.toBeInTheDocument();
  });

  it('closes on scroll and resize so fixed coordinates cannot go stale', async () => {
    setup();
    const trigger = screen.getByRole('button', { name: 'Chat actions for Bottom chat' });
    await userEvent.click(trigger);
    fireEvent.scroll(window);
    expect(screen.queryByRole('menu')).not.toBeInTheDocument();

    await userEvent.click(trigger);
    fireEvent.resize(window);
    expect(screen.queryByRole('menu')).not.toBeInTheDocument();
  });
});
