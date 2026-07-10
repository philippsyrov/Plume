// D63B: list-state behavior over mocked `sessions.*` IPC. The rules
// under test: scopes never mix, mutations are database-first (state
// changes only after the IPC resolves), and failures leave rows
// visible with the error surfaced.

import { act, renderHook, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import type { SessionSummary } from '../../lib/api/sessions';
import { useSessions } from './useSessions';

const api = vi.hoisted(() => ({
  listSessions: vi.fn(),
  createSession: vi.fn(),
  renameSession: vi.fn(),
  archiveSession: vi.fn(),
  deleteSession: vi.fn(),
}));

vi.mock('../../lib/api/sessions', () => ({
  listSessions: api.listSessions,
  createSession: api.createSession,
  renameSession: api.renameSession,
  archiveSession: api.archiveSession,
  deleteSession: api.deleteSession,
}));

function summary(id: string, title: string, updatedAtMs: number, archived = false): SessionSummary {
  return {
    id,
    title,
    createdAtMs: 1,
    updatedAtMs,
    archivedAtMs: archived ? updatedAtMs : null,
  };
}

describe('useSessions', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    api.listSessions.mockImplementation(({ scope }: { scope: string }) =>
      Promise.resolve({
        sessions:
          scope === 'local'
            ? [summary('l2', 'local two', 20), summary('l1', 'local one', 10)]
            : [summary('p1', 'project one', 30)],
      }),
    );
  });

  it('keeps the two scopes as separate lists', async () => {
    const { result } = renderHook(() => useSessions({ projectAvailable: true }));
    await waitFor(() => expect(result.current.local.status).toBe('ready'));
    await waitFor(() => expect(result.current.project.status).toBe('ready'));

    expect(result.current.visibleOf('local').map((s) => s.id)).toEqual(['l2', 'l1']);
    expect(result.current.visibleOf('project').map((s) => s.id)).toEqual(['p1']);
  });

  it('never queries the project scope without a project', async () => {
    const { result } = renderHook(() => useSessions({ projectAvailable: false }));
    await waitFor(() => expect(result.current.local.status).toBe('ready'));

    const scopes = api.listSessions.mock.calls.map(([payload]) => payload.scope);
    expect(scopes).toEqual(['local']);
  });

  it('create hits the right scope and prepends the new session', async () => {
    api.createSession.mockResolvedValue({ session: summary('l3', 'New chat', 40) });
    const { result } = renderHook(() => useSessions({ projectAvailable: true }));
    await waitFor(() => expect(result.current.local.status).toBe('ready'));

    let created: SessionSummary | null = null;
    await act(async () => {
      created = await result.current.create('local');
    });
    expect(api.createSession).toHaveBeenCalledWith({ scope: 'local' });
    expect(created).not.toBeNull();
    expect(result.current.visibleOf('local')[0]?.id).toBe('l3');

    api.createSession.mockResolvedValue({ session: summary('p2', 'New chat', 50) });
    await act(async () => {
      await result.current.create('project');
    });
    expect(api.createSession).toHaveBeenLastCalledWith({ scope: 'project' });
    expect(result.current.visibleOf('project')[0]?.id).toBe('p2');
  });

  it('rename is database-first: the row updates only after the IPC resolves', async () => {
    let resolveRename: (value: { session: SessionSummary }) => void = () => undefined;
    api.renameSession.mockReturnValue(
      new Promise((resolve) => {
        resolveRename = resolve;
      }),
    );
    const { result } = renderHook(() => useSessions({ projectAvailable: true }));
    await waitFor(() => expect(result.current.local.status).toBe('ready'));

    let done: Promise<unknown> | null = null;
    act(() => {
      done = result.current.rename('local', 'l1', 'Fresh title');
    });
    // In flight: the old title is still what renders.
    expect(
      result.current.visibleOf('local').find((s) => s.id === 'l1')?.title,
    ).toBe('local one');

    await act(async () => {
      resolveRename({ session: summary('l1', 'Fresh title', 99) });
      await done;
    });
    const renamed = result.current.visibleOf('local').find((s) => s.id === 'l1');
    expect(renamed?.title).toBe('Fresh title');
    // The bumped updatedAtMs moved it to the top.
    expect(result.current.visibleOf('local')[0]?.id).toBe('l1');
  });

  it('a failed mutation keeps the row and returns the error message', async () => {
    api.renameSession.mockRejectedValue(new Error('disk said no'));
    const { result } = renderHook(() => useSessions({ projectAvailable: true }));
    await waitFor(() => expect(result.current.local.status).toBe('ready'));

    let outcome: { ok: boolean } = { ok: true };
    await act(async () => {
      outcome = await result.current.rename('local', 'l1', 'nope');
    });
    expect(outcome).toEqual({ ok: false, message: 'disk said no' });
    expect(
      result.current.visibleOf('local').find((s) => s.id === 'l1')?.title,
    ).toBe('local one');
    expect(result.current.lastMutationError).toBe('disk said no');
  });

  it('archive hides a row from the visible list and unarchive restores it', async () => {
    api.archiveSession.mockResolvedValue({ session: summary('l1', 'local one', 10, true) });
    const { result } = renderHook(() => useSessions({ projectAvailable: true }));
    await waitFor(() => expect(result.current.local.status).toBe('ready'));

    await act(async () => {
      await result.current.setArchived('local', 'l1', true);
    });
    expect(result.current.visibleOf('local').map((s) => s.id)).toEqual(['l2']);
    expect(result.current.archivedOf('local').map((s) => s.id)).toEqual(['l1']);

    api.archiveSession.mockResolvedValue({ session: summary('l1', 'local one', 10) });
    await act(async () => {
      await result.current.setArchived('local', 'l1', false);
    });
    expect(result.current.visibleOf('local').map((s) => s.id)).toEqual(['l2', 'l1']);
    expect(result.current.archivedOf('local')).toHaveLength(0);
  });

  // D65: derived-title renames. The default-title gate lives at the
  // call site (usePersistedChat checks the fresh save response);
  // autoRename adds the never-overwrite-a-user-title guards.
  describe('autoRename', () => {
    it('renames a still-default session and folds the result in', async () => {
      api.listSessions.mockImplementation(({ scope }: { scope: string }) =>
        Promise.resolve({
          sessions: scope === 'local' ? [summary('l1', 'New chat', 10)] : [],
        }),
      );
      api.renameSession.mockResolvedValue({ session: summary('l1', 'derived title', 99) });
      const { result } = renderHook(() => useSessions({ projectAvailable: true }));
      await waitFor(() => expect(result.current.local.status).toBe('ready'));

      await act(async () => {
        await result.current.autoRename('local', 'l1', 'derived title');
      });
      expect(api.renameSession).toHaveBeenCalledWith({
        scope: 'local',
        sessionId: 'l1',
        title: 'derived title',
      });
      expect(
        result.current.visibleOf('local').find((s) => s.id === 'l1')?.title,
      ).toBe('derived title');
    });

    it('never overwrites a listed non-default title', async () => {
      const { result } = renderHook(() => useSessions({ projectAvailable: true }));
      await waitFor(() => expect(result.current.local.status).toBe('ready'));

      // l1 is listed as 'local one' — user-titled in a previous launch.
      await act(async () => {
        await result.current.autoRename('local', 'l1', 'derived title');
      });
      expect(api.renameSession).not.toHaveBeenCalled();
    });

    it('never fires for a session the user renamed this window, even back to the default', async () => {
      api.listSessions.mockImplementation(({ scope }: { scope: string }) =>
        Promise.resolve({
          sessions: scope === 'local' ? [summary('l1', 'New chat', 10)] : [],
        }),
      );
      api.renameSession.mockResolvedValue({ session: summary('l1', 'New chat', 50) });
      const { result } = renderHook(() => useSessions({ projectAvailable: true }));
      await waitFor(() => expect(result.current.local.status).toBe('ready'));

      // The user deliberately names the chat 'New chat'. The manual
      // claim is what protects it — the title check alone would let
      // the auto-rename through.
      await act(async () => {
        await result.current.rename('local', 'l1', 'New chat');
      });
      expect(api.renameSession).toHaveBeenCalledTimes(1);

      await act(async () => {
        await result.current.autoRename('local', 'l1', 'derived title');
      });
      expect(api.renameSession).toHaveBeenCalledTimes(1);
    });

    it('proceeds for a session not yet flushed into the list (lazy create)', async () => {
      api.renameSession.mockResolvedValue({
        session: summary('lazy-new', 'derived title', 99),
      });
      const { result } = renderHook(() => useSessions({ projectAvailable: true }));
      await waitFor(() => expect(result.current.local.status).toBe('ready'));

      // 'lazy-new' is absent from the list — the queued save created
      // it and React state has not flushed. Absence must NOT skip;
      // the caller has already verified the default title on the
      // fresh backend summary.
      await act(async () => {
        await result.current.autoRename('local', 'lazy-new', 'derived title');
      });
      expect(api.renameSession).toHaveBeenCalledWith({
        scope: 'local',
        sessionId: 'lazy-new',
        title: 'derived title',
      });
    });

    it('a failed auto-rename is logged, not surfaced as a mutation error', async () => {
      api.listSessions.mockImplementation(({ scope }: { scope: string }) =>
        Promise.resolve({
          sessions: scope === 'local' ? [summary('l1', 'New chat', 10)] : [],
        }),
      );
      api.renameSession.mockRejectedValue(new Error('disk said no'));
      const { result } = renderHook(() => useSessions({ projectAvailable: true }));
      await waitFor(() => expect(result.current.local.status).toBe('ready'));

      await act(async () => {
        await result.current.autoRename('local', 'l1', 'derived title');
      });
      expect(result.current.lastMutationError).toBeNull();
      expect(
        result.current.visibleOf('local').find((s) => s.id === 'l1')?.title,
      ).toBe('New chat');
    });
  });

  it('delete removes the row on success and keeps it on failure', async () => {
    api.deleteSession.mockResolvedValue({ ok: true });
    const { result } = renderHook(() => useSessions({ projectAvailable: true }));
    await waitFor(() => expect(result.current.local.status).toBe('ready'));

    await act(async () => {
      await result.current.remove('local', 'l1');
    });
    expect(result.current.visibleOf('local').map((s) => s.id)).toEqual(['l2']);

    api.deleteSession.mockRejectedValue(new Error('still streaming somewhere'));
    let outcome: { ok: boolean } = { ok: true };
    await act(async () => {
      outcome = await result.current.remove('local', 'l2');
    });
    expect(outcome.ok).toBe(false);
    expect(result.current.visibleOf('local').map((s) => s.id)).toEqual(['l2']);
  });
});
