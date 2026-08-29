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
  forkSession,
  sessionStorageUsage,
  loadSession,
  rollbackSession,
  saveSessionTranscript,
  type SessionScope,
} from '../../lib/api/sessions';
import { useChat, type ChatApi, type ChatEntry } from '../chat/useChat';
import { sameContextSources } from '../chat/contextSources';
import type { ContextSourceRef } from '../../lib/api/chat';
import { DEFAULT_SESSION_TITLE, deriveSessionTitle } from './sessionTitle';
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
  /** Ref-backed identity for async handoffs that must inspect the
   * completed transition before React's next render. */
  surfaceIdentity: () => { scope: SessionScope; sessionId: string | null };
  /** Visible status line: the streaming switch block or a load
   * failure. Cleared by the next successful action. */
  notice: string | null;
  /** Most recent transcript-save failure, if any. The next stable
   * boundary retries automatically with the full snapshot. */
  saveError: string | null;
  storageFull: boolean;
  storageWarning: string | null;
  /** Load a session's transcript into the surface. `false` when
   * blocked (streaming) or the load failed. */
  selectSession: (scope: SessionScope, sessionId: string) => Promise<boolean>;
  /** Switch the surface between local and project chat, restoring
   * that scope's remembered (or most recent) session. */
  openScope: (scope: SessionScope) => Promise<boolean>;
  /** Create a session (database-first) and select it, empty. */
  startNewSession: (scope: SessionScope, title?: string) => Promise<boolean>;
  continueInNewChat: (scope: SessionScope, sourceId: string) => Promise<boolean>;
  rewindInNewChat: (scope: SessionScope, sourceId: string, turnCount: number) => Promise<boolean>;
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
  // A full store is a state, not an incident: it is resolved by asking the
  // backend, never by reading the save error's text.
  const [storage, setStorage] = useState<{ full: boolean; warning: string | null }>({
    full: false,
    warning: null,
  });

  const refreshStorage = useCallback(async () => {
    try {
      const usage = await sessionStorageUsage();
      const full = usage.usedBytes >= usage.capBytes;
      const nearing = !full && usage.usedBytes >= usage.warnBytes;
      setStorage({
        full,
        warning: nearing
          ? `This chat store is nearly full (${Math.round(usage.usedBytes / (1024 * 1024))} MB of ${Math.round(usage.capBytes / (1024 * 1024))} MB). Delete conversations you no longer need before new messages stop saving.`
          : null,
      });
    } catch (err) {
      // A usage check that fails must never block chat; it only means the
      // warning cannot be shown this time.
      console.error('sessions.storage failed:', formatError(err));
    }
  }, []);

  useEffect(() => {
    void refreshStorage();
  }, [refreshStorage]);

  // Render-mirrored refs so async bodies (the save queue) always read
  // current state without stale closures.
  const sessionsRef = useRef(sessions);
  sessionsRef.current = sessions;
  const activeIdsRef = useRef(activeIds);
  activeIdsRef.current = activeIds;
  const chatStatusRef = useRef(chat.status);
  chatStatusRef.current = chat.status;
  const chatEntriesRef = useRef(chat.entries);
  chatEntriesRef.current = chat.entries;
  const activeScopeRef = useRef(activeScope);
  activeScopeRef.current = activeScope;

  const commitSurfaceIdentity = useCallback(
    (scope: SessionScope, sessionId: string | null) => {
      activeScopeRef.current = scope;
      activeIdsRef.current = { ...activeIdsRef.current, [scope]: sessionId };
      setActiveScope(scope);
      setActiveIds((prev) => ({ ...prev, [scope]: sessionId }));
    },
    [],
  );

  /** Last persisted (or restored) snapshot, by element reference.
   * Set optimistically at enqueue time so re-renders can't enqueue
   * the same boundary twice. */
  const lastSavedRef = useRef<{
    sessionId: string | null;
    snapshot: ChatEntry[];
    contextSources: ContextSourceRef[];
  }>({
    sessionId: null,
    snapshot: [],
    contextSources: [],
  });
  /** Serialized session-mutation queue. Boundary saves AND explicit
   * session creation both run through it, in order — so a slow lazy
   * creation can never finish after (and clobber) an explicit New
   * chat that the user clicked in the meantime (Codex P2 on #108). */
  const queueRef = useRef<Promise<void>>(Promise.resolve());
  const branchPendingRef = useRef(false);
  /** The session the queue lazily created for the current
   * session-less surface, per scope. This — never the CURRENT active
   * id — is where a pending boundary save without a captured session
   * id belongs: re-reading the active id at run time let a queued
   * terminal save land in whatever chat the user selected meanwhile,
   * overwriting its transcript (Codex re-review on #108). Only queue
   * tasks write it; surface-identity transitions (select / new chat /
   * empty scope / delete) reset it so a LATER fresh surface can never
   * reuse a previous surface's lazy session. */
  const lazySessionIdRef = useRef<Record<SessionScope, string | null>>({
    local: null,
    project: null,
  });

  const runQueued = useCallback(<T,>(task: () => Promise<T>): Promise<T> => {
    const result = queueRef.current.then(task);
    // Tasks catch their own failures; this backstop keeps an
    // unexpected rejection from killing the chain (a dead queue
    // would silently stop all future saves).
    queueRef.current = result.then(
      () => undefined,
      (err) =>
        console.error(
          'session queue task failed:',
          err instanceof Error ? err.message : err,
        ),
    );
    return result;
  }, []);

  const enqueueSave = useCallback(
    (
      scope: SessionScope,
      sessionId: string | null,
      snapshot: ChatEntry[],
      contextSources: ContextSourceRef[],
    ) => {
      void runQueued(async () => {
        // A snapshot without a captured session id belongs to the
        // surface's lazily-created session — resolved from the
        // queue-local record, NOT from the current active id, which
        // may already point at a different chat the user selected
        // while this save was pending.
        let sid = sessionId ?? lazySessionIdRef.current[scope];
        if (sid === null) {
          const summary = await sessionsRef.current.create(scope);
          if (summary === null) {
            setSaveError('Could not create a chat session to save this transcript into.');
            return;
          }
          sid = summary.id;
          lazySessionIdRef.current[scope] = summary.id;
          // Adopt the lazily-created session only if the surface is
          // still session-less — an explicit selection or New chat
          // that landed meanwhile must win. The snapshot itself still
          // saves into the lazy session either way, so the turn is
          // never lost (it just lives in its own row).
          if (activeIdsRef.current[scope] === null) {
            activeIdsRef.current = { ...activeIdsRef.current, [scope]: summary.id };
            setActiveIds((prev) =>
              prev[scope] === null ? { ...prev, [scope]: summary.id } : prev,
            );
          }
        }
        try {
          const { session } = await saveSessionTranscript({
            scope,
            sessionId: sid,
            entries: entriesToWire(snapshot),
            contextSources,
          });
          sessionsRef.current.absorb(scope, session);
          setSaveError(null);
          // Re-measured after every successful save, not only at launch. The
          // warning exists to give the user room to export or delete *before*
          // writes stop; a launch-only check would first tell them the store is
          // filling up on the launch after it already had.
          void refreshStorage();
          // D65: auto-title. The save response is the freshest backend
          // truth about the title — only a session STILL on the default
          // gets a derived title, so a user rename (this window via the
          // manual-set guard inside `autoRename`, or any previous
          // launch via this check) is never overwritten. Runs inside
          // the same queued task as the save, against the same
          // queue-resolved `sid`, so it inherits the D63B ordering
          // guarantees — it can never target a chat the user selected
          // while this save was pending. No model, no cloud: the title
          // is derived locally from the first user message.
          if (session.title === DEFAULT_SESSION_TITLE) {
            const derived = deriveSessionTitle(snapshot);
            if (derived !== null && derived !== DEFAULT_SESSION_TITLE) {
              await sessionsRef.current.autoRename(scope, sid, derived);
            }
          }
        } catch (err) {
          const message = formatError(err);
          console.error('sessions.saveTranscript failed:', message);
          setSaveError(message);
        void refreshStorage();
        }
      });
    },
    [runQueued],
  );

  // The boundary detector. Runs on every entries change; the
  // reference comparison makes token frames free.
  useEffect(() => {
    // The backend can finish a very short stream before the synchronous
    // `chat.send` acceptance response reaches the frontend. While the user
    // row is still awaiting its exact accepted-context manifest, do not save
    // any part of that turn; otherwise the assistant could be persisted
    // without its user row. Acceptance or rejection clears the marker and
    // this effect then saves the complete, honest transcript boundary.
    if (
      chat.entries.some(
        (entry) =>
          entry.kind === 'message' && entry.pendingContextStreamId !== undefined,
      )
    ) {
      return;
    }
    const persistable = persistableOf(chat.entries);
    if (
      sameEntries(lastSavedRef.current.snapshot, persistable) &&
      sameContextSources(lastSavedRef.current.contextSources, chat.contextSources)
    ) {
      return;
    }
    const sessionId = activeIdsRef.current[activeScope];
    if (persistable.length === 0 && chat.contextSources.length === 0 && sessionId === null) {
      // Fresh empty surface — nothing worth creating a session for.
      lastSavedRef.current = { sessionId, snapshot: persistable, contextSources: [] };
      return;
    }
    const contextSources = [...chat.contextSources];
    lastSavedRef.current = { sessionId, snapshot: persistable, contextSources };
    enqueueSave(activeScope, sessionId, persistable, contextSources);
  }, [chat.entries, chat.contextSources, activeScope, enqueueSave]);

  const selectSession = useCallback(
    async (scope: SessionScope, sessionId: string): Promise<boolean> => {
      if (chat.status === 'streaming') {
        setNotice(SWITCH_BLOCKED_NOTICE);
        return false;
      }
      try {
        const { session } = await loadSession({ scope, sessionId });
        const restored = wireToEntries(session.entries);
        const restoredSources = session.contextSources ?? [];
        lastSavedRef.current = {
          sessionId,
          snapshot: restored,
          contextSources: restoredSources,
        };
        // Deliberately NOT clearing `lazySessionIdRef` here: a still
        // pending boundary save from the previous session-less surface
        // must keep resolving to THAT surface's lazy session. From now
        // on boundaries capture this explicit id, so the ref is
        // unreachable until a transition back to a session-less
        // surface — which is where it gets cleared.
        chat.restore(restored, restoredSources);
        commitSurfaceIdentity(scope, sessionId);
        setNotice(null);
        return true;
      } catch (err) {
        const message = formatError(err);
        console.error(`sessions.load (${scope}) failed:`, message);
        setNotice(`Could not load that chat: ${message}`);
        return false;
      }
    },
    [chat, commitSurfaceIdentity],
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
      // first send (or explicitly via New chat). This IS a fresh
      // session-less surface, so the lazy record resets — its first
      // boundary must mint a new session, never reuse an old one.
      lastSavedRef.current = { sessionId: null, snapshot: [], contextSources: [] };
      lazySessionIdRef.current[scope] = null;
      chat.restore([]);
      commitSurfaceIdentity(scope, null);
      setNotice(null);
      return true;
    },
    [activeScope, chat, commitSurfaceIdentity, selectSession],
  );

  const startNewSession = useCallback(
    async (scope: SessionScope, title?: string): Promise<boolean> => {
      if (chat.status === 'streaming') {
        setNotice(SWITCH_BLOCKED_NOTICE);
        return false;
      }
      // Queued behind any in-flight boundary save, so this creation
      // is ordered AFTER a pending lazy creation — the id set here is
      // final, not racing (Codex P2 on #108).
      return runQueued(async () => {
        // Re-check inside the queue: a stream could have started in
        // the tick between the guard above and this task running.
        if (chatStatusRef.current === 'streaming') {
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
        lastSavedRef.current = { sessionId: summary.id, snapshot: [], contextSources: [] };
        chat.restore([]);
        commitSurfaceIdentity(scope, summary.id);
        setNotice(null);
        return true;
      });
    },
    [chat, commitSurfaceIdentity, runQueued],
  );

  const branchInNewChat = useCallback(
    async (
      scope: SessionScope,
      runBranch: () => ReturnType<typeof forkSession>,
      labels: { action: string; completed: string; staleCreatedIsSuccess: boolean },
    ): Promise<boolean> => {
      if (chat.status === 'streaming') {
        setNotice(SWITCH_BLOCKED_NOTICE);
        return false;
      }
      if (branchPendingRef.current) return false;
      const captured = {
        scope: activeScopeRef.current,
        activeId: activeIdsRef.current[activeScopeRef.current],
        entries: chatEntriesRef.current,
        status: chatStatusRef.current,
      };
      const surfaceUnchanged = () =>
        activeScopeRef.current === captured.scope &&
        activeIdsRef.current[captured.scope] === captured.activeId &&
        chatEntriesRef.current === captured.entries &&
        chatStatusRef.current === captured.status;
      branchPendingRef.current = true;
      try {
        return await runQueued(async () => {
          if (chatStatusRef.current === 'streaming') {
            setNotice(SWITCH_BLOCKED_NOTICE);
            return false;
          }
          if (!surfaceUnchanged()) {
            setNotice(`Could not ${labels.action} that chat because the current chat changed first.`);
            return false;
          }
          try {
            const { session } = await runBranch();
            const restored = wireToEntries(session.entries);
            const restoredSources = session.contextSources ?? [];
            sessionsRef.current.absorb(scope, session);
            if (!surfaceUnchanged()) {
              setNotice(
                `The ${labels.completed} chat was created and saved in the sidebar, but this chat changed before it finished, so Plume did not switch.`,
              );
              return labels.staleCreatedIsSuccess;
            }
            lastSavedRef.current = {
              sessionId: session.id,
              snapshot: restored,
              contextSources: restoredSources,
            };
            lazySessionIdRef.current[scope] = null;
            chat.restore(restored, restoredSources);
            commitSurfaceIdentity(scope, session.id);
            setNotice(null);
            return true;
          } catch (err) {
            const message = formatError(err);
            console.error(`sessions.${labels.action} (${scope}) failed:`, message);
            setNotice(`Could not ${labels.action} that chat: ${message}`);
            return false;
          }
        });
      } finally {
        branchPendingRef.current = false;
      }
    },
    [chat, commitSurfaceIdentity, runQueued],
  );

  const continueInNewChat = useCallback(
    (scope: SessionScope, sourceId: string) =>
      branchInNewChat(
        scope,
        () => forkSession({ scope, sessionId: sourceId }),
        { action: 'continue', completed: 'continued', staleCreatedIsSuccess: false },
      ),
    [branchInNewChat],
  );

  const rewindInNewChat = useCallback(
    (scope: SessionScope, sourceId: string, turnCount: number) =>
      branchInNewChat(
        scope,
        () => rollbackSession({ scope, sessionId: sourceId, turnCount }),
        { action: 'rewind', completed: 'rewound', staleCreatedIsSuccess: true },
      ),
    [branchInNewChat],
  );

  const handleDeleted = useCallback(
    (scope: SessionScope, sessionId: string) => {
      // A deleted session must not receive future lazy saves.
      if (lazySessionIdRef.current[scope] === sessionId) {
        lazySessionIdRef.current[scope] = null;
      }
      if (activeIdsRef.current[scope] !== sessionId) return;
      // Deleting the active session leaves a fresh session-less
      // surface: reset the lazy record so its first boundary mints a
      // new session instead of reviving the old surface's.
      lazySessionIdRef.current[scope] = null;
      activeIdsRef.current = { ...activeIdsRef.current, [scope]: null };
      setActiveIds((prev) => ({ ...prev, [scope]: null }));
      if (scope === activeScope) {
        lastSavedRef.current = { sessionId: null, snapshot: [], contextSources: [] };
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

  const surfaceIdentity = useCallback(() => {
    const scope = activeScopeRef.current;
    return { scope, sessionId: activeIdsRef.current[scope] };
  }, []);

  return {
    chat,
    activeScope,
    activeSessionId: activeIds[activeScope],
    surfaceIdentity,
    notice,
    saveError,
    storageFull: storage.full,
    storageWarning: storage.warning,
    selectSession,
    openScope,
    startNewSession,
    continueInNewChat,
    rewindInNewChat,
    handleDeleted,
  };
}

function formatError(err: unknown): string {
  if (isIpcError(err)) return ipcErrorMessage(err);
  if (err instanceof Error) return err.message;
  return 'Session storage request failed.';
}
