import { readFileSync } from 'node:fs';
import { join } from 'node:path';

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
    onRewind: vi.fn(),
    onArchive: vi.fn(),
    onDelete: vi.fn(),
  };
  const view = render(<SessionRow session={session} active={false} {...actions} />);
  return { ...view, ...actions };
}

describe('SessionRow menu', () => {
  it('uses the shared more icon and explains branching without changing the original', async () => {
    setup();
    const trigger = screen.getByRole('button', { name: 'Chat actions for Bottom chat' });

    expect(trigger.querySelector('svg')).toBeInTheDocument();
    expect(trigger).not.toHaveTextContent('...');
    await userEvent.click(trigger);

    expect(screen.getByRole('menu')).toHaveTextContent(
      'Copies the whole conversation into a new chat. The original stays unchanged.',
    );
    expect(screen.getByRole('menu')).toHaveTextContent(
      'Creates a new chat ending before selected recent turns. The original stays unchanged.',
    );
  });

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
    expect(trigger).toHaveFocus();
  });

  it('focuses the first item and supports arrow, Home, and End navigation', async () => {
    setup();
    const user = userEvent.setup();
    await user.click(screen.getByRole('button', { name: 'Chat actions for Bottom chat' }));

    const rename = screen.getByRole('menuitem', { name: 'Rename' });
    expect(rename).toHaveFocus();

    await user.keyboard('{ArrowDown}');
    expect(screen.getByRole('menuitem', { name: /Continue in new chat/ })).toHaveFocus();
    await user.keyboard('{End}');
    expect(screen.getByRole('menuitem', { name: 'Delete' })).toHaveFocus();
    await user.keyboard('{Home}');
    expect(rename).toHaveFocus();
    await user.keyboard('{ArrowUp}');
    expect(screen.getByRole('menuitem', { name: 'Delete' })).toHaveFocus();
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

describe('migrated menu CSS', () => {
  const css = readFileSync(
    join(process.cwd(), 'src/styles/layout/project-shell.css'),
    'utf8',
  );
  const migratedRules = [...css.matchAll(/\.(?:plume-session-(?:menu|dialog)[^{\s,]*|plume-tool-drawer[^{\s,]*)[^\{]*\{([^}]*)\}/g)]
    .map((match) => match[1] ?? '')
    .join('\n');
  const menuRule = [...css.matchAll(/\.plume-session-menu\s*\{([^}]*)\}/g)]
    .map((match) => match[1] ?? '')
    .find((rule) => rule.includes('position: fixed')) ?? '';
  const darkPortalRule = css.match(
    /\.plume-project-codex,\s*\.plume-session-menu\s*\{([^}]*)\}/,
  )?.[1] ?? '';
  const drawerHeaderRule = css.match(
    /\.plume-tool-drawer-header\s*\{([^}]*)\}/,
  )?.[1] ?? '';
  const drawerItemRule = css.match(
    /\.plume-tool-drawer-item\s*\{([^}]*)\}/,
  )?.[1] ?? '';

  it('keeps the floating menu opaque', () => {
    expect(menuRule).toMatch(/background:\s*var\(--menu-fill\)/);
    expect(menuRule).not.toMatch(/background:\s*(?:transparent|rgba\()/);
  });

  it('does not silently force the body-portal menu into macOS dark appearance', () => {
    expect(darkPortalRule).toBe('');
  });

  it('keeps drawer header and item fills on dark-capable chrome tokens', () => {
    expect(drawerHeaderRule).toMatch(/background:\s*var\(--plume-chrome-muted\)/);
    expect(drawerItemRule).toMatch(/background:\s*var\(--plume-chrome-fill\)/);
    expect(`${drawerHeaderRule}\n${drawerItemRule}`).not.toMatch(
      /background:\s*(?:rgba?\(|#[a-f\d]{3,8})/i,
    );
  });

  it('uses approved typography and radius tokens in migrated surfaces', () => {
    const fontFamilies = [...migratedRules.matchAll(/font-family:\s*([^;]+);/g)]
      .map((match) => match[1]?.trim());
    const radii = [...migratedRules.matchAll(/border-radius:\s*([^;]+);/g)]
      .map((match) => match[1]?.trim());

    expect(fontFamilies.every((family) => /^var\(--font-(?:prose|ui|code)\)$/.test(family ?? ''))).toBe(true);
    expect(radii.every((radius) => /^var\(--[a-z0-9-]+\)$/.test(radius ?? ''))).toBe(true);
  });
});
