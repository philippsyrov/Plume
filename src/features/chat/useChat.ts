// D7: window-local chat transcript + send orchestration.
//
// Lifts the chat state out of `ChatPanel` so the component stays
// rendering-only and the orchestration (in-flight tracking,
// generation counter, error handling) is testable in one place.
//
// What the hook owns:
//   * `messages` — the user/assistant transcript as displayed.
//   * `status` — idle | sending | error. Sending blocks new input.
//   * `lastError` — the most recent failure message, kept alongside
//                   the transcript so a retry shows the user what
//                   went wrong without losing history.
//
// What the hook deliberately does NOT do:
//   * persist anything to disk — closing the project drops the
//     transcript. The session/memory verbs that would persist
//     conversations are reserved in `docs/IPC_ROADMAP.md`.
//   * cancel an in-flight call. The backend is non-streaming and
//     a chat round-trip can take dozens of seconds; cancellation
//     lands with the streaming verbs in D7.1.
//   * mix the selected model into the transcript shape. Each turn
//     is just `{ role, content }` — the UI can annotate with
//     "served by X" using the response's `modelId`, but we don't
//     bake that into the message type.

import { useCallback, useRef, useState } from 'react';

import { ipcErrorMessage, isIpcError } from '../../lib/api/errors';
import {
  sendChat,
  type ChatMessage,
  type ChatResponse,
} from '../../lib/api/chat';

export type ChatStatus = 'idle' | 'sending' | 'error';

/// A line in the visible transcript. Errors are tracked alongside the
/// transcript so a failure is rendered inline; flipping `kind` keeps
/// the union exhaustively checkable when the UI grows.
export type ChatEntry =
  | { kind: 'message'; message: ChatMessage; modelUsed?: string; durationMs?: number }
  | { kind: 'error'; message: string };

export type ChatApi = {
  entries: ChatEntry[];
  status: ChatStatus;
  /** Most recent error message; null when the last call succeeded. */
  lastError: string | null;
  /**
   * Append a user turn and request the matching assistant turn from
   * the backend. Returns `true` if the request was sent (i.e. the
   * hook was idle and the inputs validated); `false` otherwise so
   * the caller does not double-fire.
   */
  send: (providerId: string, modelId: string, content: string) => Promise<boolean>;
  /** Drop the transcript and reset to `idle`. Does not cancel in-flight. */
  clear: () => void;
};

export function useChat(): ChatApi {
  const [entries, setEntries] = useState<ChatEntry[]>([]);
  const [status, setStatus] = useState<ChatStatus>('idle');
  const [lastError, setLastError] = useState<string | null>(null);
  // Generation counter: each send bumps it; a stale response (we
  // refuse to ever have more than one in flight, but defense in
  // depth) checks the counter before mutating state.
  const generationRef = useRef(0);
  // Refs so handlers don't read stale values when a render is in
  // flight (React 19 batches but reading state from inside a tick
  // can still be slightly behind).
  const statusRef = useRef<ChatStatus>('idle');
  const entriesRef = useRef<ChatEntry[]>([]);
  entriesRef.current = entries;
  statusRef.current = status;

  const send = useCallback(
    async (providerId: string, modelId: string, content: string): Promise<boolean> => {
      if (statusRef.current === 'sending') return false;
      const trimmed = content.trim();
      if (trimmed.length === 0) return false;

      const gen = ++generationRef.current;
      const userMessage: ChatMessage = { role: 'user', content: trimmed };
      const nextEntries: ChatEntry[] = [
        ...entriesRef.current,
        { kind: 'message', message: userMessage },
      ];
      setEntries(nextEntries);
      setStatus('sending');
      setLastError(null);

      // The backend wants the FULL transcript (Ollama is stateless
      // across /api/chat calls), but the transcript may include error
      // rows we shouldn't forward. Filter to message entries only.
      const transcript: ChatMessage[] = nextEntries
        .filter((e): e is Extract<ChatEntry, { kind: 'message' }> => e.kind === 'message')
        .map((e) => e.message);

      let response: ChatResponse | null = null;
      let failure: string | null = null;
      try {
        response = await sendChat({ providerId, modelId, messages: transcript });
      } catch (err) {
        failure = formatError(err);
      }

      // Ignore the result if the user cleared (which bumps the
      // generation indirectly via setStatus, but we'd also like to be
      // robust to a future "cancel" affordance).
      if (gen !== generationRef.current) return true;

      if (response) {
        setEntries((prev) => [
          ...prev,
          {
            kind: 'message',
            message: response.message,
            modelUsed: response.modelId,
            durationMs: response.durationMs,
          },
        ]);
        setStatus('idle');
        setLastError(null);
      } else if (failure) {
        setEntries((prev) => [...prev, { kind: 'error', message: failure }]);
        setStatus('error');
        setLastError(failure);
      }
      return true;
    },
    [],
  );

  const clear = useCallback(() => {
    // Bumping the counter lets any straggling in-flight resolve
    // become a no-op against the post-clear state.
    generationRef.current += 1;
    setEntries([]);
    setStatus('idle');
    setLastError(null);
  }, []);

  return { entries, status, lastError, send, clear };
}

function formatError(err: unknown): string {
  if (isIpcError(err)) return ipcErrorMessage(err);
  if (err instanceof Error) return err.message;
  return 'Chat request failed for an unknown reason.';
}
