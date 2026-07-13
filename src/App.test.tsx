// D63B regression (Codex P1 on #108): opening a DIFFERENT project must
// remount the project shell, so project A's session rows and loaded
// transcript can never stay visible while the backend's project scope
// already points at project B. The sessions IPC here is faked the way
// the backend behaves: `sessions.list({scope:'project'})` resolves
// against whichever project is currently open.

import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import type { ProjectMeta } from './lib/api/project';
import type { SessionSummary } from './lib/api/sessions';
import { App } from './App';

const api = vi.hoisted(() => ({
  openProject: vi.fn(),
  trustProject: vi.fn(),
  listSessions: vi.fn(),
  createSession: vi.fn(),
  renameSession: vi.fn(),
  archiveSession: vi.fn(),
  deleteSession: vi.fn(),
  loadSession: vi.fn(),
  saveSessionTranscript: vi.fn(),
  /** The "currently open project" the fake backend resolves
   * project-scope calls against. */
  openRoot: { current: '' },
}));

vi.mock('./lib/api/project', () => ({
  openProject: api.openProject,
  trustProject: api.trustProject,
}));
vi.mock('./lib/api/sessions', () => ({
  listSessions: api.listSessions,
  createSession: api.createSession,
  renameSession: api.renameSession,
  archiveSession: api.archiveSession,
  deleteSession: api.deleteSession,
  loadSession: api.loadSession,
  saveSessionTranscript: api.saveSessionTranscript,
  searchSessions: vi.fn().mockResolvedValue({ hits: [] }),
  SEARCH_SNIPPET_START: '',
  SEARCH_SNIPPET_END: '',
  MAX_SEARCH_RESULTS: 20,
}));
vi.mock('./features/file-tree/FileBrowser', () => ({
  useFileNavigator: () => ({ selection: null, currentLineRange: null }),
  FileNavigator: () => null,
  FileInspector: () => null,
}));
vi.mock('./features/providers/useProviderInventory', () => ({
  useProviderInventory: () => ({
    state: { kind: 'loading' },
    refreshing: false,
    revision: 0,
    load: vi.fn().mockResolvedValue(undefined),
  }),
}));
vi.mock('./features/providers/useMlxServers', () => ({
  MLX_LM_PROVIDER_ID: 'mlx-lm',
  useMlxServers: () => ({
    statuses: new Map(),
    statusOf: () => ({ kind: 'idle' }),
    handleOf: () => null,
    start: vi.fn().mockResolvedValue(null),
    stop: vi.fn().mockResolvedValue(undefined),
    clearError: vi.fn(),
  }),
}));
// The chat surface is out of scope here; the stub proves which chat
// instance (and how many restored entries) the shell wired in.
vi.mock('./features/chat/ChatPanel', () => ({
  ChatPanel: ({ chat }: { chat?: { entries: unknown[] } }) => (
    <div data-testid="chat-stub">entries:{chat ? chat.entries.length : 'internal'}</div>
  ),
}));
vi.mock('./features/knowledge/KnowledgePanel', () => ({
  KnowledgePanel: () => <div data-testid="knowledge-stub">knowledge panel stub</div>,
}));

function meta(root: string): ProjectMeta {
  return {
    id: `project-${root}`,
    root,
    hasAgentsMd: false,
    hasClaudeMd: false,
    packageManagers: [],
    git: null,
    trust: 'trusted',
  };
}

function summary(id: string, title: string): SessionSummary {
  return { id, title, createdAtMs: 1, updatedAtMs: 2, archivedAtMs: null,
    forkedFromSessionId: null, forkedThroughEntryId: null };
}

const PROJECT_ROWS: Record<string, SessionSummary[]> = {
  '/proj/alpha': [summary('pa', 'Alpha planning chat')],
  '/proj/beta': [summary('pb', 'Beta refactor chat')],
};

async function openProjectViaModal(path: string) {
  await userEvent.click(screen.getByRole('button', { name: /^Open (a )?project$/ }));
  await userEvent.type(screen.getByLabelText('Project path'), path);
  await userEvent.click(screen.getByRole('button', { name: 'Open' }));
}

describe('App project switching (D63B)', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    api.openRoot.current = '';
    api.openProject.mockImplementation((path: string) => {
      api.openRoot.current = path;
      return Promise.resolve(meta(path));
    });
    api.listSessions.mockImplementation(({ scope }: { scope: string }) =>
      Promise.resolve({
        sessions:
          scope === 'local' ? [] : (PROJECT_ROWS[api.openRoot.current] ?? []),
      }),
    );
    api.loadSession.mockImplementation(({ sessionId }: { sessionId: string }) =>
      Promise.resolve({
        session: { ...summary(sessionId, 'loaded'), entries: [] },
      }),
    );
    api.saveSessionTranscript.mockImplementation(({ sessionId }: { sessionId: string }) =>
      Promise.resolve({ session: summary(sessionId, 'saved') }),
    );
  });

  it("switching projects replaces the previous project's session rows", async () => {
    render(<App />);

    await openProjectViaModal('/proj/alpha');
    await waitFor(() =>
      expect(screen.getByText('Alpha planning chat')).toBeInTheDocument(),
    );

    await openProjectViaModal('/proj/beta');
    await waitFor(() =>
      expect(screen.getByText('Beta refactor chat')).toBeInTheDocument(),
    );
    // The leak this test pins (Codex P1): without the per-root remount,
    // project A's rows stayed in the sidebar after opening project B.
    expect(screen.queryByText('Alpha planning chat')).not.toBeInTheDocument();
    // And the transcripts loaded came from each project's own store.
    const loadedIds = api.loadSession.mock.calls.map(([p]) => p.sessionId);
    expect(loadedIds).toContain('pa');
    expect(loadedIds).toContain('pb');
    expect(api.loadSession).toHaveBeenCalledWith({ scope: 'project', sessionId: 'pb' });
  });

  it('opens Knowledge only inside the trusted project workspace', async () => {
    render(<App />);

    await openProjectViaModal('/proj/alpha');
    await userEvent.click(screen.getByRole('button', { name: 'Open workspace views' }));
    await userEvent.click(screen.getByRole('button', { name: 'Knowledge' }));

    expect(screen.getByTestId('knowledge-stub')).toBeInTheDocument();
    expect(screen.getByText('Knowledge')).toBeInTheDocument();
    expect(screen.queryByTestId('chat-stub')).not.toBeInTheDocument();
  });
});
