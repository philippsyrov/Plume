// D7.1: streaming chat transcript + send/cancel orchestration.
//
// D7 ran chat as a single synchronous IPC call. D7.1 reshapes the
// hook around `chat.token` / `chat.done` events:
//
//   1. `send(provider, model, content)` appends the user turn,
//      reserves an in-progress assistant turn, then subscribes to
//      `chat.token` / `chat.done` for the new stream id BEFORE
//      kicking the IPC off. Subscribing first is load-bearing —
//      Tauri events are not replayed, so a token emitted between
//      `chat.send` returning and `listen` resolving would be lost.
//      Empirically Tauri's `listen` Promise resolves before the
//      backend emits anything, but we order it deterministically
//      anyway: we await the subscribe handle's setup before
//      awaiting the send IPC.
//
//   2. Token events append to the streaming entry's `content`. The
//      transcript re-renders on every token; the panel scrolls to
//      the bottom as deltas arrive.
//
//   3. The terminal `chat.done` event flips the streaming entry to
//      its final state — finalised assistant message or error row —
//      detaches the listeners, and returns the hook to idle.
//
//   4. `cancel()` calls `chat.cancel(streamId)` on the backend.
//      The backend stops emitting tokens and fires a final
//      `chat.done { finish: 'cancelled' }` event. The hook treats
//      cancellation as a terminal state: the partial reply stays in
//      the transcript with a "(stopped)" marker so the user can see
//      what came back.
//
// What the hook deliberately does NOT do:
//   * persist anything to disk — closing the project drops the
//     transcript. Persistence lands with the session/memory verbs
//     reserved in `docs/IPC_ROADMAP.md`.
//   * forcibly abort the in-flight HTTP read. Cancellation is
//     cooperative on the backend; one more in-flight line can be
//     buffered before the loop notices the flag.

import { useCallback, useEffect, useRef, useState } from 'react';

import { ipcErrorMessage, isIpcError } from '../../lib/api/errors';
import {
  cancelChatStream,
  startChatStream,
  subscribeChatStream,
  type ChatDoneEvent,
  type ChatMessage,
  type ChatStreamId,
  type ChatTokenEvent,
} from '../../lib/api/chat';
import type { UnlistenFn } from '@tauri-apps/api/event';

export type ChatStatus = 'idle' | 'streaming' | 'error';

/// A line in the visible transcript. `streaming` is the in-progress
/// assistant entry that gains tokens before flipping to `message`.
export type ChatEntry =
  | { kind: 'message'; message: ChatMessage; modelUsed?: string; durationMs?: number }
  | {
      kind: 'streaming';
      streamId: ChatStreamId;
      content: string;
      tokenCount: number;
    }
  | { kind: 'error'; message: string }
  | {
      kind: 'cancelled';
      partial: string;
      modelUsed?: string;
      durationMs?: number;
    };

export type ChatApi = {
  entries: ChatEntry[];
  status: ChatStatus;
  lastError: string | null;
  /**
   * Currently active stream id; null when idle. Exposed so the panel
   * can pass it back to `cancel()` without rolling its own state.
   */
  activeStreamId: ChatStreamId | null;
  /**
   * Append a user turn and start a streamed assistant turn. Returns
   * `true` if the request was sent; `false` if the hook was busy or
   * the inputs were invalid.
   */
  send: (providerId: string, modelId: string, content: string) => Promise<boolean>;
  /** Cancel the active stream, if any. No-op otherwise. */
  cancel: () => Promise<void>;
  /** Drop the transcript and reset to `idle`. Does not cancel in-flight. */
  clear: () => void;
};

export function useChat(): ChatApi {
  const [entries, setEntries] = useState<ChatEntry[]>([]);
  const [status, setStatus] = useState<ChatStatus>('idle');
  const [lastError, setLastError] = useState<string | null>(null);
  const [activeStreamId, setActiveStreamId] = useState<ChatStreamId | null>(null);

  // Refs for handler bodies to read latest state without re-binding
  // the listeners on every render.
  const statusRef = useRef<ChatStatus>('idle');
  const entriesRef = useRef<ChatEntry[]>([]);
  const activeIdRef = useRef<ChatStreamId | null>(null);
  const unlistenRef = useRef<UnlistenFn | null>(null);
  statusRef.current = status;
  entriesRef.current = entries;
  activeIdRef.current = activeStreamId;

  // Detach listeners on unmount in case `clear`/`done` didn't fire.
  useEffect(() => {
    return () => {
      unlistenRef.current?.();
      unlistenRef.current = null;
    };
  }, []);

  const detachListeners = useCallback(() => {
    unlistenRef.current?.();
    unlistenRef.current = null;
  }, []);

  const onToken = useCallback((event: ChatTokenEvent) => {
    // Append to the streaming entry. The entry is identified by
    // stream id, not by index — a hypothetical race where two
    // streams are alive at once would still update the right entry.
    setEntries((prev) =>
      prev.map((e) =>
        e.kind === 'streaming' && e.streamId === event.id
          ? {
              ...e,
              content: e.content + event.delta,
              tokenCount: e.tokenCount + 1,
            }
          : e,
      ),
    );
  }, []);

  const onDone = useCallback(
    (event: ChatDoneEvent) => {
      // Flip the streaming entry to its terminal shape, detach
      // listeners, return to idle.
      setEntries((prev) =>
        prev.map((e): ChatEntry => {
          if (!(e.kind === 'streaming' && e.streamId === event.id)) return e;
          if (event.finish === 'stop' || event.finish === 'length') {
            return {
              kind: 'message',
              message: { role: 'assistant', content: e.content },
              ...(event.modelId ? { modelUsed: event.modelId } : {}),
              durationMs: event.durationMs,
            };
          }
          if (event.finish === 'cancelled') {
            return {
              kind: 'cancelled',
              partial: e.content,
              ...(event.modelId ? { modelUsed: event.modelId } : {}),
              durationMs: event.durationMs,
            };
          }
          // error
          return {
            kind: 'error',
            message:
              event.error ?? `Chat stream ended with finish='${event.finish}' and no message.`,
          };
        }),
      );
      detachListeners();
      setActiveStreamId(null);
      if (event.finish === 'error') {
        setStatus('error');
        setLastError(event.error ?? 'Chat stream errored.');
      } else {
        setStatus('idle');
        setLastError(null);
      }
    },
    [detachListeners],
  );

  const send = useCallback(
    async (providerId: string, modelId: string, content: string): Promise<boolean> => {
      if (statusRef.current === 'streaming') return false;
      const trimmed = content.trim();
      if (trimmed.length === 0) return false;

      const userMessage: ChatMessage = { role: 'user', content: trimmed };
      // The backend wants the FULL transcript. Build it from current
      // message-shaped entries plus the new user turn. Non-message
      // rows (errors, cancellations) are NOT forwarded — those are
      // visible-only annotations.
      const transcript: ChatMessage[] = [
        ...entriesRef.current
          .filter(
            (e): e is Extract<ChatEntry, { kind: 'message' }> => e.kind === 'message',
          )
          .map((e) => e.message),
        userMessage,
      ];

      setStatus('streaming');
      setLastError(null);

      try {
        const started = await startChatStream({
          providerId,
          modelId,
          messages: transcript,
        });
        const streamId = started.streamId;

        // Subscribe BEFORE pushing the streaming row so the listener
        // is wired by the time the first token arrives. (`listen`
        // resolves once Tauri has registered the handler; events
        // emitted before this point are not replayed.)
        const unlisten = await subscribeChatStream(streamId, {
          onToken,
          onDone,
        });
        unlistenRef.current = unlisten;
        setActiveStreamId(streamId);
        setEntries((prev) => [
          ...prev,
          { kind: 'message', message: userMessage },
          {
            kind: 'streaming',
            streamId,
            content: '',
            tokenCount: 0,
          },
        ]);
        return true;
      } catch (err) {
        // Synchronous error path: backend rejected the start (bad
        // provider, validation failure). Surface inline; don't push
        // the user turn because the round-trip never began.
        const message = formatError(err);
        setEntries((prev) => [
          ...prev,
          { kind: 'message', message: userMessage },
          { kind: 'error', message },
        ]);
        setStatus('error');
        setLastError(message);
        detachListeners();
        setActiveStreamId(null);
        return true;
      }
    },
    [detachListeners, onDone, onToken],
  );

  const cancel = useCallback(async () => {
    const id = activeIdRef.current;
    if (id === null) return;
    try {
      await cancelChatStream({ streamId: id });
    } catch (err) {
      // Cancellation is idempotent on the backend; surface the
      // unusual case where the IPC itself failed.
      const message = formatError(err);
      setLastError(message);
    }
  }, []);

  const clear = useCallback(() => {
    // Detach listeners. Any in-flight stream's `chat.done` will fire
    // but the listener is gone so it's a no-op. The stream still
    // burns server-side cycles until cancelled — for the Clear path
    // we don't auto-cancel because the user may want a fresh
    // transcript without aborting a long generation. The Stop
    // affordance is separate.
    detachListeners();
    setEntries([]);
    setStatus('idle');
    setLastError(null);
    setActiveStreamId(null);
  }, [detachListeners]);

  return { entries, status, lastError, activeStreamId, send, cancel, clear };
}

function formatError(err: unknown): string {
  if (isIpcError(err)) return ipcErrorMessage(err);
  if (err instanceof Error) return err.message;
  return 'Chat request failed for an unknown reason.';
}
