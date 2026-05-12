// Typed wrapper for the D7 `chat.send` IPC verb.
//
// D7 ships the sync, non-streaming subset of the chat surface
// sketched in `docs/IPC_CONTRACT.md § chat`. The streaming variant
// (returns a `ChatStreamId`, fires `chat.token` events, paired
// with `chat.cancel`) is queued as D7.1.
//
// Today's contract:
//   * Ollama is the only wired provider — backend rejects others
//     with `BadArgument`.
//   * `messages` is the full transcript; Ollama is stateless across
//     `/api/chat` calls so the caller concatenates history.
//   * `chat.send` returns the full assistant message at once; there
//     is no token stream to subscribe to.
//
// Errors surface through the usual `IpcError` taxonomy:
//   * `BadArgument`  — empty model id, empty messages, last message
//                       not from the user, provider has no chat
//                       adapter yet, model id not known to runtime.
//   * `ProviderDown` — could not reach Ollama, or Ollama returned
//                       5xx.
//   * `Internal`     — Ollama answered but its body did not parse.

import { invokeIpc } from './ipc';

export type ChatRole = 'system' | 'user' | 'assistant' | 'tool';

export type ChatMessage = {
  role: ChatRole;
  content: string;
};

export type ChatFinish = 'stop' | 'length';

export type ChatResponse = {
  message: ChatMessage;
  /** Echoes the provider id from the request (currently always `"ollama"`). */
  providerId: string;
  /**
   * The model id the runtime reports it actually served. Can differ
   * subtly from the request id (`llama3` → `llama3:latest` for
   * Ollama). UI should display this value, not the request id.
   */
  modelId: string;
  /** Wall-clock milliseconds the IPC call took, measured on the backend. */
  durationMs: number;
  finish: ChatFinish;
};

type ChatSendPayload = {
  providerId: string;
  modelId: string;
  messages: ChatMessage[];
};

export function sendChat(payload: ChatSendPayload): Promise<ChatResponse> {
  return invokeIpc<ChatSendPayload, ChatResponse>('chat_send', payload);
}
