// D63B: dialog behavior — rename with inline failure announcement,
// explicit delete confirmation (blocked while the chat streams), and
// Settings archive management. No native
// dialogs anywhere: everything is Plume-styled DOM.

import { fireEvent, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';

import type { SessionScope, SessionSummary } from '../../lib/api/sessions';
import { ArchivedSessionsSettings, useSessionDialogs } from './SessionDialogs';
import type { PersistedChatApi } from './usePersistedChat';
import type { MutationResult, SessionsApi } from './useSessions';

function summary(id: string, title: string, archived = false): SessionSummary {
  return { id, title, createdAtMs: 1, updatedAtMs: 2, archivedAtMs: archived ? 3 : null,
    forkedFromSessionId: null, forkedThroughEntryId: null };
}

function makeSessionsApi(overrides: Partial<SessionsApi> = {}): SessionsApi {
  return {
    local: { sessions: [], status: 'ready', error: null },
    project: { sessions: [], status: 'ready', error: null },
    visibleOf: () => [],
    archivedOf: () => [],
    refresh: vi.fn().mockResolvedValue(undefined),
    create: vi.fn().mockResolvedValue(null),
    rename: vi.fn().mockResolvedValue({ ok: true } as MutationResult),
    autoRename: vi.fn().mockResolvedValue(undefined),
    setArchived: vi.fn().mockResolvedValue({ ok: true } as MutationResult),
    remove: vi.fn().mockResolvedValue({ ok: true } as MutationResult),
    absorb: vi.fn(),
    lastMutationError: null,
    ...overrides,
  };
}

function makePersisted(overrides: Partial<PersistedChatApi> = {}): PersistedChatApi {
  return {
    chat: {
      entries: [],
      contextSources: [],
      status: 'idle',
      lastError: null,
      activeStreamId: null,
      lastInstructionsIncluded: null,
      lastMemoryUsed: null,
      lastTopicsUsed: null,
      addContextSource: vi.fn(() => 'added' as const),
      removeContextSource: vi.fn(() => true),
      appendEntries: vi.fn(),
      send: vi.fn().mockResolvedValue('accepted'),
      cancel: vi.fn().mockResolvedValue(undefined),
      clear: vi.fn(),
      restore: vi.fn(),
    },
    activeScope: 'local',
    activeSessionId: null,
    surfaceIdentity: () => ({ scope: 'local', sessionId: null }),
    notice: null,
    saveError: null,
    storageFull: false,
    storageWarning: null,
    selectSession: vi.fn().mockResolvedValue(true),
    openScope: vi.fn().mockResolvedValue(true),
    startNewSession: vi.fn().mockResolvedValue(true),
    continueInNewChat: vi.fn().mockResolvedValue(true),
    rewindInNewChat: vi.fn().mockResolvedValue(true),
    handleDeleted: vi.fn(),
    ...overrides,
  };
}

function Harness({
  sessions,
  persisted,
  scope,
  session,
  onChatCreated,
}: {
  sessions: SessionsApi;
  persisted: PersistedChatApi;
  scope: SessionScope;
  session: SessionSummary;
  onChatCreated?: (scope: SessionScope) => void;
}) {
  const dialogs = useSessionDialogs({
    sessions,
    persisted,
    ...(onChatCreated === undefined ? {} : { onChatCreated }),
  });
  return (
    <>
      <button type="button" onClick={() => dialogs.openRename(scope, session)}>
        harness-rename
      </button>
      <button type="button" onClick={() => dialogs.openDelete(scope, session)}>
        harness-delete
      </button>
      <button type="button" onClick={() => dialogs.openRewind(scope, session)}>
        harness-rewind
      </button>
      {dialogs.node}
    </>
  );
}

describe('session dialogs', () => {
  it('keeps local and project archives together in Settings', async () => {
    const local = summary('l9', 'Shelved chat', true);
    const project = summary('p9', 'Shelved project chat', true);
    const sessions = makeSessionsApi({
      archivedOf: (scope) => scope === 'local' ? [local] : [project],
    });
    render(
      <ArchivedSessionsSettings
        sessions={sessions}
        persisted={makePersisted()}
        projectAvailable
      />,
    );

    expect(screen.getByRole('heading', { name: 'Chats' })).toBeVisible();
    expect(screen.getByRole('heading', { name: 'Project chats' })).toBeVisible();
    await userEvent.click(screen.getByRole('button', { name: 'Unarchive Shelved chat' }));
    await userEvent.click(
      screen.getByRole('button', { name: 'Unarchive Shelved project chat' }),
    );
    expect(sessions.setArchived).toHaveBeenCalledWith('local', 'l9', false);
    expect(sessions.setArchived).toHaveBeenCalledWith('project', 'p9', false);
  });

  it('moves focus into Rename and loops Shift+Tab from first to last', async () => {
    const user = userEvent.setup();
    render(<Harness sessions={makeSessionsApi()} persisted={makePersisted()} scope="local" session={summary('l1', 'Source')} />);

    await user.click(screen.getByText('harness-rename'));
    expect(screen.getByRole('button', { name: 'Close rename chat' })).toHaveFocus();
    await user.keyboard('{Shift>}{Tab}{/Shift}');
    expect(screen.getByRole('button', { name: 'Save' })).toHaveFocus();
    await user.tab();
    expect(screen.getByRole('button', { name: 'Close rename chat' })).toHaveFocus();
  });

  it('moves focus into Delete and keeps Tab contained across its controls', async () => {
    const user = userEvent.setup();
    render(<Harness sessions={makeSessionsApi()} persisted={makePersisted()} scope="local" session={summary('l1', 'Source')} />);

    await user.click(screen.getByText('harness-delete'));
    expect(screen.getByRole('button', { name: 'Close delete chat' })).toHaveFocus();
    await user.keyboard('{Shift>}{Tab}{/Shift}');
    expect(screen.getByRole('button', { name: 'Delete chat Source permanently' })).toHaveFocus();
    await user.tab();
    expect(screen.getByRole('button', { name: 'Close delete chat' })).toHaveFocus();
  });

  it('leaves Rewind focus on its existing autofocus input', async () => {
    const user = userEvent.setup();
    render(<Harness sessions={makeSessionsApi()} persisted={makePersisted()} scope="local" session={summary('l1', 'Source')} />);

    await user.click(screen.getByText('harness-rewind'));

    expect(screen.getByLabelText('User turns to omit')).toHaveFocus();
  });

  it('does not steal dialog focus when a parent rerender replaces onClose', async () => {
    const user = userEvent.setup();
    const sessions = makeSessionsApi();
    const persisted = makePersisted();
    const session = summary('l1', 'Source');
    const view = render(<Harness sessions={sessions} persisted={persisted} scope="local" session={session} />);
    await user.click(screen.getByText('harness-rename'));
    const input = screen.getByLabelText('Chat title');
    await user.click(input);

    view.rerender(<Harness sessions={sessions} persisted={persisted} scope="local" session={session} />);

    expect(input).toHaveFocus();
  });

  it('rewind defaults to one turn and submits the exact scope and session', async () => {
    const persisted = makePersisted();
    const onChatCreated = vi.fn();
    render(<Harness sessions={makeSessionsApi()} persisted={persisted} scope="project" session={summary('p1', 'Source')} onChatCreated={onChatCreated} />);
    await userEvent.click(screen.getByText('harness-rewind'));

    expect(screen.getByRole('dialog')).toHaveTextContent(
      'Creates a new chat ending before the selected recent turns. The original stays unchanged.',
    );
    expect(screen.getByLabelText('User turns to omit')).toHaveValue(1);
    await userEvent.click(screen.getByRole('button', { name: 'Rewind' }));

    expect(persisted.rewindInNewChat).toHaveBeenCalledWith('project', 'p1', 1);
    expect(onChatCreated).toHaveBeenCalledWith('project');
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
  });

  it('closes with Escape or an outside press and returns focus to the opener', async () => {
    const user = userEvent.setup();
    render(<Harness sessions={makeSessionsApi()} persisted={makePersisted()} scope="local" session={summary('l1', 'Source')} />);
    const opener = screen.getByText('harness-rewind');

    await user.click(opener);
    await user.keyboard('{Escape}');
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
    expect(opener).toHaveFocus();

    await user.click(opener);
    fireEvent.mouseDown(screen.getByRole('presentation'));
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
    expect(opener).toHaveFocus();
  });

  it('rewind rejects values outside 1 through 20 and Cancel closes without calling', async () => {
    const persisted = makePersisted();
    render(<Harness sessions={makeSessionsApi()} persisted={persisted} scope="local" session={summary('l1', 'Source')} />);
    await userEvent.click(screen.getByText('harness-rewind'));
    const input = screen.getByLabelText('User turns to omit');

    await userEvent.clear(input);
    await userEvent.type(input, '0');
    expect(screen.getByRole('button', { name: 'Rewind' })).toBeDisabled();
    await userEvent.clear(input);
    await userEvent.type(input, '21');
    expect(screen.getByRole('button', { name: 'Rewind' })).toBeDisabled();
    await userEvent.click(screen.getByRole('button', { name: 'Cancel' }));

    expect(persisted.rewindInNewChat).not.toHaveBeenCalled();
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
  });

  it('keeps one rewind submission in flight and closes when the child was created', async () => {
    let finish!: (created: boolean) => void;
    const rewindInNewChat = vi.fn().mockReturnValue(new Promise<boolean>((resolve) => {
      finish = resolve;
    }));
    const persisted = makePersisted({ rewindInNewChat });
    render(<Harness sessions={makeSessionsApi()} persisted={persisted} scope="local" session={summary('l1', 'Source')} />);
    await userEvent.click(screen.getByText('harness-rewind'));
    const submit = screen.getByRole('button', { name: 'Rewind' });
    await userEvent.click(submit);
    expect(screen.getByRole('button', { name: 'Rewinding…' })).toBeDisabled();
    await userEvent.click(screen.getByRole('button', { name: 'Rewinding…' }));
    expect(rewindInNewChat).toHaveBeenCalledTimes(1);
    finish(true);
    await screen.findByText('harness-rewind');
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
  });
  it('rename submits the trimmed title and closes on success', async () => {
    const sessions = makeSessionsApi();
    render(
      <Harness
        sessions={sessions}
        persisted={makePersisted()}
        scope="local"
        session={summary('l1', 'Old title')}
      />,
    );
    await userEvent.click(screen.getByText('harness-rename'));
    const input = screen.getByLabelText(/Chat title/);
    await userEvent.clear(input);
    await userEvent.type(input, '  Fresh title  ');
    await userEvent.click(screen.getByRole('button', { name: 'Save' }));

    expect(sessions.rename).toHaveBeenCalledWith('local', 'l1', 'Fresh title');
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
  });

  it('a failed rename stays open and announces the error; the row keeps its title', async () => {
    const sessions = makeSessionsApi({
      rename: vi.fn().mockResolvedValue({ ok: false, message: 'title too long' }),
    });
    render(
      <Harness
        sessions={sessions}
        persisted={makePersisted()}
        scope="local"
        session={summary('l1', 'Old title')}
      />,
    );
    await userEvent.click(screen.getByText('harness-rename'));
    await userEvent.click(screen.getByRole('button', { name: 'Save' }));

    expect(screen.getByRole('alert')).toHaveTextContent('title too long');
    expect(screen.getByRole('dialog')).toBeInTheDocument();
  });

  it('delete requires the explicit confirmation click and reports to the bridge', async () => {
    const sessions = makeSessionsApi();
    const persisted = makePersisted();
    render(
      <Harness
        sessions={sessions}
        persisted={persisted}
        scope="project"
        session={summary('p1', 'Doomed chat')}
      />,
    );
    await userEvent.click(screen.getByText('harness-delete'));
    expect(sessions.remove).not.toHaveBeenCalled();

    await userEvent.click(
      screen.getByRole('button', { name: 'Delete chat Doomed chat permanently' }),
    );
    expect(sessions.remove).toHaveBeenCalledWith('project', 'p1');
    expect(persisted.handleDeleted).toHaveBeenCalledWith('project', 'p1');
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
  });

  it('refuses to delete the active chat while it is streaming', async () => {
    const sessions = makeSessionsApi();
    const persisted = makePersisted({ activeSessionId: 'p1' });
    persisted.chat = { ...persisted.chat, status: 'streaming' };
    render(
      <Harness
        sessions={sessions}
        persisted={persisted}
        scope="project"
        session={summary('p1', 'Streaming chat')}
      />,
    );
    await userEvent.click(screen.getByText('harness-delete'));

    expect(screen.getByRole('alert')).toHaveTextContent(/still streaming/);
    expect(
      screen.getByRole('button', { name: 'Delete chat Streaming chat permanently' }),
    ).toBeDisabled();
    expect(sessions.remove).not.toHaveBeenCalled();
  });

  it('archive settings refuse to delete the actively-streaming chat (Codex P2)', async () => {
    const archived = summary('l9', 'Streaming shelved chat', true);
    const sessions = makeSessionsApi({ archivedOf: () => [archived] });
    // Archiving never unloads the surface, so the archived chat can
    // still be the one streaming right now.
    const persisted = makePersisted({ activeSessionId: 'l9' });
    persisted.chat = { ...persisted.chat, status: 'streaming' };
    render(<ArchivedSessionsSettings sessions={sessions} persisted={persisted} projectAvailable={false} />);
    await userEvent.click(
      screen.getByRole('button', { name: 'More actions for Streaming shelved chat' }),
    );
    await userEvent.click(
      screen.getByRole('button', { name: 'Delete Streaming shelved chat' }),
    );

    expect(screen.getByRole('alert')).toHaveTextContent(/still streaming/);
    expect(
      screen.queryByRole('button', { name: /Confirm permanent delete/ }),
    ).not.toBeInTheDocument();
    expect(sessions.remove).not.toHaveBeenCalled();
  });

  it('archive settings unarchive and need two clicks to delete', async () => {
    const archived = summary('l9', 'Shelved chat', true);
    const sessions = makeSessionsApi({ archivedOf: () => [archived] });
    const view = render(
      <ArchivedSessionsSettings sessions={sessions} persisted={makePersisted()} projectAvailable={false} />,
    );
    expect(screen.getByRole('heading', { name: 'Chats' })).toBeVisible();
    expect(view.container.querySelector('time')).toHaveAttribute(
      'datetime',
      new Date(archived.updatedAtMs).toISOString(),
    );

    await userEvent.click(screen.getByRole('button', { name: 'Unarchive Shelved chat' }));
    expect(sessions.setArchived).toHaveBeenCalledWith('local', 'l9', false);

    expect(screen.queryByRole('button', { name: 'Delete Shelved chat' })).not.toBeInTheDocument();
    await userEvent.click(
      screen.getByRole('button', { name: 'More actions for Shelved chat' }),
    );
    await userEvent.click(screen.getByRole('button', { name: 'Delete Shelved chat' }));
    expect(sessions.remove).not.toHaveBeenCalled();
    await userEvent.click(
      screen.getByRole('button', { name: 'Confirm permanent delete of Shelved chat' }),
    );
    expect(sessions.remove).toHaveBeenCalledWith('local', 'l9');
  });
});
