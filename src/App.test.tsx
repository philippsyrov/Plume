// D63B regression (Codex P1 on #108): opening a DIFFERENT project must
// remount the project shell, so project A's session rows and loaded
// transcript can never stay visible while the backend's project scope
// already points at project B. The sessions IPC here is faked the way
// the backend behaves: `sessions.list({scope:'project'})` resolves
// against whichever project is currently open.

import { act, render, screen, waitFor } from '@testing-library/react';
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

const surfaceProps = vi.hoisted(() => ({
  knowledge: null as null | Record<string, unknown>,
  inspector: null as null | Record<string, unknown>,
  browser: null as null | Record<string, unknown>,
  navigator: {
    selection: {
      kind: 'ready',
      path: 'src/App.tsx',
      content: { content: 'one\ntwo', encoding: 'utf-8', bytes: 7 },
    },
    currentLineRange: { startLine: 1, endLine: 2 },
  } as Record<string, unknown>,
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
  useFileNavigator: () => surfaceProps.navigator,
  FileNavigator: () => null,
  FileInspector: (props: Record<string, unknown>) => {
    surfaceProps.inspector = props;
    return (
      <button
        type="button"
        onClick={() =>
          (props.onContextDragActiveChange as ((active: boolean) => void) | undefined)?.(true)
        }
      >
        Start inspector drag
      </button>
    );
  },
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
  ChatPanel: ({
    chat,
    emphasizedContextKey,
  }: {
    chat?: { entries: unknown[]; contextSources: unknown[] };
    emphasizedContextKey?: string | null;
  }) => (
    <div data-testid="chat-stub">
      entries:{chat ? chat.entries.length : 'internal'} sources:
      {chat ? chat.contextSources.length : 'internal'} emphasis:{emphasizedContextKey ?? 'none'}
    </div>
  ),
}));
vi.mock('./features/knowledge/KnowledgePanel', () => ({
  KnowledgePanel: (props: Record<string, unknown>) => {
    surfaceProps.knowledge = props;
    return (
      <div data-testid="knowledge-stub">
        knowledge panel stub
        <button
          type="button"
          onClick={() =>
            (props.onContextDragActiveChange as ((active: boolean) => void) | undefined)?.(true)
          }
        >
          Start knowledge drag
        </button>
      </div>
    );
  },
}));
vi.mock('./features/browser/TaskBrowserWorkspace', () => ({
  TaskBrowserWorkspace: (props: Record<string, unknown>) => {
    surfaceProps.browser = props;
    const chatProps = props.chatProps as { chat?: { entries: unknown[]; contextSources: unknown[] } };
    return <div data-testid="browser-stub">browser panel stub<div data-testid="chat-stub">entries:{chatProps.chat?.entries.length ?? 0} sources:{chatProps.chat?.contextSources.length ?? 0}</div></div>;
  },
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
    surfaceProps.knowledge = null;
    surfaceProps.inspector = null;
    surfaceProps.browser = null;
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
    api.createSession.mockImplementation(({ scope }: { scope: string }) => {
      const id = `s_${scope === 'local' ? 'a' : 'b'}`.padEnd(34, scope === 'local' ? 'a' : 'b');
      return Promise.resolve({ session: summary(id, 'New chat') });
    });
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

  it('creates a local chat before opening its task-owned Browser', async () => {
    render(<App />);

    await userEvent.click(screen.getByRole('button', { name: 'Open workspace views' }));
    await userEvent.click(screen.getByRole('button', { name: 'Browser' }));

    await waitFor(() => expect(screen.getByTestId('browser-stub')).toBeInTheDocument());
    expect(api.openProject).not.toHaveBeenCalled();
    expect(api.createSession).toHaveBeenCalledWith({ scope: 'local' });
    expect(surfaceProps.browser?.identity).toMatchObject({ scope: 'local' });
    expect(surfaceProps.browser?.onUseInChat).toBeTypeOf('function');
  });

  it('opens the same Browser workspace inside a trusted project', async () => {
    render(<App />);
    await openProjectViaModal('/proj/alpha');

    await userEvent.click(screen.getByRole('button', { name: 'Open workspace views' }));
    await userEvent.click(screen.getByRole('button', { name: 'Browser' }));

    expect(screen.getByTestId('browser-stub')).toBeInTheDocument();
    expect(screen.getByTestId('chat-stub')).toBeInTheDocument();
    expect(surfaceProps.browser?.onUseInChat).toBeTypeOf('function');
  });

  it('rejects a delayed project Browser handoff after the selected task changes', async () => {
    render(<App />);
    await openProjectViaModal('/proj/alpha');
    await userEvent.click(screen.getByRole('button', { name: 'Open workspace views' }));
    await userEvent.click(screen.getByRole('button', { name: 'Browser' }));
    const owner = surfaceProps.browser?.identity as { scope: 'project'; sessionId: string };
    const handoff = surfaceProps.browser?.onUseInChat as (
      owner: { scope: 'project'; sessionId: string },
      source: { kind: 'browserTextEvidence'; evidenceId: string },
    ) => Promise<string>;

    await userEvent.click(screen.getByRole('button', { name: 'New project chat' }));
    await waitFor(() => expect(api.createSession).toHaveBeenCalledWith({ scope: 'project' }));
    expect(await handoff(owner, {
      kind: 'browserTextEvidence', evidenceId: `be_${'e'.repeat(32)}`,
    })).toBe('unavailable');
  });

  it('rejects a Browser handoff whose local task owner is no longer selected', async () => {
    render(<App />);
    await userEvent.click(screen.getByRole('button', { name: 'Open workspace views' }));
    await userEvent.click(screen.getByRole('button', { name: 'Browser' }));
    const handoff = surfaceProps.browser?.onUseInChat as (
      owner: { scope: 'local'; sessionId: string },
      source: { kind: 'browserScreenshotEvidence'; evidenceId: string },
    ) => Promise<string>;
    expect(await handoff(
      { scope: 'local', sessionId: `s_${'f'.repeat(32)}` },
      { kind: 'browserScreenshotEvidence', evidenceId: `bs_${'e'.repeat(32)}` },
    )).toBe('unavailable');
  });

  it('does not grant a project Browser before that project is trusted', async () => {
    api.openProject.mockImplementationOnce((path: string) => {
      api.openRoot.current = path;
      return Promise.resolve({ ...meta(path), trust: 'unknown' });
    });
    render(<App />);
    await openProjectViaModal('/proj/alpha');

    expect(screen.getAllByRole('heading', { name: 'Plume' })).toHaveLength(1);
    expect(screen.queryByRole('button', { name: 'Open Browser' })).not.toBeInTheDocument();
    expect(screen.queryByTestId('browser-stub')).not.toBeInTheDocument();
  });

  it('adds an exact Knowledge ref to project chat and reveals the temporary drop target', async () => {
    render(<App />);
    await openProjectViaModal('/proj/alpha');
    await userEvent.click(screen.getByRole('button', { name: 'Open workspace views' }));
    await userEvent.click(screen.getByRole('button', { name: 'Knowledge' }));

    await userEvent.click(screen.getByRole('button', { name: 'Start knowledge drag' }));
    expect(screen.getByText('Drop into project chat')).toBeInTheDocument();

    const onUseInChat = surfaceProps.knowledge?.onUseInChat as
      | ((source: unknown) => Promise<string>)
      | undefined;
    expect(onUseInChat).toBeTypeOf('function');
    await act(async () => {
      expect(
        await onUseInChat?.({ kind: 'memoryEntry', entryId: `m_${'a'.repeat(32)}` }),
      ).toBe('added');
    });
    expect(screen.getByTestId('chat-stub')).toHaveTextContent('sources:1');
    expect(screen.getByTestId('chat-stub')).toHaveTextContent(
      `emphasis:memory:m_${'a'.repeat(32)}`,
    );
  });

  it('derives the exact inspector selection ref and reveals the same drop target in Files', async () => {
    render(<App />);
    await openProjectViaModal('/proj/alpha');
    await userEvent.click(screen.getByRole('button', { name: 'Open workspace views' }));
    await userEvent.click(screen.getByRole('button', { name: 'Files' }));

    expect(surfaceProps.inspector?.contextSource).toEqual({
      kind: 'projectFile',
      relPath: 'src/App.tsx',
      startLine: 1,
      endLine: 2,
    });
    await userEvent.click(screen.getByRole('button', { name: 'Start inspector drag' }));
    expect(screen.getByText('Drop into project chat')).toBeInTheDocument();
  });
});
