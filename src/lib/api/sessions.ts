// Typed wrappers for the `sessions.*` IPC family (D63A).
//
// Durable chat sessions, one SQLite schema in two physically separate
// databases: `scope: 'local'` is the app-data store (available without
// a project), `scope: 'project'` is the currently open TRUSTED
// project's `.plume/sessions` store. No command accepts a filesystem
// root and no response carries one — the backend alone resolves which
// database a scope means, so a project switch can never redirect local
// chat and a mismatched session id is a plain `NotFound`.
//
// D63A ships the spine only; the sidebar UI wiring is D63B.

import { invokeIpc } from './ipc';
import type { ChatStats } from './chat';

export type SessionScope = 'local' | 'project';

export type SessionSummary = {
  /** Opaque backend-minted id; never a path. */
  id: string;
  title: string;
  createdAtMs: number;
  updatedAtMs: number;
  /** `null` while the session is live; set when archived. */
  archivedAtMs: number | null;
};

/**
 * One persisted transcript entry — the frontend's visible `ChatEntry`
 * shape minus `streaming` (in-flight placeholders are never
 * persisted). `role` is narrower than `ChatRole` on purpose: only
 * `user` and `assistant` turns are transcript; `system`/`tool` are
 * transport detail and the backend rejects them.
 */
export type SessionTranscriptEntry =
  | {
      kind: 'message';
      message: { role: 'user' | 'assistant'; content: string };
      modelUsed?: string;
      durationMs?: number;
      attachmentRelPath?: string;
      attachmentLineRange?: { startLine: number; endLine: number };
      stats?: ChatStats;
      sentInMode?: 'chat' | 'proposeDiff';
    }
  | {
      kind: 'cancelled';
      partial: string;
      modelUsed?: string;
      durationMs?: number;
    }
  | { kind: 'error'; message: string };

export type SessionRecord = SessionSummary & {
  entries: SessionTranscriptEntry[];
};

export type SessionsListPayload = {
  scope: SessionScope;
  /** Archived sessions are hidden unless `true`. */
  includeArchived?: boolean;
};

export type SessionsListResponse = {
  /** Ordered by `updatedAtMs` descending. */
  sessions: SessionSummary[];
};

export function listSessions(payload: SessionsListPayload): Promise<SessionsListResponse> {
  return invokeIpc<SessionsListPayload, SessionsListResponse>('sessions_list', payload);
}

export type SessionsCreatePayload = {
  scope: SessionScope;
  /** Omit for the backend's default title. Trimmed; 1–120 characters. */
  title?: string;
};

export type SessionSummaryResponse = {
  session: SessionSummary;
};

export function createSession(payload: SessionsCreatePayload): Promise<SessionSummaryResponse> {
  return invokeIpc<SessionsCreatePayload, SessionSummaryResponse>('sessions_create', payload);
}

export type SessionsLoadPayload = {
  scope: SessionScope;
  sessionId: string;
};

export type SessionRecordResponse = {
  session: SessionRecord;
};

export function loadSession(payload: SessionsLoadPayload): Promise<SessionRecordResponse> {
  return invokeIpc<SessionsLoadPayload, SessionRecordResponse>('sessions_load', payload);
}

export type SessionsRenamePayload = {
  scope: SessionScope;
  sessionId: string;
  title: string;
};

export function renameSession(payload: SessionsRenamePayload): Promise<SessionSummaryResponse> {
  return invokeIpc<SessionsRenamePayload, SessionSummaryResponse>('sessions_rename', payload);
}

export type SessionsArchivePayload = {
  scope: SessionScope;
  sessionId: string;
  /** `true` archives, `false` unarchives. Idempotent. */
  archived: boolean;
};

export function archiveSession(payload: SessionsArchivePayload): Promise<SessionSummaryResponse> {
  return invokeIpc<SessionsArchivePayload, SessionSummaryResponse>('sessions_archive', payload);
}

export type SessionsDeletePayload = {
  scope: SessionScope;
  sessionId: string;
};

export type SessionsDeleteResponse = {
  ok: true;
};

/** Permanent: deletes the session and its messages. First call
 * succeeds; a repeat (or unknown id) rejects with `NotFound`. */
export function deleteSession(payload: SessionsDeletePayload): Promise<SessionsDeleteResponse> {
  return invokeIpc<SessionsDeletePayload, SessionsDeleteResponse>('sessions_delete', payload);
}

export type SessionsSaveTranscriptPayload = {
  scope: SessionScope;
  sessionId: string;
  /** The complete visible transcript at a stable boundary (never
   * per-token, never with a streaming placeholder). Replaces the
   * persisted snapshot atomically. */
  entries: SessionTranscriptEntry[];
};

export function saveSessionTranscript(
  payload: SessionsSaveTranscriptPayload,
): Promise<SessionSummaryResponse> {
  return invokeIpc<SessionsSaveTranscriptPayload, SessionSummaryResponse>(
    'sessions_save_transcript',
    payload,
  );
}
