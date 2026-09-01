// The gate every persisted send passes through before it reaches the backend.
//
// Split out of `usePersistedChat` for size, but it is also one idea on its own:
// a send is only honest if the transcript the backend will project is already
// durable and the owner is the exact one. Everything here serves that.

import { useCallback, useRef, type MutableRefObject } from 'react';

import type { ContextSourceRef } from '../../lib/api/chat';
import type { SessionIdentity, SessionScope } from '../../lib/api/sessions';
import type { ChatEntry } from '../chat/useChat';

type FailedSave = {
  sessionId: string | null;
  snapshot: ChatEntry[];
  contextSources: ContextSourceRef[];
};

export type DurableSendGate = {
  prepareSend: () => Promise<SessionIdentity | null>;
  /** Called by the save queue when a save for `scope` lands. */
  noteSaveLanded: (scope: SessionScope) => void;
  /** Called by the save queue when a save for `scope` fails, with the exact
   * arguments so the retry lands in that same surface. */
  noteSaveFailed: (scope: SessionScope, failed: FailedSave) => void;
};

export function useDurableSendGate({
  activeScopeRef,
  queueRef,
  enqueueSave,
  ensureOwnedSession,
}: {
  activeScopeRef: MutableRefObject<SessionScope>;
  queueRef: MutableRefObject<Promise<void>>;
  enqueueSave: (
    scope: SessionScope,
    sessionId: string | null,
    snapshot: ChatEntry[],
    contextSources: ContextSourceRef[],
  ) => void;
  ensureOwnedSession: (scope: SessionScope) => Promise<SessionIdentity | null>;
}): DurableSendGate {
  /** Per scope, the save that failed and has not landed since, or `null`.
   *
   * The boundary detector cannot retry on its own: the hook records the
   * snapshot optimistically at enqueue time, so after a failure the unchanged
   * transcript no longer looks like a new boundary. Keeping the failed
   * arguments here is what lets this gate re-drive that exact save against that
   * exact surface, rather than latching the chat shut until the next relaunch.
   */
  const failedSaveRef = useRef<Record<SessionScope, FailedSave | null>>({
    local: null,
    project: null,
  });

  const noteSaveLanded = useCallback((scope: SessionScope) => {
    failedSaveRef.current[scope] = null;
  }, []);

  const noteSaveFailed = useCallback((scope: SessionScope, failed: FailedSave) => {
    failedSaveRef.current[scope] = failed;
  }, []);

  /** Re-drive the save that failed for `scope`, if there is one. `true` means
   * this scope's transcript is durable as of now. */
  const retryFailedSave = useCallback(
    async (scope: SessionScope): Promise<boolean> => {
      const failed = failedSaveRef.current[scope];
      if (failed === null) return true;
      enqueueSave(scope, failed.sessionId, failed.snapshot, failed.contextSources);
      await queueRef.current;
      return failedSaveRef.current[scope] === null;
    },
    [enqueueSave],
  );

  /** Make this surface safe to send from, and say who owns it.
   *
   * Two things have to be true before a persisted send, and both are why this
   * exists rather than the caller reading `activeSessionId`.
   *
   * The transcript has to be durable first. The backend projects the prompt
   * from the durable store, so a turn still sitting in the save queue is a turn
   * the model will not see — the previous assistant reply most of all, because
   * a fast second send races its save.
   *
   * And the owner has to be the exact one. A first send on a local surface can
   * beat Home's resolution; without this it would take the ownerless
   * compatibility path, omit the durable history, and then overwrite Home with
   * the incomplete transcript it did send.
   *
   * `null` means do not send. The caller keeps the draft and the user can try
   * again — a refusal here is recoverable, an incomplete prompt is not. */
  const prepareSend = useCallback(async (): Promise<SessionIdentity | null> => {
    const scope = activeScopeRef.current;
    await queueRef.current;
    if (!(await retryFailedSave(scope))) return null;
    const owner = await ensureOwnedSession(scope);
    if (owner === null) return null;
    // Resolving the owner can mint or adopt a session, and adoption enqueues a
    // save of its own; that one has to land too.
    await queueRef.current;
    if (failedSaveRef.current[owner.scope] !== null) return null;
    return owner;
  }, [ensureOwnedSession, retryFailedSave]);

  return { prepareSend, noteSaveLanded, noteSaveFailed };
}
