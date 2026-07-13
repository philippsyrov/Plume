// D63B: the sidebar renders PERSISTED session lists — local and
// project strictly separate — and routes row/menu actions with the
// right scope. Presentational component, so plain render + click.

import { render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { describe, expect, it, vi } from 'vitest';

import type { SessionSummary } from '../../lib/api/sessions';
import { UnifiedSidebar } from './UnifiedSidebar';

const projectShellCss = readFileSync(
  join(process.cwd(), 'src/styles/layout/project-shell.css'),
  'utf8',
);

function blockOf(css: string, selector: string): string {
  const escaped = selector.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
  const match = css.match(new RegExp(`${escaped}\\s*\\{([^}]*)\\}`, 's'));
  if (!match?.[1]) throw new Error(`no rule block found for ${selector}`);
  return match[1];
}

function summary(id: string, title: string): SessionSummary {
  return { id, title, createdAtMs: 1, updatedAtMs: 2, archivedAtMs: null };
}

function renderSidebar(overrides: Partial<Parameters<typeof UnifiedSidebar>[0]> = {}) {
  const handlers = {
    onSelectSession: vi.fn(),
    onNewLocalChat: vi.fn(),
    onNewProjectChat: vi.fn(),
    onRenameSession: vi.fn(),
    onArchiveSession: vi.fn(),
    onDeleteSession: vi.fn(),
    onShowArchived: vi.fn(),
    onSearch: vi.fn(),
    onSettings: vi.fn(),
    onOpenProject: vi.fn(),
    onCloseProject: vi.fn(),
  };
  render(
    <UnifiedSidebar
      projectName="plume-demo"
      trustLabel="trusted"
      activeView="project-chat"
      settingsOpen={false}
      localSessions={[summary('l1', 'Groceries planning')]}
      projectSessions={[summary('p1', 'Refactor greeting')]}
      activeSessionId="p1"
      activeScope="project"
      hasArchivedLocal={false}
      hasArchivedProject={false}
      {...handlers}
      {...overrides}
    />,
  );
  return handlers;
}

describe('UnifiedSidebar sessions', () => {
  it('keeps navigation and session sections inside one scroll owner, with the footer outside', () => {
    renderSidebar();
    const sidebar = screen.getByRole('complementary', { name: 'Project navigation' });
    const content = sidebar.querySelector('.plume-project-sidebar-content');
    const nav = screen.getByRole('navigation', { name: 'Workspace' });
    const sections = sidebar.querySelectorAll('.plume-project-sidebar-section');
    const footer = sidebar.querySelector('.plume-project-sidebar-footer');

    expect(content).not.toBeNull();
    expect(content).toContainElement(nav);
    expect(content).toContainElement(sections[0] as HTMLElement);
    expect(content).toContainElement(sections[1] as HTMLElement);
    expect(content).not.toContainElement(footer as HTMLElement);
    expect(footer?.parentElement).toBe(sidebar);
  });

  it('assigns vertical scrolling to the session content while the sidebar keeps clipping', () => {
    const sidebar = blockOf(projectShellCss, '.plume-project-sidebar');
    const content = blockOf(projectShellCss, '.plume-project-sidebar-content');
    const footer = blockOf(projectShellCss, '.plume-project-sidebar-footer');

    expect(sidebar).toMatch(/overflow:\s*hidden/);
    expect(content).toMatch(/min-height:\s*0/);
    expect(content).toMatch(/overflow-y:\s*auto/);
    expect(footer).toMatch(/flex:\s*0 0 auto/);
  });

  it('renders local and project sessions in their own sections', () => {
    renderSidebar();
    const sections = document.querySelectorAll('.plume-project-sidebar-section');
    const chats = sections[0] as HTMLElement;
    const projects = sections[1] as HTMLElement;

    expect(within(chats).getByText('Groceries planning')).toBeInTheDocument();
    expect(within(chats).queryByText('Refactor greeting')).not.toBeInTheDocument();
    expect(within(projects).getByText('Refactor greeting')).toBeInTheDocument();
    expect(within(projects).queryByText('Groceries planning')).not.toBeInTheDocument();
  });

  it('routes selection with the row scope', async () => {
    const handlers = renderSidebar();
    await userEvent.click(screen.getByText('Groceries planning'));
    expect(handlers.onSelectSession).toHaveBeenCalledWith('local', 'l1');
    await userEvent.click(screen.getByText('Refactor greeting'));
    expect(handlers.onSelectSession).toHaveBeenCalledWith('project', 'p1');
  });

  it('New chat creates a local session; the project plus creates a project one', async () => {
    const handlers = renderSidebar();
    await userEvent.click(screen.getByRole('button', { name: 'New chat' }));
    expect(handlers.onNewLocalChat).toHaveBeenCalledTimes(1);
    expect(handlers.onNewProjectChat).not.toHaveBeenCalled();

    await userEvent.click(screen.getByRole('button', { name: 'New project chat' }));
    expect(handlers.onNewProjectChat).toHaveBeenCalledTimes(1);
  });

  it('Search chats opens the search overlay (D66)', async () => {
    const handlers = renderSidebar();
    await userEvent.click(screen.getByRole('button', { name: 'Search chats' }));
    expect(handlers.onSearch).toHaveBeenCalledTimes(1);
  });

  it('the row menu exposes Rename / Archive / Delete with the session attached', async () => {
    const handlers = renderSidebar();
    await userEvent.click(
      screen.getByRole('button', { name: 'Chat actions for Groceries planning' }),
    );
    await userEvent.click(screen.getByRole('menuitem', { name: 'Rename' }));
    expect(handlers.onRenameSession).toHaveBeenCalledWith(
      'local',
      expect.objectContaining({ id: 'l1' }),
    );

    await userEvent.click(
      screen.getByRole('button', { name: 'Chat actions for Refactor greeting' }),
    );
    await userEvent.click(screen.getByRole('menuitem', { name: 'Delete' }));
    expect(handlers.onDeleteSession).toHaveBeenCalledWith(
      'project',
      expect.objectContaining({ id: 'p1' }),
    );
  });

  it('shows quiet empty states and the archived entry points', () => {
    const handlers = renderSidebar({
      localSessions: [],
      projectSessions: [],
      hasArchivedLocal: true,
      hasArchivedProject: true,
    });
    expect(screen.getByText(/No chats yet/)).toBeInTheDocument();
    expect(screen.getByText(/No project chats yet/)).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Archived chats' })).toBeInTheDocument();
    expect(
      screen.getByRole('button', { name: 'Archived project chats' }),
    ).toBeInTheDocument();
    expect(handlers.onShowArchived).not.toHaveBeenCalled();
  });

  it('without a project there are no project chats, only Open project', () => {
    renderSidebar({
      projectName: null,
      projectSessions: [],
      activeScope: 'local',
      activeSessionId: 'l1',
      activeView: 'local-chat',
    });
    expect(screen.getByRole('button', { name: 'Open project' })).toBeInTheDocument();
    expect(screen.queryByText('Refactor greeting')).not.toBeInTheDocument();
    expect(
      screen.queryByRole('button', { name: 'New project chat' }),
    ).not.toBeInTheDocument();
  });
});
