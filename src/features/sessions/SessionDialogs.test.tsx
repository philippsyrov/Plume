// D63B: dialog behavior — rename with inline failure announcement,
// explicit delete confirmation (blocked while the chat streams), and
// the archived-chats modal's unarchive / two-step delete. No native
// dialogs anywhere: everything is Plume-styled DOM.

import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';

import type { SessionScope, SessionSummary } from '../../lib/api/sessions';
import { useSessionDialogs } from './SessionDialogs';
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
      status: 'idle',
      lastError: null,
      activeStreamId: null,
      lastInstructionsIncluded: null,
      lastMemoryUsed: null,
      lastTopicsUsed: null,
      send: vi.fn().mockResolvedValue('accepted'),
      cancel: vi.fn().mockResolvedValue(undefined),
      clear: vi.fn(),
      restore: vi.fn(),
    },
    activeScope: 'local',
    activeSessionId: null,
    notice: null,
    saveError: null,
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
}: {
  sessions: SessionsApi;
  persisted: PersistedChatApi;
  scope: SessionScope;
  session: SessionSummary;
}) {
  const dialogs = useSessionDialogs({ sessions, persisted });
  return (
    <>
      <button type="button" onClick={() => dialogs.openRename(scope, session)}>
        harness-rename
      </button>
      <button type="button" onClick={() => dialogs.openDelete(scope, session)}>
        harness-delete
      </button>
      <button type="button" onClick={() => dialogs.openArchived(scope)}>
        harness-archived
      </button>
      <button type="button" onClick={() => dialogs.openRewind(scope, session)}>
        harness-rewind
      </button>
      {dialogs.node}
    </>
  );
}

describe('session dialogs', () => {
  it('rewind defaults to one turn and submits the exact scope and session', async () => {
    const persisted = makePersisted();
    render(<Harness sessions={makeSessionsApi()} persisted={persisted} scope="project" session={summary('p1', 'Source')} />);
    await userEvent.click(screen.getByText('harness-rewind'));

    expect(screen.getByRole('dialog')).toHaveTextContent('source chat stays unchanged');
    expect(screen.getByLabelText('User turns to omit')).toHaveValue(1);
    await userEvent.click(screen.getByRole('button', { name: 'Rewind' }));

    expect(persisted.rewindInNewChat).toHaveBeenCalledWith('project', 'p1', 1);
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
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

  it('archived modal refuses to delete the actively-streaming chat (Codex P2)', async () => {
    const archived = summary('l9', 'Streaming shelved chat', true);
    const sessions = makeSessionsApi({ archivedOf: () => [archived] });
    // Archiving never unloads the surface, so the archived chat can
    // still be the one streaming right now.
    const persisted = makePersisted({ activeSessionId: 'l9' });
    persisted.chat = { ...persisted.chat, status: 'streaming' };
    render(
      <Harness sessions={sessions} persisted={persisted} scope="local" session={archived} />,
    );
    await userEvent.click(screen.getByText('harness-archived'));
    await userEvent.click(
      screen.getByRole('button', { name: 'Delete Streaming shelved chat' }),
    );

    expect(screen.getByRole('alert')).toHaveTextContent(/still streaming/);
    expect(
      screen.queryByRole('button', { name: /Confirm permanent delete/ }),
    ).not.toBeInTheDocument();
    expect(sessions.remove).not.toHaveBeenCalled();
  });

  it('archived modal unarchives and needs two clicks to delete', async () => {
    const archived = summary('l9', 'Shelved chat', true);
    const sessions = makeSessionsApi({ archivedOf: () => [archived] });
    render(
      <Harness
        sessions={sessions}
        persisted={makePersisted()}
        scope="local"
        session={archived}
      />,
    );
    await userEvent.click(screen.getByText('harness-archived'));
    expect(screen.getByRole('dialog')).toHaveTextContent('Archived chats — Chats');

    await userEvent.click(screen.getByRole('button', { name: 'Unarchive Shelved chat' }));
    expect(sessions.setArchived).toHaveBeenCalledWith('local', 'l9', false);

    await userEvent.click(screen.getByRole('button', { name: 'Delete Shelved chat' }));
    expect(sessions.remove).not.toHaveBeenCalled();
    await userEvent.click(
      screen.getByRole('button', { name: 'Confirm permanent delete of Shelved chat' }),
    );
    expect(sessions.remove).toHaveBeenCalledWith('local', 'l9');
  });
});
