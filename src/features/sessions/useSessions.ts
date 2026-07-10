// D63B: sidebar session-list state over the D63A `sessions.*` IPC.
//
// One instance owns BOTH scopes' summary lists. Scope separation is
// enforced by the backend (local resolves to app data, project to the
// open trusted project); this hook just never mixes the two arrays.
//
// Mutations are database-first (spec § Save Lifecycle): React state
// changes only after the IPC resolves. A failed mutation leaves the
// row exactly as it was and returns the error message for the calling
// dialog to announce — nothing disappears optimistically.

import { useCallback, useEffect, useRef, useState } from 'react';

import { ipcErrorMessage, isIpcError } from '../../lib/api/errors';
import {
  archiveSession,
  createSession,
  deleteSession,
  listSessions,
  renameSession,
  type SessionScope,
  type SessionSummary,
} from '../../lib/api/sessions';
import { DEFAULT_SESSION_TITLE } from './sessionTitle';

export type MutationResult = { ok: true } | { ok: false; message: string };

export type ScopeListState = {
  /** Every session in the scope, archived included, `updatedAtMs`
   * descending (backend order). Use the `visible*` / `archived*`
   * helpers for the two sidebar views. */
  sessions: SessionSummary[];
  status: 'loading' | 'ready' | 'error';
  error: string | null;
};

export type SessionsApi = {
  local: ScopeListState;
  project: ScopeListState;
  /** Non-archived sessions of a scope, newest update first. */
  visibleOf: (scope: SessionScope) => SessionSummary[];
  /** Archived sessions of a scope, for the archived-chats modal. */
  archivedOf: (scope: SessionScope) => SessionSummary[];
  refresh: (scope: SessionScope) => Promise<void>;
  /** Create a session (database-first) and prepend it to the list. */
  create: (scope: SessionScope, title?: string) => Promise<SessionSummary | null>;
  rename: (scope: SessionScope, sessionId: string, title: string) => Promise<MutationResult>;
  /** D65: derived-title rename. Applies ONLY while the session still
   * carries the backend default title and the user has not renamed
   * it this window — a user title is never overwritten. Failures are
   * logged, not surfaced: the title stays default and the next
   * stable boundary retries. */
  autoRename: (scope: SessionScope, sessionId: string, title: string) => Promise<void>;
  setArchived: (
    scope: SessionScope,
    sessionId: string,
    archived: boolean,
  ) => Promise<MutationResult>;
  remove: (scope: SessionScope, sessionId: string) => Promise<MutationResult>;
  /** Fold a summary returned by another verb (e.g. `saveTranscript`)
   * into the list so ordering tracks `updatedAtMs` without a refetch. */
  absorb: (scope: SessionScope, summary: SessionSummary) => void;
  /** Message from the most recent failed mutation, for a quiet shell
   * banner; dialogs get the same message from their own call site. */
  lastMutationError: string | null;
};

const EMPTY: ScopeListState = { sessions: [], status: 'loading', error: null };

export function useSessions({ projectAvailable }: { projectAvailable: boolean }): SessionsApi {
  const [local, setLocal] = useState<ScopeListState>(EMPTY);
  const [project, setProject] = useState<ScopeListState>(EMPTY);
  const [lastMutationError, setLastMutationError] = useState<string | null>(null);
  // Stale-response guard: a refresh that resolves after the hook was
  // torn down (project closed) must not set state.
  const aliveRef = useRef(true);
  useEffect(() => {
    aliveRef.current = true;
    return () => {
      aliveRef.current = false;
    };
  }, []);

  const setterOf = useCallback(
    (scope: SessionScope) => (scope === 'local' ? setLocal : setProject),
    [],
  );

  const refresh = useCallback(
    async (scope: SessionScope) => {
      const setScope = setterOf(scope);
      try {
        const { sessions } = await listSessions({ scope, includeArchived: true });
        if (!aliveRef.current) return;
        setScope({ sessions, status: 'ready', error: null });
      } catch (err) {
        const message = formatError(err);
        console.error(`sessions.list (${scope}) failed:`, message);
        if (!aliveRef.current) return;
        setScope((prev) => ({ ...prev, status: 'error', error: message }));
      }
    },
    [setterOf],
  );

  useEffect(() => {
    void refresh('local');
  }, [refresh]);
  useEffect(() => {
    if (projectAvailable) void refresh('project');
  }, [projectAvailable, refresh]);

  const create = useCallback(
    async (scope: SessionScope, title?: string): Promise<SessionSummary | null> => {
      try {
        const { session } = await createSession({ scope, ...(title ? { title } : {}) });
        if (aliveRef.current) {
          setterOf(scope)((prev) => ({
            ...prev,
            sessions: [session, ...prev.sessions],
          }));
          setLastMutationError(null);
        }
        return session;
      } catch (err) {
        const message = formatError(err);
        console.error(`sessions.create (${scope}) failed:`, message);
        if (aliveRef.current) setLastMutationError(message);
        return null;
      }
    },
    [setterOf],
  );

  const absorb = useCallback(
    (scope: SessionScope, summary: SessionSummary) => {
      setterOf(scope)((prev) => ({
        ...prev,
        sessions: resort([
          summary,
          ...prev.sessions.filter((s) => s.id !== summary.id),
        ]),
      }));
    },
    [setterOf],
  );

  const mutate = useCallback(
    async (
      scope: SessionScope,
      label: string,
      op: () => Promise<SessionSummary | null>,
    ): Promise<MutationResult> => {
      try {
        const summary = await op();
        if (aliveRef.current) {
          if (summary !== null) {
            absorb(scope, summary);
          }
          setLastMutationError(null);
        }
        return { ok: true };
      } catch (err) {
        const message = formatError(err);
        console.error(`${label} (${scope}) failed:`, message);
        if (aliveRef.current) setLastMutationError(message);
        return { ok: false, message };
      }
    },
    [absorb],
  );

  // D65: ids the user renamed in this window. Marked synchronously at
  // call time (before the IPC resolves) so a queued auto-rename that
  // checks the set even one tick later already sees the user's claim
  // on the title. Bounded by construction: ids are only added on
  // explicit user renames and the backend caps each database at 200
  // sessions; `remove` sweeps deleted ids as hygiene.
  const manualTitlesRef = useRef<Set<string>>(new Set());

  const rename = useCallback(
    (scope: SessionScope, sessionId: string, title: string) => {
      manualTitlesRef.current.add(sessionId);
      return mutate(scope, 'sessions.rename', async () => {
        const { session } = await renameSession({ scope, sessionId, title });
        return session;
      });
    },
    [mutate],
  );

  const autoRename = useCallback(
    async (scope: SessionScope, sessionId: string, title: string): Promise<void> => {
      // Contract: the caller has just observed the DEFAULT title on a
      // fresh backend summary (the `saveTranscript` response) — this
      // function cannot re-check list state authoritatively, because
      // inside the serialized save queue the lazily-created session
      // may not have flushed into `local`/`project` yet.
      //
      // Guard 1: a user rename this window always wins, even a rename
      // back to the literal default title.
      if (manualTitlesRef.current.has(sessionId)) return;
      // Guard 2 (positive evidence only): if the possibly-stale list
      // already shows a non-default title, something else titled this
      // session — never overwrite it. An absent row does NOT skip
      // (the lazy-create case above).
      const listed = (scope === 'local' ? local : project).sessions.find(
        (s) => s.id === sessionId,
      );
      if (listed !== undefined && listed.title !== DEFAULT_SESSION_TITLE) return;
      try {
        const { session } = await renameSession({ scope, sessionId, title });
        if (aliveRef.current) absorb(scope, session);
      } catch (err) {
        // Cosmetic failure: log only — no banner. The title stays
        // default, so the next stable boundary retries.
        console.error(`sessions.rename (auto, ${scope}) failed:`, formatError(err));
      }
    },
    [absorb, local, project],
  );

  const setArchived = useCallback(
    (scope: SessionScope, sessionId: string, archived: boolean) =>
      mutate(scope, 'sessions.archive', async () => {
        const { session } = await archiveSession({ scope, sessionId, archived });
        return session;
      }),
    [mutate],
  );

  const remove = useCallback(
    async (scope: SessionScope, sessionId: string): Promise<MutationResult> => {
      try {
        await deleteSession({ scope, sessionId });
        manualTitlesRef.current.delete(sessionId);
        if (aliveRef.current) {
          setterOf(scope)((prev) => ({
            ...prev,
            sessions: prev.sessions.filter((s) => s.id !== sessionId),
          }));
          setLastMutationError(null);
        }
        return { ok: true };
      } catch (err) {
        const message = formatError(err);
        console.error(`sessions.delete (${scope}) failed:`, message);
        if (aliveRef.current) setLastMutationError(message);
        return { ok: false, message };
      }
    },
    [setterOf],
  );

  const stateOf = useCallback(
    (scope: SessionScope) => (scope === 'local' ? local : project),
    [local, project],
  );
  const visibleOf = useCallback(
    (scope: SessionScope) => stateOf(scope).sessions.filter((s) => s.archivedAtMs === null),
    [stateOf],
  );
  const archivedOf = useCallback(
    (scope: SessionScope) => stateOf(scope).sessions.filter((s) => s.archivedAtMs !== null),
    [stateOf],
  );

  return {
    local,
    project,
    visibleOf,
    archivedOf,
    refresh,
    create,
    rename,
    autoRename,
    setArchived,
    remove,
    absorb,
    lastMutationError,
  };
}

/** Keep the backend's contract order after a local splice: newest
 * update first, id as the stable tiebreak. */
function resort(sessions: SessionSummary[]): SessionSummary[] {
  return [...sessions].sort((a, b) =>
    b.updatedAtMs !== a.updatedAtMs
      ? b.updatedAtMs - a.updatedAtMs
      : b.id.localeCompare(a.id),
  );
}

function formatError(err: unknown): string {
  if (isIpcError(err)) return ipcErrorMessage(err);
  if (err instanceof Error) return err.message;
  return 'Session request failed.';
}
