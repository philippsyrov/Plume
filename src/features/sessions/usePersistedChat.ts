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
  homeSession,
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

type StorageState = {
  /** What the most recent `sessions.storage` read said about this store. */
  atCap: boolean;
  /** Whether the most recent write to this store was refused for space. */
  writesRefused: boolean;
  warning: string | null;
};

const EMPTY_STORAGE: StorageState = { atCap: false, writesRefused: false, warning: null };

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
  // A full store is a state, not an incident, and it has two independent
  // sources that must not overwrite each other.
  //
  // `atCap` is what a usage read says. `writesRefused` is what actually
  // happened to the most recent save. They disagree routinely, because the
  // backend refuses on PROJECTED size: a store at 400 of 512 MB legitimately
  // refuses a 200 MB save while every usage read still reports it below the
  // cap. Deriving the whole state from usage therefore erased the refusal the
  // user had just hit — they were told the save failed with none of the copy
  // saying why or what to do. Keeping them apart means a stale or below-cap
  // usage response can never clear a refusal.
  //
  // Kept per scope. The two stores are physically separate and fill
  // independently, so one shared field would report a project store's refusal
  // over a local store holding a megabyte — and, worse for a sticky field,
  // would let a save landing in one store clear the other's refusal.
  const [storage, setStorage] = useState<Record<SessionScope, StorageState>>({
    local: EMPTY_STORAGE,
    project: EMPTY_STORAGE,
  });

  // Scoped to the store that was actually written: a project store at its cap
  // would otherwise be reported through the local store's healthy numbers.
  const refreshStorage = useCallback(async (scope: SessionScope) => {
    try {
      const usage = await sessionStorageUsage({ scope });
      const atCap = usage.usedBytes >= usage.capBytes;
      const nearing = !atCap && usage.usedBytes >= usage.warnBytes;
      // Deliberately leaves `writesRefused` alone: this reading may predate the
      // refusal, and even a fresh one is below the cap whenever the refusal was
      // about a save that would have crossed it.
      setStorage((current) => ({
        ...current,
        [scope]: {
          ...current[scope],
          atCap,
          warning: nearing
            ? `This chat store is nearly full (${Math.round(usage.usedBytes / (1024 * 1024))} MB of ${Math.round(usage.capBytes / (1024 * 1024))} MB). Export and delete conversations you no longer need before new messages stop saving.`
            : null,
        },
      }));
    } catch (err) {
      // A usage check that fails must never block chat; it only means the
      // warning cannot be shown this time.
      console.error('sessions.storage failed:', formatError(err));
    }
  }, []);

  useEffect(() => {
    void refreshStorage(initialScope);
  }, [initialScope, refreshStorage]);

  const setStorageRefused = useCallback((scope: SessionScope, refused: boolean) => {
    setStorage((current) =>
      current[scope].writesRefused === refused
        ? current
        : { ...current, [scope]: { ...current[scope], writesRefused: refused } },
    );
  }, []);

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
          // Local scope has no fresh surface: Home always exists. Resolving it
          // here rather than creating a session matters when the user types
          // before startup's Home lookup returns — otherwise that first message
          // lands in an ordinary chat, startup then skips Home because a
          // session is already active, and the next relaunch opens an empty
          // Home while the real conversation sits somewhere else.
          const summary =
            scope === 'local'
              ? await homeSession()
                  .then(({ session }) => {
                    sessionsRef.current.absorb('local', session);
                    return session;
                  })
                  .catch((err: unknown) => {
                    console.error('sessions.home failed:', formatError(err));
                    return sessionsRef.current.create(scope);
                  })
              : await sessionsRef.current.create(scope);
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
          setStorageRefused(scope, false);
          // Re-measured after every successful save, not only at launch. The
          // warning exists to give the user room to export or delete *before*
          // writes stop; a launch-only check would first tell them the store is
          // filling up on the launch after it already had.
          void refreshStorage(scope);
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
          // Set on a space refusal and cleared on anything else, because the
          // state this drives is "the last save was refused for space". A
          // transient lock failure after a refusal is not that, and leaving the
          // flag standing would tell the user to go delete history over a
          // database lock. A store that really is at its cap keeps saying so
          // through `atCap`, which comes from usage rather than from this.
          setStorageRefused(scope, isIpcError(err) && err.kind === 'StorageFull');
          void refreshStorage(scope);
        }
      });
    },
    [runQueued, setStorageRefused],
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
            // A branch copies a whole transcript, so it can be refused for
            // space like any save. Leaving it as a polite notice would hide the
            // one action that fixes it.
            if (isIpcError(err) && err.kind === 'StorageFull') {
              setStorageRefused(scope, true);
              setSaveError(message);
            }
            return false;
          }
        });
      } finally {
        branchPendingRef.current = false;
      }
    },
    [chat, commitSurfaceIdentity, runQueued, setStorageRefused],
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

  // Relaunch restore. Local scope returns to the durable Home conversation:
  // the point of Home is that relaunching lands in the same place, so the
  // most-recently-updated heuristic would defeat it the moment the user
  // opened a second chat. Project scope keeps that heuristic, because a
  // project has no Home.
  //
  // Home's id comes from the backend on every launch and is never persisted
  // here — the frontend must not be able to choose which conversation is Home.
  // If resolving it fails, fall back to the previous behaviour rather than
  // leaving the user with no conversation at all.
  const didInitRef = useRef(false);
  const initialState = initialScope === 'local' ? sessions.local : sessions.project;
  useEffect(() => {
    if (didInitRef.current) return;
    if (initialState.status !== 'ready') return;
    didInitRef.current = true;

    const mostRecent = () => {
      const first = sessionsRef.current.visibleOf(initialScope)[0];
      if (first !== undefined) void selectSession(initialScope, first.id);
    };

    if (initialScope !== 'local') {
      mostRecent();
      return;
    }

    // Resolving Home is an IPC round-trip, and the user is not frozen during it.
    // If they picked a chat, started a new one, or began streaming while it was
    // in flight, landing on Home would yank them off their own choice — and
    // `selectSession` would restore over a live stream, because its status
    // guard closes over the value captured when this effect ran.
    void Promise.resolve()
      .then(homeSession)
      .then(({ session }) => {
        if (activeIdsRef.current.local !== null) return;
        if (chatStatusRef.current === 'streaming') return;
        sessionsRef.current.absorb('local', session);
        return selectSession('local', session.id);
      })
      .catch((err: unknown) => {
        console.error('sessions.home failed:', formatError(err));
        mostRecent();
      });
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
    // Either source is enough to stop promising an automatic retry.
    storageFull: storage[activeScope].atCap || storage[activeScope].writesRefused,
    storageWarning: storage[activeScope].warning,
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
