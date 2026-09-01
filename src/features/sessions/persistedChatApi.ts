// The shape `usePersistedChat` exposes, kept beside the hook rather than inside
// it: consumers import the type far more often than they read the hook, and the
// hook is already at the file-size guardrail.

import type { SessionIdentity, SessionScope } from '../../lib/api/sessions';
import type { ChatApi } from '../chat/useChat';

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
  /**
   * The session that should own a new attachment or Browser workspace for
   * `scope`, resolved the way that scope requires.
   *
   * This is the one path a consumer may use. Local always resolves the
   * backend-owned Home conversation, and every caller joins the same in-flight
   * lookup, so the Browser and Library cannot each start their own resolution
   * — or, when one of them lost that race, mint an ordinary chat instead.
   * `null` means the surface has no owner and the caller must do nothing.
   */
  ensureOwnedSession: (scope: SessionScope) => Promise<SessionIdentity | null>;
  /** Drain the transcript-save queue and resolve the exact owner before send. */
  prepareSend: () => Promise<SessionIdentity | null>;
  continueInNewChat: (scope: SessionScope, sourceId: string) => Promise<boolean>;
  rewindInNewChat: (scope: SessionScope, sourceId: string, turnCount: number) => Promise<boolean>;
  /** Tell the bridge a session was deleted so an active surface
   * backed by it resets instead of saving into a dead id. */
  handleDeleted: (scope: SessionScope, sessionId: string) => void;
};
