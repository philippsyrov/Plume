// D63B regression (Codex P1 on #108): opening a DIFFERENT project must
// remount the project shell, so project A's session rows and loaded
// transcript can never stay visible while the backend's project scope
// already points at project B. The sessions IPC here is faked the way
// the backend behaves: `sessions.list({scope:'project'})` resolves
// against whichever project is currently open.

import { act, render, renderHook, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { useEffect } from 'react';

import type { ProjectMeta } from './lib/api/project';
import type { SessionSummary } from './lib/api/sessions';
import { App, useWindowModelState } from './App';

const api = vi.hoisted(() => ({
  openProject: vi.fn(),
  closeProject: vi.fn(),
  trustProject: vi.fn(),
  listSessions: vi.fn(),
  createSession: vi.fn(),
  renameSession: vi.fn(),
  archiveSession: vi.fn(),
  deleteSession: vi.fn(),
  loadSession: vi.fn(),
  homeSession: vi.fn(),
  sessionStorageUsage: vi.fn(),
  saveSessionTranscript: vi.fn(),
  /** The "currently open project" the fake backend resolves
   * project-scope calls against. */
  openRoot: { current: '' },
}));

const surfaceProps = vi.hoisted(() => ({
  library: null as null | Record<string, unknown>,
  librarySettings: [] as boolean[],
  inspector: null as null | Record<string, unknown>,
  browser: null as null | Record<string, unknown>,
  chat: null as null | Record<string, unknown>,
  navigator: {
    selection: {
      kind: 'ready',
      path: 'src/App.tsx',
      content: { content: 'one\ntwo', encoding: 'utf-8', bytes: 7 },
    },
    currentLineRange: { startLine: 1, endLine: 2 },
  } as Record<string, unknown>,
}));

const selectedModelControl = vi.hoisted(() => ({
  select: null as null | ((next: { providerId: string; providerDisplayName: string; modelId: string }) => void),
}));

const modelCatalogControl = vi.hoisted(() => ({
  api: {
    entries: [],
    entry: () => null,
    loading: false,
    downloadEventsReady: true,
    error: null,
    download: vi.fn().mockResolvedValue(undefined),
    cancelDownload: vi.fn().mockResolvedValue(undefined),
    useApple: vi.fn().mockResolvedValue(undefined),
    useQwen: vi.fn().mockResolvedValue(undefined),
    removeQwen: vi.fn().mockResolvedValue(undefined),
    refresh: vi.fn().mockResolvedValue(undefined),
  },
}));

vi.mock('./lib/api/project', () => ({
  openProject: api.openProject,
  closeProject: api.closeProject,
  trustProject: api.trustProject,
  chooseProjectFolder: vi.fn().mockResolvedValue(null),
}));
vi.mock('@tauri-apps/api/webview', () => ({
  getCurrentWebview: () => ({ onDragDropEvent: vi.fn().mockResolvedValue(vi.fn()) }),
}));
vi.mock('./lib/api/sessions', () => ({
  listSessions: api.listSessions,
  createSession: api.createSession,
  renameSession: api.renameSession,
  archiveSession: api.archiveSession,
  deleteSession: api.deleteSession,
  loadSession: api.loadSession,
  homeSession: api.homeSession,
  sessionStorageUsage: api.sessionStorageUsage,
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
vi.mock('./features/model-picker/useModelCatalog', () => ({
  defaultModelCatalogDependencies: {},
  useModelCatalog: () => modelCatalogControl.api,
}));
vi.mock('./features/model-picker/useSelectedModel', async () => {
  const React = await vi.importActual<typeof import('react')>('react');
  return {
    useSelectedModel: () => {
      const [selected, setSelected] = React.useState<{
        providerId: string;
        providerDisplayName: string;
        modelId: string;
      } | null>(null);
      selectedModelControl.select = setSelected;
      return { selected, select: setSelected, clear: () => setSelected(null) };
    },
  };
});
// The chat surface is out of scope here; the stub proves which chat
// instance (and how many restored entries) the shell wired in.
vi.mock('./features/chat/ChatPanel', () => ({
  ChatPanel: ({
    chat,
    emphasizedContextKey,
    selected,
    ...props
  }: {
    chat?: { entries: unknown[]; contextSources: unknown[] };
    emphasizedContextKey?: string | null;
    selected?: { modelId: string } | null;
    onOpenResearchSource?: (url: string) => void;
  }) => (
    <div data-testid="chat-stub" ref={() => { surfaceProps.chat = props; }}>
      entries:{chat ? chat.entries.length : 'internal'} sources:
      {chat ? chat.contextSources.length : 'internal'} emphasis:{emphasizedContextKey ?? 'none'} model:
      {selected?.modelId ?? 'none'}
      <button type="button" onClick={() => props.onOpenResearchSource?.('https://example.com/a')}>
        Open research source
      </button>
    </div>
  ),
}));
vi.mock('./features/library/LibraryPanel', () => ({
  LibraryPanel: (props: Record<string, unknown>) => {
    surfaceProps.library = props;
    return (
      <div data-testid="library-stub">
        library panel stub
        <button
          type="button"
          onClick={() =>
            (props.onContextDragActiveChange as ((active: boolean) => void) | undefined)?.(true)
          }
        >
          Start library drag
        </button>
      </div>
    );
  },
}));
vi.mock('./features/library/LibrarySettingsPanel', () => ({
  LibrarySettingsPanel: ({ projectAvailable }: { projectAvailable: boolean }) => {
    surfaceProps.librarySettings.push(projectAvailable);
    return <div data-testid="library-settings-stub">library settings stub</div>;
  },
}));
vi.mock('./features/browser/TaskBrowserWorkspace', () => ({
  TaskBrowserWorkspace: (props: Record<string, unknown>) => {
    surfaceProps.browser = props;
    const navigationRequest = props.navigationRequest as {
      id: number;
      onResult?: (outcome: 'opened') => void;
    } | undefined;
    useEffect(() => {
      navigationRequest?.onResult?.('opened');
    }, [navigationRequest]);
    const chatProps = props.chatProps as { chat?: { entries: unknown[]; contextSources: unknown[] } };
    return <div data-testid="browser-stub">
      browser panel stub suspended:{String(props.suspended)}
      <button
        type="button"
        onClick={() =>
          (props.onOverlaySafeChange as ((safe: boolean) => void) | undefined)?.(true)
        }
      >
        Confirm native Browser is safe
      </button>
      <button
        type="button"
        onClick={() =>
          (props.onOverlaySafeChange as ((safe: boolean) => void) | undefined)?.(false)
        }
      >
        Report native Browser is unsafe
      </button>
      <div data-testid="chat-stub">entries:{chatProps.chat?.entries.length ?? 0} sources:{chatProps.chat?.contextSources.length ?? 0}</div>
    </div>;
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
  await userEvent.click(screen.getByRole('button', { name: 'Enter path instead' }));
  await userEvent.type(screen.getByLabelText('Project path'), path);
  await userEvent.click(screen.getByRole('button', { name: 'Open' }));
}

it('opens project selection as workspace content instead of an overlay', async () => {
  api.listSessions.mockResolvedValue({ sessions: [] });
  api.homeSession.mockResolvedValue({ session: summary('s_home'.padEnd(34, 'h'), 'Home') });
  render(<App />);
  await userEvent.click(screen.getByRole('button', { name: /^Open (a )?project$/ }));

  expect(screen.queryByRole('dialog', { name: 'Open a project' })).not.toBeInTheDocument();
  expect(screen.getByRole('region', { name: 'Open a project' })).toBeVisible();
});

describe('App project switching (D63B)', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    api.sessionStorageUsage.mockResolvedValue({
      usedBytes: 1,
      warnBytes: 90,
      capBytes: 100,
    });
    api.openRoot.current = '';
    surfaceProps.library = null;
    surfaceProps.librarySettings = [];
    surfaceProps.inspector = null;
    surfaceProps.browser = null;
    surfaceProps.chat = null;
    selectedModelControl.select = null;
    api.openProject.mockImplementation((path: string) => {
      api.openRoot.current = path;
      return Promise.resolve(meta(path));
    });
    api.closeProject.mockResolvedValue(undefined);
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
    // Phase 1A: local startup resolves the backend-owned Home conversation.
    api.homeSession.mockImplementation(() =>
      Promise.resolve({ session: summary('s_home'.padEnd(34, 'h'), 'Home') }),
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

  it('reopening the same path remounts the fresh backend project generation', async () => {
    render(<App />);

    await openProjectViaModal('/proj/alpha');
    await userEvent.click(screen.getByRole('button', { name: 'Library' }));
    expect(screen.getByTestId('library-stub')).toBeInTheDocument();

    api.openProject.mockImplementationOnce((path: string) => {
      api.openRoot.current = path;
      return Promise.resolve({ ...meta(path), id: 'project-alpha-reopened' });
    });
    await openProjectViaModal('/proj/alpha');

    await waitFor(() => expect(screen.getByTestId('chat-stub')).toBeInTheDocument());
    expect(screen.queryByTestId('library-stub')).not.toBeInTheDocument();
  });

  it('keeps a successful queued open when the following close fails', async () => {
    render(<App />);
    await openProjectViaModal('/proj/alpha');
    let finishOpen!: () => void;
    api.openProject.mockImplementationOnce(() => new Promise<ProjectMeta>((resolve) => {
      finishOpen = () => resolve(meta('/proj/beta'));
    }));
    api.closeProject.mockRejectedValueOnce({
      kind: 'Internal',
      details: 'native Browser teardown failed',
    });
    await openProjectViaModal('/proj/beta');

    await userEvent.click(screen.getByRole('button', { name: 'Project actions for alpha' }));
    await userEvent.click(screen.getByRole('menuitem', { name: 'Close project' }));

    expect(api.closeProject).not.toHaveBeenCalled();
    await act(async () => finishOpen());
    await waitFor(() => expect(api.closeProject).toHaveBeenCalledOnce());
    expect(await screen.findByRole('alert')).toHaveTextContent(
      'Internal error: native Browser teardown failed',
    );
    expect(screen.getByRole('button', { name: 'Project actions for beta' })).toBeInTheDocument();
  });

  it('keeps a successful queued close when the following open fails', async () => {
    let finishClose!: () => void;
    api.closeProject.mockImplementationOnce(() => new Promise<void>((resolve) => {
      finishClose = resolve;
    }));
    render(<App />);
    await openProjectViaModal('/proj/alpha');
    api.openProject.mockRejectedValueOnce({ kind: 'Internal', details: 'beta open failed' });

    await userEvent.click(screen.getByRole('button', { name: 'Project actions for alpha' }));
    await userEvent.click(screen.getByRole('menuitem', { name: 'Close project' }));
    await openProjectViaModal('/proj/beta');

    expect(api.openProject).toHaveBeenCalledTimes(1);
    await act(async () => finishClose());
    await waitFor(() => expect(api.openProject).toHaveBeenCalledWith('/proj/beta'));
    expect(await screen.findByRole('alert')).toHaveTextContent(
      'Internal error: beta open failed',
    );
    expect(screen.queryByRole('button', { name: /Project actions for/ })).not.toBeInTheDocument();
  });

  it('keeps successful trust when the following queued close fails', async () => {
    api.openProject.mockImplementationOnce((path: string) =>
      Promise.resolve({ ...meta(path), trust: 'unknown' }));
    let finishTrust!: () => void;
    api.trustProject.mockImplementationOnce((root: string) =>
      new Promise<ProjectMeta>((resolve) => {
        finishTrust = () => resolve(meta(root));
      }));
    api.closeProject.mockRejectedValueOnce({
      kind: 'Internal',
      details: 'native Browser teardown failed',
    });
    render(<App />);
    await openProjectViaModal('/proj/alpha');

    await userEvent.click(screen.getByRole('button', { name: 'Trust and open' }));
    await userEvent.click(screen.getByRole('button', { name: 'Cancel' }));

    expect(api.closeProject).not.toHaveBeenCalled();
    await act(async () => finishTrust());
    expect(await screen.findByRole('alert')).toHaveTextContent(
      'Internal error: native Browser teardown failed',
    );
    expect(screen.getByRole('button', { name: 'Project actions for alpha' })).toBeInTheDocument();
  });

  it('keeps the catalog API with the app-level selection and MLX handle owners', () => {
    const { result } = renderHook(() => useWindowModelState());

    expect(result.current.modelCatalog).toBe(modelCatalogControl.api);
    expect(result.current.mlxServers.handleOf('qwen-coder-1.5b-mlx-4bit')).toBeNull();
    expect(result.current.selectedModel.selected).toBeNull();
  });

  it('keeps one selected model when the window switches from local chat to a project', async () => {
    render(<App />);
    await waitFor(() => expect(selectedModelControl.select).not.toBeNull());

    act(() => {
      selectedModelControl.select?.({
        providerId: 'mlx-lm',
        providerDisplayName: 'Qwen Coder',
        modelId: 'qwen-coder-1.5b-mlx-4bit',
      });
    });
    expect(screen.getByTestId('chat-stub')).toHaveTextContent('model:qwen-coder-1.5b-mlx-4bit');

    await openProjectViaModal('/proj/alpha');
    await waitFor(() =>
      expect(screen.getByTestId('chat-stub')).toHaveTextContent('model:qwen-coder-1.5b-mlx-4bit'),
    );
  });

  it('opens the Library workspace for the exact trusted project', async () => {
    render(<App />);

    await openProjectViaModal('/proj/alpha');
    await userEvent.click(screen.getByRole('button', { name: 'Library' }));

    expect(screen.getByTestId('library-stub')).toBeInTheDocument();
    expect(surfaceProps.library?.projectIdentity).toBe('/proj/alpha');
    expect(document.querySelector('.plume-unified-subtitle')).toHaveTextContent('Library');
    expect(screen.queryByTestId('chat-stub')).not.toBeInTheDocument();

    await userEvent.click(screen.getByRole('button', { name: 'Start library drag' }));
    expect(screen.getByText('Drop into chat')).toBeInTheDocument();
  });

  it('opens the app-private Library without a project', async () => {
    render(<App />);

    await userEvent.click(screen.getByRole('button', { name: 'Library' }));

    expect(screen.getByTestId('library-stub')).toBeInTheDocument();
    expect(surfaceProps.library?.projectIdentity).toBeNull();
    expect(document.querySelector('.plume-unified-subtitle')).toHaveTextContent('Library');
  });

  it('opens useful local Help from the sidebar', async () => {
    render(<App />);

    const help = screen.getByRole('button', { name: 'Help' });
    await userEvent.click(help);
    expect(screen.getByRole('dialog', { name: 'Help' })).toBeInTheDocument();
    expect(screen.getByRole('heading', { name: 'Chat or Project?' })).toBeInTheDocument();
    expect(screen.getByText(/Chat answers/)).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Open handbook' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Close help' })).toHaveFocus();

    await userEvent.keyboard('{Escape}');
    expect(screen.queryByRole('dialog', { name: 'Help' })).not.toBeInTheDocument();
    expect(help).toHaveFocus();
  });

  // Phase 1A: local chat is Home, which already exists, so the Browser attaches
  // to it rather than triggering a lazy chat creation.
  it('opens its task-owned Browser against the durable Home chat', async () => {
    render(<App />);

    await userEvent.click(screen.getByRole('button', { name: 'Open workspace views' }));
    await userEvent.click(screen.getByRole('button', { name: 'Browser' }));

    await waitFor(() => expect(screen.getByTestId('browser-stub')).toBeInTheDocument());
    expect(api.openProject).not.toHaveBeenCalled();
    expect(api.createSession).not.toHaveBeenCalled();
    expect(surfaceProps.browser?.identity).toMatchObject({ scope: 'local' });
    expect(surfaceProps.browser?.onUseInChat).toBeTypeOf('function');
    expect(document.querySelector('.plume-unified-subtitle')).toHaveTextContent('Home');
  });

  it('waits for confirmed native Browser safety before opening HTML overlays', async () => {
    render(<App />);

    await userEvent.click(screen.getByRole('button', { name: 'Open workspace views' }));
    await userEvent.click(screen.getByRole('button', { name: 'Browser' }));
    await waitFor(() => expect(screen.getByTestId('browser-stub')).toBeInTheDocument());

    await userEvent.click(screen.getByRole('button', { name: 'Open workspace views' }));
    expect(screen.queryByRole('heading', { name: 'Workspace views' })).not.toBeInTheDocument();
    expect(screen.getByTestId('browser-stub')).toHaveTextContent('suspended:true');
    await userEvent.click(screen.getByRole('button', { name: 'Confirm native Browser is safe' }));
    await waitFor(() =>
      expect(screen.getByRole('heading', { name: 'Workspace views' })).toBeInTheDocument(),
    );
    expect(screen.getByTestId('browser-stub')).toBeInTheDocument();
    await userEvent.click(screen.getAllByRole('button', { name: 'Close workspace views' }).at(-1)!);
    expect(screen.getByTestId('browser-stub')).toBeInTheDocument();
    await userEvent.click(screen.getByRole('button', { name: 'Report native Browser is unsafe' }));

    await userEvent.click(screen.getByRole('button', { name: 'Settings' }));
    expect(screen.queryByRole('dialog', { name: 'Settings' })).not.toBeInTheDocument();
    await userEvent.click(screen.getByRole('button', { name: 'Confirm native Browser is safe' }));
    await waitFor(() =>
      expect(screen.getByRole('dialog', { name: 'Settings' })).toBeInTheDocument(),
    );
    expect(screen.getByTestId('browser-stub')).toBeInTheDocument();
    await userEvent.click(screen.getByRole('button', { name: 'Report native Browser is unsafe' }));
    expect(screen.getByRole('dialog', { name: 'Settings' })).toBeInTheDocument();
    await userEvent.click(screen.getByRole('button', { name: 'Close settings' }));
    expect(screen.getByTestId('browser-stub')).toBeInTheDocument();

    await userEvent.click(screen.getByRole('button', { name: 'Help' }));
    expect(screen.queryByRole('dialog', { name: 'Help' })).not.toBeInTheDocument();
    await userEvent.click(screen.getByRole('button', { name: 'Confirm native Browser is safe' }));
    await waitFor(() =>
      expect(screen.getByRole('dialog', { name: 'Help' })).toBeInTheDocument(),
    );
    expect(screen.getByTestId('browser-stub')).toBeInTheDocument();
  });

  it('replaces the no-project Browser with inline model choice', async () => {
    render(<App />);
    await userEvent.click(screen.getByRole('button', { name: 'Open workspace views' }));
    await userEvent.click(screen.getByRole('button', { name: 'Browser' }));
    await waitFor(() => expect(screen.getByTestId('browser-stub')).toBeInTheDocument());

    await userEvent.click(screen.getByRole('button', { name: 'Model' }));
    expect(screen.queryByRole('dialog', { name: 'Choose a model' })).not.toBeInTheDocument();
    expect(screen.queryByTestId('browser-stub')).not.toBeInTheDocument();
    expect(screen.getByRole('region', { name: 'Choose a model' })).toBeVisible();
  });

  it('requires fresh native safety after leaving and reopening the same Browser task', async () => {
    render(<App />);

    await userEvent.click(screen.getByRole('button', { name: 'Open workspace views' }));
    await userEvent.click(screen.getByRole('button', { name: 'Browser' }));
    await waitFor(() => expect(screen.getByTestId('browser-stub')).toBeInTheDocument());

    await userEvent.click(screen.getByRole('button', { name: 'Open workspace views' }));
    await userEvent.click(screen.getByRole('button', { name: 'Confirm native Browser is safe' }));
    await userEvent.click(await screen.findByRole('button', { name: 'Library' }));
    expect(screen.queryByTestId('browser-stub')).not.toBeInTheDocument();

    await userEvent.click(screen.getByRole('button', { name: 'Open workspace views' }));
    await userEvent.click(screen.getByRole('button', { name: 'Browser' }));
    await waitFor(() => expect(screen.getByTestId('browser-stub')).toBeInTheDocument());

    await userEvent.click(screen.getByRole('button', { name: 'Settings' }));
    expect(screen.queryByRole('dialog', { name: 'Settings' })).not.toBeInTheDocument();
    await userEvent.click(screen.getByRole('button', { name: 'Confirm native Browser is safe' }));
    expect(await screen.findByRole('dialog', { name: 'Settings' })).toBeInTheDocument();
  });

  it('keeps the selected persisted local task title when Browser opens', async () => {
    api.listSessions.mockImplementation(({ scope }: { scope: string }) =>
      Promise.resolve({
        sessions:
          scope === 'local'
            ? [summary('la', 'Plan the Lisbon launch')]
            : (PROJECT_ROWS[api.openRoot.current] ?? []),
      }),
    );
    // Local startup lands on Home, so the persisted chat under test is Home.
    api.homeSession.mockResolvedValue({ session: summary('la', 'Plan the Lisbon launch') });
    render(<App />);

    await waitFor(() =>
      expect(screen.getByText('Plan the Lisbon launch')).toBeInTheDocument(),
    );
    await userEvent.click(screen.getByRole('button', { name: 'Open workspace views' }));
    await userEvent.click(screen.getByRole('button', { name: 'Browser' }));

    expect(screen.getByTestId('browser-stub')).toBeInTheDocument();
    expect(document.querySelector('.plume-unified-subtitle')).toHaveTextContent(
      'Plan the Lisbon launch',
    );
  });

  it('opens the same Browser workspace inside a trusted project', async () => {
    render(<App />);
    await openProjectViaModal('/proj/alpha');

    await userEvent.click(screen.getByRole('button', { name: 'Open workspace views' }));
    await userEvent.click(screen.getByRole('button', { name: 'Browser' }));

    expect(screen.getByTestId('browser-stub')).toBeInTheDocument();
    expect(screen.getByTestId('chat-stub')).toBeInTheDocument();
    expect(surfaceProps.browser?.onUseInChat).toBeTypeOf('function');
    expect(document.querySelector('.plume-unified-subtitle')).toHaveTextContent(
      'Alpha planning chat',
    );
  });

  it('owns source navigation by chat and clears it for a normal Browser open', async () => {
    api.listSessions.mockImplementation(({ scope }: { scope: string }) =>
      Promise.resolve({
        sessions: scope === 'local' ? [] : [
          summary('pa', 'Alpha planning chat'),
          summary('pa2', 'Second chat'),
        ],
      }),
    );
    render(<App />);
    await openProjectViaModal('/proj/alpha');
    await waitFor(() => expect(screen.getAllByText('Alpha planning chat').length).toBeGreaterThan(0));
    await waitFor(() => expect(surfaceProps.chat?.onOpenResearchSource).toBeTypeOf('function'));

    await userEvent.click(screen.getByRole('button', { name: 'Open research source' }));
    await waitFor(() => expect(screen.getByTestId('browser-stub')).toBeInTheDocument());
    expect(surfaceProps.browser?.navigationRequest).toEqual(expect.objectContaining({
      id: 1,
      identity: surfaceProps.browser?.identity,
      url: 'https://example.com/a',
    }));

    await userEvent.click(screen.getByRole('button', { name: /^Second chat/ }));
    await userEvent.click(screen.getByRole('button', { name: 'Open workspace views' }));
    await userEvent.click(screen.getByRole('button', { name: 'Browser' }));
    await waitFor(() => expect(screen.getByTestId('browser-stub')).toBeInTheDocument());
    expect(surfaceProps.browser?.identity).toMatchObject({ sessionId: 'pa2' });
    expect(surfaceProps.browser?.navigationRequest).toBeUndefined();
  });

  it('requires fresh native Browser safety for trusted-project overlays after reopening', async () => {
    render(<App />);
    await openProjectViaModal('/proj/alpha');

    await userEvent.click(screen.getByRole('button', { name: 'Open workspace views' }));
    await userEvent.click(screen.getByRole('button', { name: 'Browser' }));
    await waitFor(() => expect(screen.getByTestId('browser-stub')).toBeInTheDocument());
    const firstIdentity = surfaceProps.browser?.identity;
    expect(firstIdentity).toMatchObject({ scope: 'project' });

    await userEvent.click(screen.getByRole('button', { name: 'Open workspace views' }));
    expect(screen.queryByRole('heading', { name: 'Workspace views' })).not.toBeInTheDocument();
    await userEvent.click(screen.getByRole('button', { name: 'Confirm native Browser is safe' }));
    await userEvent.click(await screen.findByRole('button', { name: /^Alpha planning chat/ }));
    expect(screen.queryByTestId('browser-stub')).not.toBeInTheDocument();

    await userEvent.click(screen.getByRole('button', { name: 'Open workspace views' }));
    await userEvent.click(screen.getByRole('button', { name: 'Browser' }));
    await waitFor(() => expect(screen.getByTestId('browser-stub')).toBeInTheDocument());
    expect(surfaceProps.browser?.identity).toEqual(firstIdentity);

    await userEvent.click(screen.getByRole('button', { name: 'Settings' }));
    expect(screen.queryByRole('dialog', { name: 'Settings' })).not.toBeInTheDocument();
    await userEvent.click(screen.getByRole('button', { name: 'Confirm native Browser is safe' }));
    expect(await screen.findByRole('dialog', { name: 'Settings' })).toBeInTheDocument();
  });

  it('replaces the project Browser with inline model choice', async () => {
    render(<App />);
    await openProjectViaModal('/proj/alpha');
    await userEvent.click(screen.getByRole('button', { name: 'Open workspace views' }));
    await userEvent.click(screen.getByRole('button', { name: 'Browser' }));
    await waitFor(() => expect(screen.getByTestId('browser-stub')).toBeInTheDocument());

    await userEvent.click(screen.getByRole('button', { name: 'Model' }));
    expect(screen.queryByRole('dialog', { name: 'Choose a model' })).not.toBeInTheDocument();
    expect(screen.queryByTestId('browser-stub')).not.toBeInTheDocument();
    expect(screen.getByRole('region', { name: 'Choose a model' })).toBeVisible();
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

    await userEvent.click(screen.getByRole('button', { name: 'New chat' }));
    await userEvent.click(screen.getByRole('button', { name: 'Project' }));
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

    expect(screen.getByRole('heading', { name: 'Open alpha?' })).toBeInTheDocument();
    expect(screen.queryByText('Project safety')).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Open Browser' })).not.toBeInTheDocument();
    expect(screen.queryByTestId('browser-stub')).not.toBeInTheDocument();
  });

  it('translates each Library item into its exact project-chat source kind', async () => {
    render(<App />);
    await openProjectViaModal('/proj/alpha');
    await userEvent.click(screen.getByRole('button', { name: 'Library' }));

    const onUseInChat = surfaceProps.library?.onUseInChat as
      | ((item: unknown) => Promise<string>)
      | undefined;
    expect(onUseInChat).toBeTypeOf('function');
    await act(async () => {
      expect(await onUseInChat?.({ kind: 'userMemory', entryId: `m_${'a'.repeat(32)}` }))
        .toBe('added');
      expect(await onUseInChat?.({ kind: 'projectMemory', entryId: `m_${'b'.repeat(32)}` }))
        .toBe('added');
      expect(await onUseInChat?.({ kind: 'topic', name: 'topics/plume.md' }))
        .toBe('added');
    });
    expect(screen.getByTestId('chat-stub')).toHaveTextContent('sources:3');
    expect(screen.getByTestId('chat-stub')).toHaveTextContent(
      'emphasis:topic:topics/plume.md',
    );
  });

  it('keeps app-private user memory in local chat and rejects forged project items without a project', async () => {
    render(<App />);
    await userEvent.click(screen.getByRole('button', { name: 'Library' }));
    const onUseInChat = surfaceProps.library?.onUseInChat as
      | ((item: unknown) => Promise<string>)
      | undefined;

    await act(async () => {
      expect(await onUseInChat?.({ kind: 'userMemory', entryId: `m_${'c'.repeat(32)}` }))
        .toBe('added');
      expect(await onUseInChat?.({ kind: 'projectMemory', entryId: `m_${'d'.repeat(32)}` }))
        .toBe('unavailable');
      expect(await onUseInChat?.({ kind: 'topic', name: 'topics/private.md' }))
        .toBe('unavailable');
    });

    // Phase 1A: Home already exists, so nothing is lazily created here.
    expect(api.createSession).not.toHaveBeenCalled();
    expect(screen.getByTestId('chat-stub')).toHaveTextContent('sources:1');
  });

  it('routes project-only Library items away from an active local chat', async () => {
    render(<App />);
    await openProjectViaModal('/proj/alpha');
    await userEvent.click(screen.getByRole('button', { name: 'New chat' }));
    await userEvent.click(screen.getByRole('button', { name: 'Chat' }));
    await waitFor(() => expect(api.createSession).toHaveBeenCalledWith({ scope: 'local' }));
    await userEvent.click(screen.getByRole('button', { name: 'Library' }));
    const onUseInChat = surfaceProps.library?.onUseInChat as
      (item: unknown) => Promise<string>;

    await act(async () => {
      expect(await onUseInChat({ kind: 'projectMemory', entryId: `m_${'e'.repeat(32)}` }))
        .toBe('added');
    });

    expect(screen.getByRole('region', { name: 'Project chat' })).toBeInTheDocument();
    expect(screen.getByTestId('chat-stub')).toHaveTextContent('sources:1');
  });

  it('mounts Library settings for app-private and project memory in both shells', async () => {
    render(<App />);
    await userEvent.click(screen.getByRole('button', { name: 'Settings' }));
    expect(screen.getAllByTestId('library-settings-stub')).toHaveLength(2);
    expect(surfaceProps.librarySettings.at(-1)).toBe(false);
    await userEvent.click(screen.getByRole('button', { name: 'Close settings' }));

    await openProjectViaModal('/proj/alpha');
    await userEvent.click(screen.getByRole('button', { name: 'Settings' }));
    expect(screen.getAllByTestId('library-settings-stub')).toHaveLength(2);
    expect(surfaceProps.librarySettings.at(-1)).toBe(true);
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
    expect(screen.getByText('Drop into chat')).toBeInTheDocument();
  });
});
