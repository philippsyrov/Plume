// Typed wrapper for the streaming `chat.send` + `chat.cancel` IPC
// verbs and the matching `chat.token` / `chat.done` events.
//
// D7 shipped chat as a single synchronous IPC call returning the
// full assistant message. D7.1 reshapes that path: `chat.send` now
// returns a `ChatStreamId` immediately and the assistant reply
// arrives over Tauri events. `chat.cancel(streamId)` flips a
// cooperative cancel flag on the backend.
//
// Contract today:
//   * Ollama is the only wired provider — backend rejects others
//     with `BadArgument`.
//   * `messages` is the full transcript; Ollama is stateless across
//     `/api/chat` calls so the caller concatenates history.
//   * Exactly one `chat.done` event fires per stream id, after
//     which the id becomes invalid; further `cancelChat(id)` is a
//     silent no-op.
//
// See `docs/IPC_CONTRACT.md § chat` for the full shape.

import { listen, type UnlistenFn } from '@tauri-apps/api/event';

import { invokeIpc } from './ipc';

export type ChatRole = 'system' | 'user' | 'assistant' | 'tool';

export type ChatMessage = {
  role: ChatRole;
  content: string;
};

export type ChatFinish = 'stop' | 'length' | 'cancelled' | 'error';

export type ChatStreamId = string;

export type ChatSendStartedResponse = {
  /**
   * Opaque stream id. Subscribers should filter `chat.token` /
   * `chat.done` events by matching `payload.id`.
   */
  streamId: ChatStreamId;
  /** Echoed for routing convenience. */
  providerId: string;
  /** Echoed for routing convenience. */
  modelId: string;
};

export type ChatTokenEvent = {
  id: ChatStreamId;
  /** Monotonic per stream, starting at 0. */
  seq: number;
  /** New text since the previous frame. Caller concatenates. */
  delta: string;
};

export type ChatDoneEvent = {
  id: ChatStreamId;
  /** Equals the count of `chat.token` events the stream emitted. */
  seq: number;
  finish: ChatFinish;
  /** `null` if the stream errored before reading any frame. */
  modelId: string | null;
  /** Backend-measured wall-clock duration in ms. */
  durationMs: number;
  /** Present iff `finish === 'error'`. */
  error: string | null;
};

type ChatSendPayload = {
  providerId: string;
  modelId: string;
  messages: ChatMessage[];
};

type ChatCancelPayload = {
  streamId: ChatStreamId;
};

/// Start a streaming chat. Resolves with the stream id once the
/// backend has accepted the call and started the streaming task —
/// not when the assistant reply finishes. The caller subscribes to
/// `chat.token` / `chat.done` events to receive the actual content.
export function startChatStream(payload: ChatSendPayload): Promise<ChatSendStartedResponse> {
  return invokeIpc<ChatSendPayload, ChatSendStartedResponse>('chat_send', payload);
}

/// Cooperatively cancel an in-flight stream. Idempotent: cancelling
/// a finished or unknown stream resolves successfully. The
/// corresponding `chat.done` event will fire with
/// `finish: 'cancelled'` shortly after if the stream was live.
export function cancelChatStream(payload: ChatCancelPayload): Promise<void> {
  return invokeIpc<ChatCancelPayload, void>('chat_cancel', payload);
}

export type ChatStreamHandlers = {
  /** Fires for each `chat.token` event with matching id. */
  onToken: (event: ChatTokenEvent) => void;
  /** Fires for the single terminal `chat.done` event. */
  onDone: (event: ChatDoneEvent) => void;
};

/// Subscribe to a stream's events, filtering by id. Returns an
/// unsubscribe function that detaches both listeners; callers
/// should call it after `onDone` fires or when the component
/// unmounts.
///
/// Note: subscribing AFTER the backend has already emitted token
/// events for this id is unsafe — Tauri events are not replayed.
/// `startChatStream` is structured so the backend returns its
/// promise BEFORE emitting any tokens; call this before awaiting
/// `startChatStream` to be sure of catching the first frame, or
/// subscribe before sending.
export async function subscribeChatStream(
  streamId: ChatStreamId,
  handlers: ChatStreamHandlers,
): Promise<UnlistenFn> {
  const unlistenToken = await listen<ChatTokenEvent>('chat.token', (e) => {
    if (e.payload.id === streamId) handlers.onToken(e.payload);
  });
  const unlistenDone = await listen<ChatDoneEvent>('chat.done', (e) => {
    if (e.payload.id === streamId) handlers.onDone(e.payload);
  });
  return () => {
    unlistenToken();
    unlistenDone();
  };
}
