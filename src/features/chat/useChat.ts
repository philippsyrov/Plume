// D7.1: streaming chat transcript + send/cancel orchestration.
//
// Flow per send (post-Codex review fix):
//
//   1. Mint a fresh stream id with `mintStreamId()`. The frontend
//      owns the id space for chat streams — this is the only way
//      to guarantee listeners are registered before the backend
//      can possibly emit a `chat.token` event (Tauri events are
//      not replayed, so any event emitted before listen() resolves
//      would be lost).
//
//   2. Append a user turn + an in-progress streaming entry to the
//      transcript. Initialise the per-stream `StreamGuard` that
//      tracks expected seq + a pending-events buffer.
//
//   3. Await `subscribeChatStream(streamId, ...)`. Both
//      `chat.token` and `chat.done` listeners are now live and
//      filtered to this id.
//
//   4. Await `startChatStream({ streamId, ... })`. From here on
//      events arrive on the channel; the IPC return value just
//      confirms the backend accepted the registration.
//
//   5. Token events: `seq` is enforced per
//      `docs/IPC_CONTRACT.md § Event sequencing`. Out-of-order
//      events buffer (small cap), duplicates drop silently, gaps
//      detected at done time mark the stream corrupt.
//
//   6. `chat.done`: terminal. Flip the streaming entry to its
//      final shape (message / cancelled / error), detach
//      listeners, drop the guard, return the hook to idle.
//
//   7. `cancel()`: calls `chat.cancel(streamId)`. The backend stops
//      emitting tokens and fires a final
//      `chat.done { finish: 'cancelled' }` event. Any tokens that
//      were already in the kernel buffer between the flag flip and
//      the loop check are still applied — that's the documented
//      cooperative-cancel limit.
//
// What the hook deliberately does NOT do:
//   * persist anything to disk — closing the project drops the
//     transcript. Persistence lands with the session/memory verbs
//     reserved in `docs/IPC_ROADMAP.md`.
//   * forcibly abort the in-flight HTTP read.

import { useCallback, useEffect, useRef, useState } from 'react';

import { ipcErrorMessage, isIpcError } from '../../lib/api/errors';
import {
  cancelChatStream,
  mintStreamId,
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

/// Per-stream sequencing guard. Lives in a ref so handlers always
/// see the latest snapshot; one of these is created on `send` and
/// dropped on terminal `chat.done` / `clear` / corruption.
///
/// The contract (`docs/IPC_CONTRACT.md § Event sequencing`):
///   * `seq` is monotonic per stream id, starting at 0.
///   * Duplicates are dropped silently.
///   * Out-of-order events buffer until they can be applied in
///     order.
///   * Gaps that cannot close mark the stream corrupt.
///
/// `chat.done.seq` is the count of `chat.token` events emitted, so
/// once `expectedSeq === done.seq` and `pending` is empty we know
/// every token has been applied.
type StreamGuard = {
  streamId: ChatStreamId;
  expectedSeq: number;
  /** seq → delta, for events that arrived ahead of order. */
  pending: Map<number, string>;
  /** Terminal event held while we wait for missing tokens. */
  heldDone: ChatDoneEvent | null;
};

/// Cap on the out-of-order buffer. Tauri events are normally
/// in-order per origin so this should never fill in practice. A
/// runaway buffer means something is structurally broken; we
/// surface that as corruption instead of letting it grow.
const PENDING_CAP = 256;

export function useChat(): ChatApi {
  const [entries, setEntries] = useState<ChatEntry[]>([]);
  const [status, setStatus] = useState<ChatStatus>('idle');
  const [lastError, setLastError] = useState<string | null>(null);
  const [activeStreamId, setActiveStreamId] = useState<ChatStreamId | null>(null);

  // Refs for handler bodies to read latest state without re-binding
  // listeners on every render.
  const statusRef = useRef<ChatStatus>('idle');
  const entriesRef = useRef<ChatEntry[]>([]);
  const activeIdRef = useRef<ChatStreamId | null>(null);
  const unlistenRef = useRef<UnlistenFn | null>(null);
  const guardRef = useRef<StreamGuard | null>(null);
  statusRef.current = status;
  entriesRef.current = entries;
  activeIdRef.current = activeStreamId;

  // Detach listeners on unmount in case `clear` / `done` didn't fire.
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

  const applyDelta = useCallback((streamId: ChatStreamId, delta: string) => {
    setEntries((prev) =>
      prev.map((e) =>
        e.kind === 'streaming' && e.streamId === streamId
          ? {
              ...e,
              content: e.content + delta,
              tokenCount: e.tokenCount + 1,
            }
          : e,
      ),
    );
  }, []);

  const finalizeStream = useCallback(
    (event: ChatDoneEvent) => {
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
          return {
            kind: 'error',
            message:
              event.error ?? `Chat stream ended with finish='${event.finish}' and no message.`,
          };
        }),
      );
      detachListeners();
      setActiveStreamId(null);
      guardRef.current = null;
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

  const markCorrupt = useCallback(
    (streamId: ChatStreamId, reason: string) => {
      // Replace the in-progress streaming entry with an error row
      // so the user sees something happened. Fire-and-forget cancel
      // on the backend so it doesn't keep streaming uselessly.
      setEntries((prev) =>
        prev.map((e): ChatEntry =>
          e.kind === 'streaming' && e.streamId === streamId
            ? { kind: 'error', message: `Stream corrupted: ${reason}` }
            : e,
        ),
      );
      detachListeners();
      setActiveStreamId(null);
      guardRef.current = null;
      setStatus('error');
      setLastError(`Stream corrupted: ${reason}`);
      void cancelChatStream({ streamId }).catch(() => {
        /* best-effort, idempotent on the backend */
      });
    },
    [detachListeners],
  );

  /// Walk `pending` while it has the next expected seq; apply each
  /// delta in order and advance `expectedSeq`.
  const drainPending = useCallback(
    (guard: StreamGuard) => {
      while (guard.pending.has(guard.expectedSeq)) {
        const delta = guard.pending.get(guard.expectedSeq)!;
        guard.pending.delete(guard.expectedSeq);
        applyDelta(guard.streamId, delta);
        guard.expectedSeq += 1;
      }
    },
    [applyDelta],
  );

  const onToken = useCallback(
    (event: ChatTokenEvent) => {
      const guard = guardRef.current;
      if (guard === null || guard.streamId !== event.id) return;
      // Duplicate token: seq is in the past or already pending.
      if (event.seq < guard.expectedSeq || guard.pending.has(event.seq)) return;

      if (event.seq === guard.expectedSeq) {
        applyDelta(guard.streamId, event.delta);
        guard.expectedSeq += 1;
        drainPending(guard);
        // If a `chat.done` was held back waiting for in-order
        // tokens, fire it now if we've caught up.
        if (guard.heldDone !== null && guard.heldDone.seq === guard.expectedSeq) {
          const held = guard.heldDone;
          guard.heldDone = null;
          finalizeStream(held);
        }
      } else {
        // Future seq — buffer until the gap closes. PENDING_CAP
        // protects against a structural break (e.g. the backend
        // skipped a seq) that would otherwise grow this map
        // unboundedly.
        if (guard.pending.size >= PENDING_CAP) {
          markCorrupt(
            guard.streamId,
            `out-of-order chat.token buffer exceeded ${PENDING_CAP} events`,
          );
          return;
        }
        guard.pending.set(event.seq, event.delta);
      }
    },
    [applyDelta, drainPending, finalizeStream, markCorrupt],
  );

  const onDone = useCallback(
    (event: ChatDoneEvent) => {
      const guard = guardRef.current;
      if (guard === null || guard.streamId !== event.id) return;
      // Duplicate done (or done that pre-dates already-applied
      // tokens) — drop, idempotent.
      if (event.seq < guard.expectedSeq) return;

      // Drain anything in `pending` that's now in-order. If a
      // `chat.done` arrived before the last token events, the
      // tokens are very likely already buffered.
      drainPending(guard);

      if (event.seq === guard.expectedSeq) {
        finalizeStream(event);
      } else if (event.seq > guard.expectedSeq) {
        // Hold the done until the missing tokens fill the gap.
        // Sanity check: if `pending` doesn't contain enough events
        // to ever close the gap, the stream is corrupt.
        const haveCount = guard.expectedSeq + guard.pending.size;
        if (haveCount < event.seq) {
          markCorrupt(
            guard.streamId,
            `chat.done announced seq=${event.seq} but only ${haveCount} chat.token events received`,
          );
          return;
        }
        guard.heldDone = event;
      }
    },
    [drainPending, finalizeStream, markCorrupt],
  );

  const send = useCallback(
    async (providerId: string, modelId: string, content: string): Promise<boolean> => {
      if (statusRef.current === 'streaming') return false;
      const trimmed = content.trim();
      if (trimmed.length === 0) return false;

      const userMessage: ChatMessage = { role: 'user', content: trimmed };
      const transcript: ChatMessage[] = [
        ...entriesRef.current
          .filter(
            (e): e is Extract<ChatEntry, { kind: 'message' }> => e.kind === 'message',
          )
          .map((e) => e.message),
        userMessage,
      ];

      // 1. Mint the id locally. From here on the backend will use
      //    whatever we hand it.
      const streamId = mintStreamId();

      // 2. Push the visible transcript rows + initialise the guard
      //    BEFORE subscribing. The streaming entry has to exist so
      //    `applyDelta` can find it on the first token; the guard
      //    has to exist so the listeners can do their seq checks.
      setStatus('streaming');
      setLastError(null);
      setActiveStreamId(streamId);
      guardRef.current = {
        streamId,
        expectedSeq: 0,
        pending: new Map(),
        heldDone: null,
      };
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

      // 3. Subscribe BEFORE sending. `await listen(...)` resolves
      //    only after Tauri has registered the handler, so any
      //    event the backend emits after `startChatStream` returns
      //    is guaranteed to reach us.
      let unlisten: UnlistenFn;
      try {
        unlisten = await subscribeChatStream(streamId, { onToken, onDone });
        unlistenRef.current = unlisten;
      } catch (err) {
        // Listener setup itself failed (rare). Surface inline and
        // never fire the send.
        const message = formatError(err);
        setEntries((prev) =>
          prev.map((e): ChatEntry =>
            e.kind === 'streaming' && e.streamId === streamId
              ? { kind: 'error', message }
              : e,
          ),
        );
        setStatus('error');
        setLastError(message);
        setActiveStreamId(null);
        guardRef.current = null;
        return true;
      }

      // 4. Send the actual request. If the backend rejects
      //    synchronously (bad provider, validation, duplicate id),
      //    flip the in-progress entry to an error row and tear
      //    down.
      try {
        await startChatStream({
          streamId,
          providerId,
          modelId,
          messages: transcript,
        });
        return true;
      } catch (err) {
        const message = formatError(err);
        setEntries((prev) =>
          prev.map((e): ChatEntry =>
            e.kind === 'streaming' && e.streamId === streamId
              ? { kind: 'error', message }
              : e,
          ),
        );
        setStatus('error');
        setLastError(message);
        detachListeners();
        setActiveStreamId(null);
        guardRef.current = null;
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
      const message = formatError(err);
      setLastError(message);
    }
  }, []);

  const clear = useCallback(() => {
    detachListeners();
    setEntries([]);
    setStatus('idle');
    setLastError(null);
    setActiveStreamId(null);
    guardRef.current = null;
  }, [detachListeners]);

  return { entries, status, lastError, activeStreamId, send, cancel, clear };
}

function formatError(err: unknown): string {
  if (isIpcError(err)) return ipcErrorMessage(err);
  if (err instanceof Error) return err.message;
  return 'Chat request failed for an unknown reason.';
}
