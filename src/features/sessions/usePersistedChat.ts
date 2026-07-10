// D63B: bridge between `useChat`'s visible transcript and the D63A
// `sessions.saveTranscript` snapshot persistence.
//
// The frontend keeps owning live token rendering; this hook persists
// ONLY at stable boundaries (spec § Save Lifecycle):
//
//   * after an accepted send — the transcript gains the user turn
//     (plus a streaming placeholder that is filtered out);
//   * on `chat/done`, stop, or error — the streaming entry flips to
//     its terminal shape.
//
// Token frames replace only the streaming entry, so the persistable
// slice keeps element identity and `sameEntries` detects "nothing to
// save" without serializing anything. No timer, no debounce, no
// per-token writes — the boundary IS the state change.
//
// Switching sessions (or scope surfaces) while a stream is active is
// blocked with a visible explanation; nothing is cancelled or
// detached silently. One `useChat` instance per window shell means
// only one session can stream at a time, matching the spec.

import { useCallback, useEffect, useRef, useState } from 'react';

import { ipcErrorMessage, isIpcError } from '../../lib/api/errors';
import {
  loadSession,
  saveSessionTranscript,
  type SessionScope,
} from '../../lib/api/sessions';
import { useChat, type ChatApi, type ChatEntry } from '../chat/useChat';
import { entriesToWire, persistableOf, sameEntries, wireToEntries } from './transcript';
import type { SessionsApi } from './useSessions';

export const SWITCH_BLOCKED_NOTICE =
  'A reply is still streaming. Stop it or let it finish before switching chats — switching never cancels a stream silently.';

export type PersistedChatApi = {
  /** The hoisted chat instance — hand this to `ChatPanel`. */
  chat: ChatApi;
  /** Which scope the central chat surface is showing. */
  activeScope: SessionScope;
  /** Persisted session backing the surface; `null` for a fresh, not
   * yet persisted surface (first send lazily creates the session). */
  activeSessionId: string | null;
  /** Visible status line: the streaming switch block or a load
   * failure. Cleared by the next successful action. */
  notice: string | null;
  /** Most recent transcript-save failure, if any. The next stable
   * boundary retries automatically with the full snapshot. */
  saveError: string | null;
  /** Load a session's transcript into the surface. `false` when
   * blocked (streaming) or the load failed. */
  selectSession: (scope: SessionScope, sessionId: string) => Promise<boolean>;
  /** Switch the surface between local and project chat, restoring
   * that scope's remembered (or most recent) session. */
  openScope: (scope: SessionScope) => Promise<boolean>;
  /** Create a session (database-first) and select it, empty. */
  startNewSession: (scope: SessionScope, title?: string) => Promise<boolean>;
  /** Tell the bridge a session was deleted so an active surface
   * backed by it resets instead of saving into a dead id. */
  handleDeleted: (scope: SessionScope, sessionId: string) => void;
};

export function usePersistedChat({
  sessions,
  initialScope,
}: {
  sessions: SessionsApi;
  initialScope: SessionScope;
}): PersistedChatApi {
  const chat = useChat();
  const [activeScope, setActiveScope] = useState<SessionScope>(initialScope);
  const [activeIds, setActiveIds] = useState<Record<SessionScope, string | null>>({
    local: null,
    project: null,
  });
  const [notice, setNotice] = useState<string | null>(null);
  const [saveError, setSaveError] = useState<string | null>(null);

  // Render-mirrored refs so async bodies (the save queue) always read
  // current state without stale closures.
  const sessionsRef = useRef(sessions);
  sessionsRef.current = sessions;
  const activeIdsRef = useRef(activeIds);
  activeIdsRef.current = activeIds;

  /** Last persisted (or restored) snapshot, by element reference.
   * Set optimistically at enqueue time so re-renders can't enqueue
   * the same boundary twice. */
  const lastSavedRef = useRef<{ sessionId: string | null; snapshot: ChatEntry[] }>({
    sessionId: null,
    snapshot: [],
  });
  /** Serialized save queue: boundaries persist in order, and a
   * lazy-create can never race a concurrent save into two sessions. */
  const queueRef = useRef<Promise<void>>(Promise.resolve());

  const enqueueSave = useCallback(
    (scope: SessionScope, sessionId: string | null, snapshot: ChatEntry[]) => {
      const queued = queueRef.current.then(async () => {
        // Re-read at run time: an earlier queued task may have
        // lazily created the session this snapshot belongs to.
        let sid = sessionId ?? activeIdsRef.current[scope];
        if (sid === null) {
          const summary = await sessionsRef.current.create(scope);
          if (summary === null) {
            setSaveError('Could not create a chat session to save this transcript into.');
            return;
          }
          sid = summary.id;
          setActiveIds((prev) => ({ ...prev, [scope]: summary.id }));
        }
        try {
          const { session } = await saveSessionTranscript({
            scope,
            sessionId: sid,
            entries: entriesToWire(snapshot),
          });
          sessionsRef.current.absorb(scope, session);
          setSaveError(null);
        } catch (err) {
          const message = formatError(err);
          console.error('sessions.saveTranscript failed:', message);
          setSaveError(message);
        }
      });
      // Every task above already catches its own failures; this keeps
      // an unexpected rejection from killing the chain (a dead queue
      // would silently stop all future saves).
      queueRef.current = queued.catch((err) =>
        console.error('session save queue error:', err instanceof Error ? err.message : err),
      );
    },
    [],
  );

  // The boundary detector. Runs on every entries change; the
  // reference comparison makes token frames free.
  useEffect(() => {
    const persistable = persistableOf(chat.entries);
    if (sameEntries(lastSavedRef.current.snapshot, persistable)) return;
    const sessionId = activeIdsRef.current[activeScope];
    if (persistable.length === 0 && sessionId === null) {
      // Fresh empty surface — nothing worth creating a session for.
      lastSavedRef.current = { sessionId, snapshot: persistable };
      return;
    }
    lastSavedRef.current = { sessionId, snapshot: persistable };
    enqueueSave(activeScope, sessionId, persistable);
  }, [chat.entries, activeScope, enqueueSave]);

  const selectSession = useCallback(
    async (scope: SessionScope, sessionId: string): Promise<boolean> => {
      if (chat.status === 'streaming') {
        setNotice(SWITCH_BLOCKED_NOTICE);
        return false;
      }
      try {
        const { session } = await loadSession({ scope, sessionId });
        const restored = wireToEntries(session.entries);
        lastSavedRef.current = { sessionId, snapshot: restored };
        chat.restore(restored);
        setActiveScope(scope);
        setActiveIds((prev) => ({ ...prev, [scope]: sessionId }));
        setNotice(null);
        return true;
      } catch (err) {
        const message = formatError(err);
        console.error(`sessions.load (${scope}) failed:`, message);
        setNotice(`Could not load that chat: ${message}`);
        return false;
      }
    },
    [chat],
  );

  const openScope = useCallback(
    async (scope: SessionScope): Promise<boolean> => {
      if (scope === activeScope) return true;
      if (chat.status === 'streaming') {
        setNotice(SWITCH_BLOCKED_NOTICE);
        return false;
      }
      const remembered = activeIdsRef.current[scope];
      const target = remembered ?? sessionsRef.current.visibleOf(scope)[0]?.id ?? null;
      if (target !== null) return selectSession(scope, target);
      // Empty scope: blank surface, session created lazily on the
      // first send (or explicitly via New chat).
      lastSavedRef.current = { sessionId: null, snapshot: [] };
      chat.restore([]);
      setActiveScope(scope);
      setNotice(null);
      return true;
    },
    [activeScope, chat, selectSession],
  );

  const startNewSession = useCallback(
    async (scope: SessionScope, title?: string): Promise<boolean> => {
      if (chat.status === 'streaming') {
        setNotice(SWITCH_BLOCKED_NOTICE);
        return false;
      }
      const summary = await sessionsRef.current.create(scope, title);
      if (summary === null) {
        // The detailed reason is in `sessions.lastMutationError` and
        // the console; the surface just needs a visible outcome.
        setNotice('Could not create a new chat — see the app log for details.');
        return false;
      }
      lastSavedRef.current = { sessionId: summary.id, snapshot: [] };
      chat.restore([]);
      setActiveScope(scope);
      setActiveIds((prev) => ({ ...prev, [scope]: summary.id }));
      setNotice(null);
      return true;
    },
    [chat],
  );

  const handleDeleted = useCallback(
    (scope: SessionScope, sessionId: string) => {
      if (activeIdsRef.current[scope] !== sessionId) return;
      setActiveIds((prev) => ({ ...prev, [scope]: null }));
      if (scope === activeScope) {
        lastSavedRef.current = { sessionId: null, snapshot: [] };
        chat.restore([]);
      }
    },
    [activeScope, chat],
  );

  // Relaunch restore: once the initial scope's list is ready, select
  // its most recently updated session — scopes never mix (the list
  // itself came from the scope-specific database).
  const didInitRef = useRef(false);
  const initialState = initialScope === 'local' ? sessions.local : sessions.project;
  useEffect(() => {
    if (didInitRef.current) return;
    if (initialState.status !== 'ready') return;
    didInitRef.current = true;
    const first = sessionsRef.current.visibleOf(initialScope)[0];
    if (first !== undefined) void selectSession(initialScope, first.id);
  }, [initialScope, initialState.status, selectSession]);

  return {
    chat,
    activeScope,
    activeSessionId: activeIds[activeScope],
    notice,
    saveError,
    selectSession,
    openScope,
    startNewSession,
    handleDeleted,
  };
}

function formatError(err: unknown): string {
  if (isIpcError(err)) return ipcErrorMessage(err);
  if (err instanceof Error) return err.message;
  return 'Session storage request failed.';
}
