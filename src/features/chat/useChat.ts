// D7.1: streaming chat transcript + send/cancel orchestration.
//
// Flow per send (post-Codex review fix):
//
//   1. Mint a fresh stream id with `mintStreamId()`. The frontend
//      owns the id space for chat streams — this is the only way
//      to guarantee listeners are registered before the backend
//      can possibly emit a `chat/token` event (Tauri events are
//      not replayed, so any event emitted before listen() resolves
//      would be lost).
//
//   2. Append a user turn + an in-progress streaming entry to the
//      transcript. Initialise the per-stream `StreamGuard` that
//      tracks expected seq + a pending-events buffer.
//
//   3. Await `subscribeChatStream(streamId, ...)`. Both
//      `chat/token` and `chat/done` listeners are now live and
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
//   6. `chat/done`: terminal. Flip the streaming entry to its
//      final shape (message / cancelled / error), detach
//      listeners, drop the guard, return the hook to idle.
//
//   7. `cancel()`: calls `chat.cancel(streamId)`. The backend stops
//      emitting tokens and fires a final
//      `chat/done { finish: 'cancelled' }` event. Any tokens that
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
  type ChatAttachment,
  type ChatDoneEvent,
  type ChatMemoryUsage,
  type ChatTopicsUsage,
  type ChatMessage,
  type ChatMode,
  type ChatStats,
  type ChatStreamId,
  type ChatTokenEvent,
} from '../../lib/api/chat';
import type { UnlistenFn } from '@tauri-apps/api/event';

export type ChatStatus = 'idle' | 'streaming' | 'error';

/// A line in the visible transcript. `streaming` is the in-progress
/// assistant entry that gains tokens before flipping to `message`.
///
/// A `user` `message` carries an optional `attachmentRelPath` so the
/// transcript can render the chip in-line with the turn that
/// included it. The chip is purely visual — the file content itself
/// lives only on the wire (backend-side prompt assembly), never in
/// frontend state.
export type ChatEntry =
  | {
      kind: 'message';
      message: ChatMessage;
      modelUsed?: string;
      durationMs?: number;
      attachmentRelPath?: string;
      /** D10: optional line range that rode with the attachment.
       * Present only when the user attached a selection rather
       * than the whole file. The chip in the transcript renders
       * `relPath:start–end` when set, `relPath` alone otherwise. */
      attachmentLineRange?: { startLine: number; endLine: number };
      /** D9: generation telemetry, present on completed assistant
       * turns when the runtime reported metrics. */
      stats?: ChatStats;
      /** D15: mode this turn was sent with. Only set on user turns
       * (the model's reply role doesn't carry mode — the mode
       * shaped the request, not the response itself). Lets the
       * panel render assistant replies that followed a
       * `'proposeDiff'` send as diff previews, even after the
       * user has flipped the mode toggle back to chat. */
      sentInMode?: ChatMode;
    }
  | {
      kind: 'streaming';
      streamId: ChatStreamId;
      content: string;
      tokenCount: number;
      /** D15: mode the streaming response was requested with.
       * Carried so finalisation can stamp the completed
       * assistant turn with the matching mode for renderer
       * dispatch. */
      sentInMode?: ChatMode;
    }
  | { kind: 'error'; message: string }
  | {
      kind: 'cancelled';
      partial: string;
      modelUsed?: string;
      durationMs?: number;
    };

export type SendOptions = {
  /**
   * Optional read-only project-file attachment. Forwarded verbatim
   * to the backend in `chat.send` and rendered as a chip on the
   * user turn it was sent with.
   */
  attachment?: ChatAttachment;
  /**
   * D15: response-shape mode for this send. Omit (or pass
   * `'chat'`) for the existing free-form path. `'proposeDiff'`
   * pins the model to a unified-diff response that the chat panel
   * renders with per-line coloring. Carried on the user turn so
   * the transcript shows which mode that turn was sent with.
   */
  mode?: ChatMode;
  /**
   * D46: MLX server handle id from `providers.startServer`.
   * Forwarded verbatim to the backend; required when
   * `providerId === 'mlx-lm'` and ignored otherwise. The chat
   * panel reads the handle from `useMlxServers.handleOf(modelId)`
   * and threads it through here.
   */
  handleId?: string;
  /**
   * Defaults to true. No-project chat passes false so the backend
   * does not fold in AGENTS.md or memory from the last trusted
   * project it still remembers.
   */
  includeProjectContext?: boolean;
};

/// D14: discriminated outcome from `send()` so the caller can
/// react to synchronous rejections without watching the transcript
/// for a freshly-appended error row.
///
/// * `'accepted'` — the backend accepted the request and a stream
///   is in flight (or already finished). The chat panel can safely
///   treat the attachment chip as "consumed" and clear it.
/// * `'rejected'` — the backend rejected synchronously (provider
///   down, validation, etc.). The transcript already carries an
///   error row; the chat panel should RESTORE the chip so the
///   user doesn't have to re-attach the same file before retrying.
/// * `'busy'` — the hook is already streaming a previous turn.
/// * `'empty'` — the input was blank or whitespace-only.
///
/// Before D14 `send` returned `boolean`, where `true` covered both
/// `'accepted'` and `'rejected'` because the hook handled the
/// error inline. That left the caller no way to differentiate
/// "I lost my attachment because Ollama is down" from "I lost my
/// attachment because the model is generating now" — both looked
/// like `true`. The discriminated outcome closes that gap.
export type SendOutcome = 'accepted' | 'rejected' | 'busy' | 'empty';

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
   * D11: whether the most recent accepted `chat.send` confirmed
   * that the project's `AGENTS.md` was folded in as a system
   * message. `null` until the first send is accepted, then
   * `true`/`false` reflecting the backend's report. The chat panel
   * uses this to flip its badge between "available" (forward-
   * looking promise from `meta.hasAgentsMd`) and "included" /
   * "skipped" (after-the-fact confirmation) — so the indicator
   * never claims included from project metadata alone.
   *
   * Reset to `null` on `clear()`. A synchronous send rejection
   * (no response received) does NOT update this value.
   */
  lastInstructionsIncluded: boolean | null;
  /**
   * D42: confirmed memory summary echoed by the most recent
   * accepted `chat.send`. `null` until the first successful
   * accept (the `MemoryBadge` falls back to the forward-looking
   * `chat.context` preview in that case) and on every honest
   * skip — no trusted project, no memory store, store
   * unreadable, no entries.
   *
   * Reset to `null` on `clear()`. A synchronous send rejection
   * does NOT update this value.
   */
  lastMemoryUsed: ChatMemoryUsage | null;
  /**
   * D72: confirmed curated topic-file summary echoed by the most
   * recent accepted `chat.send`. Same posture as `lastMemoryUsed` —
   * `null` until the first accept (the `TopicsBadge` falls back to
   * the `chat.context` preview) and on every honest skip. Reset on
   * `clear()`.
   */
  lastTopicsUsed: ChatTopicsUsage | null;
  /**
   * Append a user turn and start a streamed assistant turn. The
   * returned `SendOutcome` lets the caller distinguish a
   * synchronous backend reject (e.g. Ollama down) from "the hook
   * is busy" or "you gave me empty input" — the chat panel uses
   * the `'rejected'` outcome to restore a chip that would
   * otherwise be silently consumed.
   */
  send: (
    providerId: string,
    modelId: string,
    content: string,
    options?: SendOptions,
  ) => Promise<SendOutcome>;
  /** Cancel the active stream, if any. No-op otherwise. */
  cancel: () => Promise<void>;
  /** Drop the transcript and reset to `idle`. Does not cancel in-flight. */
  clear: () => void;
  /**
   * D63B: replace the transcript with a restored session snapshot
   * (loaded from `sessions.load`). Same reset semantics as `clear()`
   * — stream state, error, and the confirmation badges all drop —
   * but the installed entries are the restored ones. Ignored while
   * streaming: the session layer blocks switching during a stream,
   * so this guard is defense in depth, not a reachable UX path.
   */
  restore: (entries: ChatEntry[]) => void;
};

/// Per-stream sequencing guard. Lives in a ref so handlers always
/// see the latest snapshot; one of these is created on `send` and
/// dropped on terminal `chat/done` / `clear` / corruption.
///
/// The contract (`docs/IPC_CONTRACT.md § Event sequencing`):
///   * `seq` is monotonic per stream id, starting at 0.
///   * Duplicates are dropped silently.
///   * Out-of-order events buffer until they can be applied in
///     order.
///   * Gaps that cannot close mark the stream corrupt.
///
/// `chat/done.seq` is the count of `chat/token` events emitted, so
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
  // D11: latest accepted send's instructions confirmation. `null`
  // means "no send has resolved yet"; the badge renders that as
  // "available" rather than "included".
  const [lastInstructionsIncluded, setLastInstructionsIncluded] = useState<boolean | null>(
    null,
  );
  // D42: latest accepted send's memory summary. `null` covers
  // "no send yet" AND "send went out but no memory was folded in"
  // (no project, no store, no entries). The `MemoryBadge` falls
  // back to the chat.context preview while this is `null`.
  const [lastMemoryUsed, setLastMemoryUsed] = useState<ChatMemoryUsage | null>(null);
  // D72: latest accepted send's curated topic-file summary. Same
  // posture as `lastMemoryUsed`.
  const [lastTopicsUsed, setLastTopicsUsed] = useState<ChatTopicsUsage | null>(null);

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
          // D15: carry the requesting mode onto the completed
          // assistant turn so the renderer can dispatch a diff
          // view for `'proposeDiff'` even after the user has
          // flipped the mode toggle back to `'chat'`.
          const sentInMode = e.sentInMode;
          if (event.finish === 'stop' || event.finish === 'length') {
            return {
              kind: 'message',
              message: { role: 'assistant', content: e.content },
              ...(event.modelId ? { modelUsed: event.modelId } : {}),
              durationMs: event.durationMs,
              // D9: stats only ride on 'stop' from Ollama today,
              // but the wire shape allows them on any finish; we
              // attach when present and let the renderer decide.
              ...(event.stats ? { stats: event.stats } : {}),
              ...(sentInMode ? { sentInMode } : {}),
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
        // If a `chat/done` was held back waiting for in-order
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
            `out-of-order chat/token buffer exceeded ${PENDING_CAP} events`,
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
      // `chat/done` arrived before the last token events, the
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
            `chat/done announced seq=${event.seq} but only ${haveCount} chat/token events received`,
          );
          return;
        }
        guard.heldDone = event;
      }
    },
    [drainPending, finalizeStream, markCorrupt],
  );

  const send = useCallback(
    async (
      providerId: string,
      modelId: string,
      content: string,
      options?: SendOptions,
    ): Promise<SendOutcome> => {
      if (statusRef.current === 'streaming') return 'busy';
      const trimmed = content.trim();
      if (trimmed.length === 0) return 'empty';

      const attachment = options?.attachment;
      const mode: ChatMode = options?.mode ?? 'chat';
      const handleId = options?.handleId;
      const includeProjectContext = options?.includeProjectContext ?? true;
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
      //    The user row carries the attachment chip alongside it
      //    so the transcript shows which turn included context.
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
        {
          kind: 'message',
          message: userMessage,
          ...(attachment ? { attachmentRelPath: attachment.relPath } : {}),
          ...(attachment &&
          typeof attachment.startLine === 'number' &&
          typeof attachment.endLine === 'number'
            ? {
                attachmentLineRange: {
                  startLine: attachment.startLine,
                  endLine: attachment.endLine,
                },
              }
            : {}),
          // D15: tag the user turn with its mode so the
          // transcript can render a "(Propose diff)" hint inline
          // and the matching assistant entry knows how to render.
          ...(mode !== 'chat' ? { sentInMode: mode } : {}),
        },
        {
          kind: 'streaming',
          streamId,
          content: '',
          tokenCount: 0,
          // Carry the mode onto the streaming entry; finaliseStream
          // copies it onto the completed `'message'` entry so
          // mode dispatch survives the streaming → message
          // transition.
          ...(mode !== 'chat' ? { sentInMode: mode } : {}),
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
        // never fire the send. Treated as a `'rejected'` outcome
        // so the chat panel restores the attachment chip — the
        // request never reached the backend.
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
        return 'rejected';
      }

      // 4. Send the actual request. If the backend rejects
      //    synchronously (bad provider, validation, duplicate id,
      //    or a prompt-read rejection like `Blocked` for an .env
      //    attachment), flip the in-progress entry to an error
      //    row and tear down.
      try {
        const response = await startChatStream({
          streamId,
          providerId,
          modelId,
          messages: transcript,
          ...(attachment ? { attachment } : {}),
          // D15: only thread `mode` when it's non-default. The
          // backend defaults to `'chat'` on an absent field;
          // omitting it on the wire keeps D7.1-shape sends
          // byte-identical to pre-D15.
          ...(mode !== 'chat' ? { mode } : {}),
          // D46: only thread `handleId` when present. Omitting on
          // Ollama sends keeps the wire byte-identical to pre-D46.
          ...(handleId ? { handleId } : {}),
          ...(includeProjectContext ? {} : { includeProjectContext: false }),
        });
        // D11: backend confirmation that AGENTS.md was (or wasn't)
        // folded into this send. Only updated on a successful
        // synchronous accept; a rejection (caught below) leaves
        // the previous value alone — the chat panel keeps showing
        // whatever the LAST accepted send reported.
        setLastInstructionsIncluded(response.instructionsIncluded);
        // D42: same posture for the memory summary. `response.memory`
        // is `null` when nothing was folded in (no project / no store /
        // no entries) — store that so the badge can render "available"
        // off the preview again instead of stale "included" numbers.
        setLastMemoryUsed(response.memory);
        // D72: same posture for the curated topic-file summary.
        setLastTopicsUsed(response.topics);
        return 'accepted';
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
        return 'rejected';
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
    // Clearing the transcript also clears the instructions
    // confirmation — the badge goes back to "available" until
    // the next send round-trips.
    setLastInstructionsIncluded(null);
    setLastMemoryUsed(null);
    setLastTopicsUsed(null);
    guardRef.current = null;
  }, [detachListeners]);

  const restore = useCallback(
    (restored: ChatEntry[]) => {
      if (statusRef.current === 'streaming') return;
      detachListeners();
      setEntries(restored);
      setStatus('idle');
      setLastError(null);
      setActiveStreamId(null);
      setLastInstructionsIncluded(null);
      setLastMemoryUsed(null);
      setLastTopicsUsed(null);
      guardRef.current = null;
    },
    [detachListeners],
  );

  return {
    entries,
    status,
    lastError,
    activeStreamId,
    lastInstructionsIncluded,
    lastMemoryUsed,
    lastTopicsUsed,
    send,
    cancel,
    clear,
    restore,
  };
}

function formatError(err: unknown): string {
  if (isIpcError(err)) return ipcErrorMessage(err);
  if (err instanceof Error) return err.message;
  if (typeof err === 'string' && err.trim().length > 0) return err;
  try {
    return `Chat request failed: ${JSON.stringify(err)}`;
  } catch {
    return `Chat request failed: ${String(err)}`;
  }
}
