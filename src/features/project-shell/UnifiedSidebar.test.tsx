// D63B: the sidebar renders PERSISTED session lists — local and
// project strictly separate — and routes row/menu actions with the
// right scope. Presentational component, so plain render + click.

import { render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';

import type { SessionSummary } from '../../lib/api/sessions';
import { UnifiedSidebar } from './UnifiedSidebar';

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
