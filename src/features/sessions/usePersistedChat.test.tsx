// D63B: stable-boundary persistence rules. `useChat` is replaced by a
// controllable fake so the tests can drive the exact entry/state
// transitions the real hook produces: accepted send (user turn +
// streaming placeholder), token frames (only the streaming entry is
// replaced), and each terminal outcome. `sessions.*` IPC is mocked at
// the api layer.

import { act, renderHook, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import type { ChatEntry } from '../chat/useChat';
import type { SessionSummary } from '../../lib/api/sessions';
import { SWITCH_BLOCKED_NOTICE, usePersistedChat } from './usePersistedChat';
import { useSessions } from './useSessions';

const api = vi.hoisted(() => ({
  listSessions: vi.fn(),
  createSession: vi.fn(),
  renameSession: vi.fn(),
  archiveSession: vi.fn(),
  deleteSession: vi.fn(),
  loadSession: vi.fn(),
  saveSessionTranscript: vi.fn(),
}));

vi.mock('../../lib/api/sessions', () => ({
  listSessions: api.listSessions,
  createSession: api.createSession,
  renameSession: api.renameSession,
  archiveSession: api.archiveSession,
  deleteSession: api.deleteSession,
  loadSession: api.loadSession,
  saveSessionTranscript: api.saveSessionTranscript,
}));

// Controllable stand-in for `useChat`: same public surface, but the
// test drives `entries`/`status` directly through `chatControl`.
const chatControl = vi.hoisted(() => ({
  setEntries: (_entries: unknown[]) => undefined as void,
  setStatus: (_status: string) => undefined as void,
}));

vi.mock('../chat/useChat', async () => {
  const { useState } = await import('react');
  return {
    useChat: () => {
      const [entries, setEntries] = useState<unknown[]>([]);
      const [status, setStatus] = useState<string>('idle');
      chatControl.setEntries = (next: unknown[]) => setEntries(next);
      chatControl.setStatus = (next: string) => setStatus(next);
      return {
        entries,
        status,
        lastError: null,
        activeStreamId: null,
        lastInstructionsIncluded: null,
        lastMemoryUsed: null,
        lastTopicsUsed: null,
        send: vi.fn().mockResolvedValue('accepted'),
        cancel: vi.fn().mockResolvedValue(undefined),
        clear: vi.fn(),
        restore: (restored: unknown[]) => {
          setEntries(restored);
          setStatus('idle');
        },
      };
    },
  };
});

function summary(id: string, title: string, updatedAtMs: number): SessionSummary {
  return { id, title, createdAtMs: 1, updatedAtMs, archivedAtMs: null };
}

const userTurn: ChatEntry = {
  kind: 'message',
  message: { role: 'user', content: 'hello' },
};
const streamingEntry: ChatEntry = {
  kind: 'streaming',
  streamId: 'chat-1',
  content: '',
  tokenCount: 0,
};

function useHarness(initialScope: 'local' | 'project') {
  const sessions = useSessions({ projectAvailable: true });
  const persisted = usePersistedChat({ sessions, initialScope });
  return { sessions, persisted };
}

async function flushQueue() {
  // Drain the serialized save queue (microtasks only, no timers).
  await act(async () => {
    await Promise.resolve();
    await Promise.resolve();
    await Promise.resolve();
  });
}

describe('usePersistedChat', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    api.listSessions.mockImplementation(({ scope }: { scope: string }) =>
      Promise.resolve({
        sessions:
          scope === 'local'
            ? [summary('l2', 'newest local', 20), summary('l1', 'older local', 10)]
            : [],
      }),
    );
    api.loadSession.mockResolvedValue({
      session: { ...summary('l2', 'newest local', 20), entries: [] },
    });
    api.saveSessionTranscript.mockImplementation(({ sessionId }: { sessionId: string }) =>
      Promise.resolve({ session: summary(sessionId, 'saved', 99) }),
    );
    api.createSession.mockResolvedValue({ session: summary('fresh', 'New chat', 50) });
  });

  it('relaunch: restores the most recently updated session of the initial scope only', async () => {
    api.loadSession.mockResolvedValue({
      session: {
        ...summary('l2', 'newest local', 20),
        entries: [
          { kind: 'message', message: { role: 'user', content: 'stored q' } },
          { kind: 'message', message: { role: 'assistant', content: 'stored a' } },
        ],
      },
    });
    const { result } = renderHook(() => useHarness('local'));

    await waitFor(() =>
      expect(api.loadSession).toHaveBeenCalledWith({ scope: 'local', sessionId: 'l2' }),
    );
    await waitFor(() => expect(result.current.persisted.activeSessionId).toBe('l2'));
    expect(result.current.persisted.chat.entries).toHaveLength(2);
    // The restored snapshot is NOT saved back.
    await flushQueue();
    expect(api.saveSessionTranscript).not.toHaveBeenCalled();
    // The other scope's transcripts were never touched.
    const loadScopes = api.loadSession.mock.calls.map(([p]) => p.scope);
    expect(loadScopes).toEqual(['local']);
  });

  it('saves on the accepted user turn, never on token frames, again on the terminal turn', async () => {
    const { result } = renderHook(() => useHarness('local'));
    await waitFor(() => expect(result.current.persisted.activeSessionId).toBe('l2'));

    // Accepted send: user turn + streaming placeholder appear.
    act(() => {
      chatControl.setStatus('streaming');
      chatControl.setEntries([userTurn, streamingEntry]);
    });
    await flushQueue();
    expect(api.saveSessionTranscript).toHaveBeenCalledTimes(1);
    expect(api.saveSessionTranscript).toHaveBeenLastCalledWith({
      scope: 'local',
      sessionId: 'l2',
      entries: [{ kind: 'message', message: { role: 'user', content: 'hello' } }],
    });

    // Twenty token frames: only the streaming entry is replaced.
    for (let i = 1; i <= 20; i += 1) {
      act(() => {
        chatControl.setEntries([
          userTurn,
          { ...streamingEntry, content: 'x'.repeat(i), tokenCount: i },
        ]);
      });
    }
    await flushQueue();
    expect(api.saveSessionTranscript).toHaveBeenCalledTimes(1);

    // Terminal: streaming flips to a message.
    act(() => {
      chatControl.setStatus('idle');
      chatControl.setEntries([
        userTurn,
        { kind: 'message', message: { role: 'assistant', content: 'x'.repeat(20) } },
      ]);
    });
    await flushQueue();
    expect(api.saveSessionTranscript).toHaveBeenCalledTimes(2);
    const [lastCall] = api.saveSessionTranscript.mock.calls.slice(-1);
    expect(lastCall[0].entries).toHaveLength(2);
  });

  it('saves the stopped and errored terminal shapes too', async () => {
    const { result } = renderHook(() => useHarness('local'));
    await waitFor(() => expect(result.current.persisted.activeSessionId).toBe('l2'));

    act(() => {
      chatControl.setEntries([userTurn, { kind: 'cancelled', partial: 'half' }]);
    });
    await flushQueue();
    let [call] = api.saveSessionTranscript.mock.calls.slice(-1);
    expect(call[0].entries[1]).toEqual({ kind: 'cancelled', partial: 'half' });

    act(() => {
      chatControl.setEntries([
        userTurn,
        { kind: 'cancelled', partial: 'half' },
        { kind: 'error', message: 'boom' },
      ]);
    });
    await flushQueue();
    [call] = api.saveSessionTranscript.mock.calls.slice(-1);
    expect(call[0].entries[2]).toEqual({ kind: 'error', message: 'boom' });
  });

  it('blocks switching sessions while a stream is active, with a visible explanation', async () => {
    const { result } = renderHook(() => useHarness('local'));
    await waitFor(() => expect(result.current.persisted.activeSessionId).toBe('l2'));
    api.loadSession.mockClear();

    act(() => {
      chatControl.setStatus('streaming');
    });
    let ok = true;
    await act(async () => {
      ok = await result.current.persisted.selectSession('local', 'l1');
    });
    expect(ok).toBe(false);
    expect(result.current.persisted.notice).toBe(SWITCH_BLOCKED_NOTICE);
    expect(api.loadSession).not.toHaveBeenCalled();
    // The active session did not change and nothing was cancelled.
    expect(result.current.persisted.activeSessionId).toBe('l2');
    expect(result.current.persisted.chat.status).toBe('streaming');
  });

  it('startNewSession creates with the requested scope', async () => {
    const { result } = renderHook(() => useHarness('local'));
    await waitFor(() => expect(result.current.persisted.activeSessionId).toBe('l2'));

    await act(async () => {
      await result.current.persisted.startNewSession('local');
    });
    expect(api.createSession).toHaveBeenLastCalledWith({ scope: 'local' });

    await act(async () => {
      await result.current.persisted.startNewSession('project');
    });
    expect(api.createSession).toHaveBeenLastCalledWith({ scope: 'project' });
    expect(result.current.persisted.activeScope).toBe('project');
    expect(result.current.persisted.activeSessionId).toBe('fresh');
  });

  it('lazily creates a session for the first turn on a fresh surface', async () => {
    api.listSessions.mockResolvedValue({ sessions: [] });
    const { result } = renderHook(() => useHarness('local'));
    await waitFor(() => expect(result.current.sessions.local.status).toBe('ready'));
    expect(result.current.persisted.activeSessionId).toBeNull();

    act(() => {
      chatControl.setEntries([userTurn, streamingEntry]);
    });
    await flushQueue();
    expect(api.createSession).toHaveBeenCalledWith({ scope: 'local' });
    expect(api.saveSessionTranscript).toHaveBeenCalledWith({
      scope: 'local',
      sessionId: 'fresh',
      entries: [{ kind: 'message', message: { role: 'user', content: 'hello' } }],
    });
  });

  it('a failed save surfaces and the next boundary retries with the full snapshot', async () => {
    const { result } = renderHook(() => useHarness('local'));
    await waitFor(() => expect(result.current.persisted.activeSessionId).toBe('l2'));

    api.saveSessionTranscript.mockRejectedValueOnce(new Error('database is locked'));
    act(() => {
      chatControl.setEntries([userTurn, streamingEntry]);
    });
    await flushQueue();
    await waitFor(() =>
      expect(result.current.persisted.saveError).toBe('database is locked'),
    );

    act(() => {
      chatControl.setEntries([
        userTurn,
        { kind: 'message', message: { role: 'assistant', content: 'recovered' } },
      ]);
    });
    await flushQueue();
    const [call] = api.saveSessionTranscript.mock.calls.slice(-1);
    expect(call[0].entries).toHaveLength(2);
    await waitFor(() => expect(result.current.persisted.saveError).toBeNull());
  });

  it('an explicit New chat is never clobbered by a slower lazy creation (Codex P2)', async () => {
    api.listSessions.mockResolvedValue({ sessions: [] });
    let resolveLazyCreate: (value: { session: SessionSummary }) => void = () => undefined;
    api.createSession
      .mockImplementationOnce(
        () =>
          new Promise((resolve) => {
            resolveLazyCreate = resolve;
          }),
      )
      .mockImplementationOnce(() =>
        Promise.resolve({ session: summary('explicit-new', 'New chat', 60) }),
      );
    const { result } = renderHook(() => useHarness('local'));
    await waitFor(() => expect(result.current.sessions.local.status).toBe('ready'));

    // A boundary on the fresh surface starts the (slow) lazy creation…
    act(() => {
      chatControl.setEntries([userTurn, streamingEntry]);
    });
    await flushQueue();
    expect(api.createSession).toHaveBeenCalledTimes(1);

    // …and the user clicks New chat while it is still in flight.
    let newChatDone: Promise<boolean> = Promise.resolve(false);
    act(() => {
      newChatDone = result.current.persisted.startNewSession('local');
    });
    await act(async () => {
      resolveLazyCreate({ session: summary('lazy-old', 'New chat', 55) });
      await newChatDone;
    });

    // The explicit creation wins — pre-fix this reverted to lazy-old.
    expect(result.current.persisted.activeSessionId).toBe('explicit-new');
    // The fresh surface's turn was still preserved, in the lazy row.
    await flushQueue();
    expect(api.saveSessionTranscript).toHaveBeenCalledWith(
      expect.objectContaining({ sessionId: 'lazy-old' }),
    );
  });

  it('a pending lazy save never lands in a chat selected meanwhile (Codex re-review)', async () => {
    api.listSessions.mockResolvedValue({ sessions: [] });
    // The lazy creation is slow, so the terminal boundary is enqueued
    // while the surface still has no session id at all.
    let releaseLazyCreate: (value: { session: SessionSummary }) => void = () => undefined;
    api.createSession.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          releaseLazyCreate = resolve;
        }),
    );
    api.loadSession.mockResolvedValue({
      session: {
        ...summary('existing', 'Existing chat', 40),
        entries: [{ kind: 'message', message: { role: 'user', content: 'precious history' } }],
      },
    });
    const { result } = renderHook(() => useHarness('local'));
    await waitFor(() => expect(result.current.sessions.local.status).toBe('ready'));

    // Boundary 1 (accepted turn) starts the held lazy creation…
    act(() => {
      chatControl.setEntries([userTurn, streamingEntry]);
    });
    await flushQueue();
    expect(api.createSession).toHaveBeenCalledTimes(1);

    // …boundary 2 (terminal) is enqueued while the surface is still
    // session-less…
    act(() => {
      chatControl.setEntries([
        userTurn,
        { kind: 'message', message: { role: 'assistant', content: 'done' } },
      ]);
    });

    // …and the user selects an EXISTING chat before the queue drains.
    await act(async () => {
      await result.current.persisted.selectSession('local', 'existing');
    });
    expect(result.current.persisted.activeSessionId).toBe('existing');

    await act(async () => {
      releaseLazyCreate({ session: summary('lazy-old', 'New chat', 50) });
    });
    await flushQueue();

    // Both saves belong to the lazy surface. Pre-fix the second one
    // resolved the CURRENT active id and overwrote the selected
    // chat's transcript: ['lazy-old', 'existing'].
    const targets = api.saveSessionTranscript.mock.calls.map(([p]) => p.sessionId);
    expect(targets).toEqual(['lazy-old', 'lazy-old']);
    // The selected chat kept its own transcript and stayed active.
    expect(result.current.persisted.activeSessionId).toBe('existing');
    expect(result.current.persisted.chat.entries).toHaveLength(1);
  });

  it('handleDeleted resets an active surface backed by the deleted session', async () => {
    const { result } = renderHook(() => useHarness('local'));
    await waitFor(() => expect(result.current.persisted.activeSessionId).toBe('l2'));

    act(() => {
      result.current.persisted.handleDeleted('local', 'l2');
    });
    expect(result.current.persisted.activeSessionId).toBeNull();
    expect(result.current.persisted.chat.entries).toHaveLength(0);
  });

  // D65: automatic titles from the first accepted user message. The
  // rename rides the SAME queued task as the boundary save, gated on
  // the save response still carrying the backend default title.
  describe('auto-title (D65)', () => {
    it('renames a default-titled chat from the first accepted user turn', async () => {
      api.listSessions.mockImplementation(({ scope }: { scope: string }) =>
        Promise.resolve({
          sessions: scope === 'local' ? [summary('l2', 'New chat', 20)] : [],
        }),
      );
      api.saveSessionTranscript.mockImplementation(
        ({ sessionId }: { sessionId: string }) =>
          Promise.resolve({ session: summary(sessionId, 'New chat', 99) }),
      );
      api.renameSession.mockResolvedValue({ session: summary('l2', 'hello', 100) });
      const { result } = renderHook(() => useHarness('local'));
      await waitFor(() => expect(result.current.persisted.activeSessionId).toBe('l2'));

      act(() => {
        chatControl.setStatus('streaming');
        chatControl.setEntries([userTurn, streamingEntry]);
      });
      await flushQueue();
      expect(api.renameSession).toHaveBeenCalledWith({
        scope: 'local',
        sessionId: 'l2',
        title: 'hello',
      });
      // Save first, then the title — the transcript is never risked
      // on a cosmetic rename.
      const saveOrder = api.saveSessionTranscript.mock.invocationCallOrder[0];
      const renameOrder = api.renameSession.mock.invocationCallOrder[0];
      expect(saveOrder).toBeLessThan(renameOrder);
      // The sidebar row shows the derived title.
      expect(
        result.current.sessions.visibleOf('local').find((s) => s.id === 'l2')?.title,
      ).toBe('hello');
    });

    it('never renames a chat the user titled (manual title preserved)', async () => {
      // Default harness: l2 is listed as 'newest local' and the save
      // response returns 'saved' — both non-default. No rename fires
      // at any boundary.
      const { result } = renderHook(() => useHarness('local'));
      await waitFor(() => expect(result.current.persisted.activeSessionId).toBe('l2'));

      act(() => {
        chatControl.setEntries([userTurn, streamingEntry]);
      });
      await flushQueue();
      act(() => {
        chatControl.setEntries([
          userTurn,
          { kind: 'message', message: { role: 'assistant', content: 'done' } },
        ]);
      });
      await flushQueue();
      expect(api.saveSessionTranscript).toHaveBeenCalledTimes(2);
      expect(api.renameSession).not.toHaveBeenCalled();
    });

    it('rejected and empty sends never rename (no boundary, no save, no title)', async () => {
      api.listSessions.mockImplementation(({ scope }: { scope: string }) =>
        Promise.resolve({
          sessions: scope === 'local' ? [summary('l2', 'New chat', 20)] : [],
        }),
      );
      const { result } = renderHook(() => useHarness('local'));
      await waitFor(() => expect(result.current.persisted.activeSessionId).toBe('l2'));

      // A busy/empty send returns before touching the transcript;
      // the only observable is a status flicker at most. No entries
      // change → no boundary → nothing persisted, nothing titled.
      act(() => {
        chatControl.setStatus('streaming');
      });
      act(() => {
        chatControl.setStatus('idle');
      });
      await flushQueue();
      expect(api.saveSessionTranscript).not.toHaveBeenCalled();
      expect(api.renameSession).not.toHaveBeenCalled();
    });

    it('a snapshot without a user message saves but never titles', async () => {
      api.listSessions.mockImplementation(({ scope }: { scope: string }) =>
        Promise.resolve({
          sessions: scope === 'local' ? [summary('l2', 'New chat', 20)] : [],
        }),
      );
      api.saveSessionTranscript.mockImplementation(
        ({ sessionId }: { sessionId: string }) =>
          Promise.resolve({ session: summary(sessionId, 'New chat', 99) }),
      );
      const { result } = renderHook(() => useHarness('local'));
      await waitFor(() => expect(result.current.persisted.activeSessionId).toBe('l2'));

      act(() => {
        chatControl.setEntries([{ kind: 'error', message: 'send failed' }]);
      });
      await flushQueue();
      expect(api.saveSessionTranscript).toHaveBeenCalledTimes(1);
      expect(api.renameSession).not.toHaveBeenCalled();
    });

    it('lazy-create path: create, save, then title, in queue order', async () => {
      api.listSessions.mockResolvedValue({ sessions: [] });
      api.saveSessionTranscript.mockImplementation(
        ({ sessionId }: { sessionId: string }) =>
          Promise.resolve({ session: summary(sessionId, 'New chat', 99) }),
      );
      api.renameSession.mockResolvedValue({ session: summary('fresh', 'hello', 100) });
      const { result } = renderHook(() => useHarness('local'));
      await waitFor(() => expect(result.current.sessions.local.status).toBe('ready'));
      expect(result.current.persisted.activeSessionId).toBeNull();

      act(() => {
        chatControl.setEntries([userTurn, streamingEntry]);
      });
      await flushQueue();
      expect(api.renameSession).toHaveBeenCalledWith({
        scope: 'local',
        sessionId: 'fresh',
        title: 'hello',
      });
      const createOrder = api.createSession.mock.invocationCallOrder[0];
      const saveOrder = api.saveSessionTranscript.mock.invocationCallOrder[0];
      const renameOrder = api.renameSession.mock.invocationCallOrder[0];
      expect(createOrder).toBeLessThan(saveOrder);
      expect(saveOrder).toBeLessThan(renameOrder);
    });

    it('relaunch restore alone never titles; the next boundary titles from the FIRST user message', async () => {
      api.listSessions.mockImplementation(({ scope }: { scope: string }) =>
        Promise.resolve({
          sessions: scope === 'local' ? [summary('l2', 'New chat', 20)] : [],
        }),
      );
      api.loadSession.mockResolvedValue({
        session: {
          ...summary('l2', 'New chat', 20),
          entries: [
            { kind: 'message', message: { role: 'user', content: 'stored q' } },
            { kind: 'message', message: { role: 'assistant', content: 'stored a' } },
          ],
        },
      });
      api.saveSessionTranscript.mockImplementation(
        ({ sessionId }: { sessionId: string }) =>
          Promise.resolve({ session: summary(sessionId, 'New chat', 99) }),
      );
      api.renameSession.mockResolvedValue({ session: summary('l2', 'stored q', 100) });
      const { result } = renderHook(() => useHarness('local'));
      await waitFor(() => expect(result.current.persisted.activeSessionId).toBe('l2'));

      // Restoring the transcript is not a boundary — a pre-existing
      // default title survives relaunch untouched until the user
      // actually sends something.
      await flushQueue();
      expect(api.saveSessionTranscript).not.toHaveBeenCalled();
      expect(api.renameSession).not.toHaveBeenCalled();

      // New accepted turn: the title comes from the FIRST user
      // message of the transcript, not the newest one.
      const restored = result.current.persisted.chat.entries;
      act(() => {
        chatControl.setEntries([...restored, userTurn, streamingEntry]);
      });
      await flushQueue();
      expect(api.renameSession).toHaveBeenCalledWith({
        scope: 'local',
        sessionId: 'l2',
        title: 'stored q',
      });
    });

    it('a failed auto-rename retries at the next stable boundary', async () => {
      api.listSessions.mockImplementation(({ scope }: { scope: string }) =>
        Promise.resolve({
          sessions: scope === 'local' ? [summary('l2', 'New chat', 20)] : [],
        }),
      );
      api.saveSessionTranscript.mockImplementation(
        ({ sessionId }: { sessionId: string }) =>
          Promise.resolve({ session: summary(sessionId, 'New chat', 99) }),
      );
      api.renameSession
        .mockRejectedValueOnce(new Error('database is locked'))
        .mockResolvedValueOnce({ session: summary('l2', 'hello', 100) });
      const { result } = renderHook(() => useHarness('local'));
      await waitFor(() => expect(result.current.persisted.activeSessionId).toBe('l2'));

      act(() => {
        chatControl.setEntries([userTurn, streamingEntry]);
      });
      await flushQueue();
      expect(api.renameSession).toHaveBeenCalledTimes(1);
      // The failure is cosmetic: no save-error banner.
      expect(result.current.persisted.saveError).toBeNull();

      act(() => {
        chatControl.setEntries([
          userTurn,
          { kind: 'message', message: { role: 'assistant', content: 'done' } },
        ]);
      });
      await flushQueue();
      expect(api.renameSession).toHaveBeenCalledTimes(2);
      expect(api.renameSession).toHaveBeenLastCalledWith({
        scope: 'local',
        sessionId: 'l2',
        title: 'hello',
      });
    });
  });
});
