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
  forkedFromSessionId: string | null;
  forkedThroughEntryId: string | null;
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

export function forkSession(payload: SessionsLoadPayload): Promise<SessionRecordResponse> {
  return invokeIpc<SessionsLoadPayload, SessionRecordResponse>('sessions_fork', payload);
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

// D66: full-text search over ONE scope's database. Strict separation
// is structural — searching both surfaces means two calls, and the
// results can never mix inside a single query.

/** Snippet highlight markers (private-use code points, mirroring the
 * backend's `SNIPPET_START`/`SNIPPET_END`). Split on these to render
 * highlights; never show them raw. */
export const SEARCH_SNIPPET_START = '\uE000';
export const SEARCH_SNIPPET_END = '\uE001';

/** Backend result cap; `limit` beyond this is rejected. */
export const MAX_SEARCH_RESULTS = 20;

export type SessionSearchHit = {
  id: string;
  title: string;
  updatedAtMs: number;
  /** `null` for a live session; archived chats stay searchable. */
  archivedAtMs: number | null;
  /** `title` also covers title-AND-content matches. */
  matchKind: 'title' | 'content';
  /** Transcript excerpt with marker-wrapped matches; `null` for a
   * title-only match. */
  snippet: string | null;
};

export type SessionsSearchPayload = {
  scope: SessionScope;
  /** Literal text — FTS operators in it are searched for, never
   * interpreted. Trimmed; 1–200 characters. */
  query: string;
  /** 1..=20 when present; defaults to 20. */
  limit?: number;
};

export type SessionsSearchResponse = {
  /** Title matches first, then content-only matches; bounded. */
  hits: SessionSearchHit[];
};

export function searchSessions(
  payload: SessionsSearchPayload,
): Promise<SessionsSearchResponse> {
  return invokeIpc<SessionsSearchPayload, SessionsSearchResponse>('sessions_search', payload);
}
