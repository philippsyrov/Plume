// Typed wrapper for the streaming `chat.send` + `chat.cancel` IPC
// verbs and the matching `chat/token` / `chat/done` events.
//
// D7 shipped chat as a single synchronous IPC call returning the
// full assistant message. D7.1 reshapes that path: the frontend
// mints a `ChatStreamId`, subscribes to events filtered by it, and
// only then calls `chat.send`. The backend validates the id (non-
// empty, length-capped, not already in flight), spawns the
// streaming task, and returns the same id back. The assistant
// reply arrives over Tauri events. `chat.cancel(streamId)` flips a
// cooperative cancel flag on the backend.
//
// D8 layers an optional `attachment` field onto the payload. When
// present, the backend uses its Rust-private prompt-read path
// (`prompts::assemble`) to fold the file content + secret-redactor
// output into the LAST user message before the stream starts. The
// frontend never receives raw file bytes — `fs.read` is the
// display surface; this attachment ref is the only thing the model
// path ever sees from disk. See `docs/IPC_CONTRACT.md § chat`.
//
// Why the client mints the id: Tauri events are not replayed. If
// the backend minted the id and spawned the task before the IPC
// return reached the frontend, a fast local Ollama could emit
// `chat/token` events before the frontend's listeners exist, and
// those tokens would be silently lost. Letting the frontend pick
// the id closes the race — listeners are registered before the
// backend can possibly emit.
//
// Contract today:
//   * Ollama is the only wired provider — backend rejects others
//     with `BadArgument`.
//   * `messages` is the full transcript; Ollama is stateless across
//     `/api/chat` calls so the caller concatenates history.
//   * Exactly one `chat/done` event fires per stream id, after
//     which the id becomes invalid; further `cancelChat(id)` is a
//     silent no-op.
//   * Events carry a monotonic `seq` per stream id; frontend
//     enforces order, drops duplicates, and treats gaps as
//     corruption per `docs/IPC_CONTRACT.md § Event sequencing`.
//   * `attachment` is optional and one-shot per send. When set,
//     the backend requires a trusted open project and reads via
//     the prompt-read path; secret-pattern filenames, oversize
//     files, binary content, and path escapes reject synchronously
//     before a stream id is registered.
//
// See `docs/IPC_CONTRACT.md § chat` for the full shape.

import { listen, type UnlistenFn } from '@tauri-apps/api/event';

import { invokeIpc } from './ipc';

const CHAT_TOKEN_EVENT = 'chat/token';
const CHAT_DONE_EVENT = 'chat/done';

/// Backend's hard cap on a single prompt-read attachment, in bytes.
/// Mirrors `PROMPT_READ_MAX_BYTES` in `src-tauri/src/prompts/read.rs`.
/// Frontend uses this to disable the Attach button before a doomed
/// IPC round-trip; the backend re-checks regardless and is the source
/// of truth.
export const PROMPT_READ_MAX_BYTES = 256 * 1024;

export type ChatRole = 'system' | 'user' | 'assistant' | 'tool';

export type ChatMessage = {
  role: ChatRole;
  content: string;
};

export type ChatFinish = 'stop' | 'length' | 'cancelled' | 'error';

export type ChatStreamId = string;

export type ChatSendStartedResponse = {
  /**
   * Opaque stream id. Subscribers should filter `chat/token` /
   * `chat/done` events by matching `payload.id`.
   */
  streamId: ChatStreamId;
  /** Echoed for routing convenience. */
  providerId: string;
  /** Echoed for routing convenience. */
  modelId: string;
  /**
   * D11: `true` when the project's root `AGENTS.md` was
   * successfully read and folded in as a system message for this
   * send. The frontend's "Project instructions included"
   * indicator reflects this. `false` covers every honest skip —
   * no trusted project open, `AGENTS.md` missing / oversize /
   * binary / unreadable.
   */
  instructionsIncluded: boolean;
  /**
   * D42: summary of the project-memory injection on this send.
   * `null` covers every honest skip — no trusted project, no
   * memory store, store unreadable, or no entries. When `Some`,
   * the chat panel renders a "Memory · N entries · K bytes" badge.
   */
  memory: ChatMemoryUsage | null;
  /**
   * D72: summary of the curated topic-file injection (INDEX/USER/
   * SOUL) on this send. `null` on the same honest skips as `memory`.
   * A UI badge for this is a reserved follow-up; the data rides along
   * now so the contract is complete.
   */
  topics: ChatTopicsUsage | null;
  /** Exact ordered explicit sources that reached this prompt. */
  contextSources: ContextSourceManifestItem[];
};

export type ContextSourceRef =
  | {
      kind: 'projectFile';
      relPath: string;
      startLine?: number;
      endLine?: number;
    }
  | { kind: 'memoryEntry'; entryId: string }
  | { kind: 'topicFile'; name: string }
  | { kind: 'browserTextEvidence'; evidenceId: string }
  | { kind: 'browserScreenshotEvidence'; evidenceId: string };

export type ContextSourceManifestItem =
  | {
      kind: 'projectFile';
      relPath: string;
      startLine: number | null;
      endLine: number | null;
      bytes: number;
      originalBytes: number;
      redactionCount: number;
    }
  | {
      kind: 'memoryEntry';
      entryId: string;
      createdAtMs: number;
      bytes: number;
      preview: string;
    }
  | { kind: 'topicFile'; name: string; bytes: number }
  | {
      kind: 'browserTextEvidence';
      evidenceId: string;
      captureKind: 'selection' | 'page';
      sourceUrl: string;
      title: string | null;
      capturedAtMs: number;
      bytes: number;
      redactionCount: number;
      truncated: boolean;
      preview: string;
    }
  | {
      kind: 'browserScreenshotEvidence';
      evidenceId: string;
      sourceUrl: string;
      title: string | null;
      capturedAtMs: number;
      width: number;
      height: number;
      bytes: number;
      sha256: string;
    };

/**
 * D42: shape of the memory summary echoed by both `chat.send`
 * (post-fold) and `chat.context` (preflight). One wire shape so
 * the frontend can reuse one renderer for both call sites.
 */
export type ChatMemoryUsage = {
  /** Number of memory entries folded into the system block. */
  entryCount: number;
  /** Bytes of memory content folded into the system block — sum
   * of `entry.text.length` over the picked entries. */
  bytes: number;
  /** Hard cap in bytes applied to `bytes`. The backend constant
   * is currently 4096 (4 KiB); echoed here so the UI can show
   * "K / 4096" without hard-coding the number on its side. */
  byteCap: number;
  /** `true` when at least one stored entry was dropped to stay
   * within `byteCap`. The dropped entries are the oldest ones —
   * newest are picked first. */
  truncated: boolean;
  /** Exact newest-first entries that were folded into this prompt. */
  entries: ChatMemoryContextEntry[];
};

export type ChatMemoryContextEntry = {
  id: string;
  createdAtMs: number;
  /** UTF-8 bytes in the full redacted entry text. */
  textBytes: number;
  /** Single-line, Unicode-safe preview capped at 120 characters. */
  preview: string;
};

/**
 * D72: shape of the curated topic-file summary echoed by both
 * `chat.send` and `chat.context`. Counts only — no content.
 */
export type ChatTopicsUsage = {
  /** How many of the core trio (INDEX/USER/SOUL) were folded in. */
  fileCount: number;
  /** Bytes of topic-file content folded into the system block. */
  bytes: number;
  /** Hard cap in bytes applied to `bytes` (backend constant, 6 KiB). */
  byteCap: number;
  /** `true` when a core file was skipped to fit the budget or trimmed
   * at its per-file cap. */
  truncated: boolean;
  /** Exact core files folded in, in prompt order. */
  files: ChatTopicContextFile[];
};

export type ChatTopicContextFile = {
  name: string;
  /** UTF-8 bytes of this file's content that reached the prompt. */
  bytes: number;
};

export type ChatTokenEvent = {
  id: ChatStreamId;
  /** Monotonic per stream, starting at 0. */
  seq: number;
  /** New text since the previous frame. Caller concatenates. */
  delta: string;
};

/// D9 generation telemetry. Every field is independently optional
/// because the underlying runtime may report only a subset; today
/// Ollama populates all four when `finish === 'stop'`. The frontend
/// hides the stats footer entirely when nothing useful is present.
///
/// `tokensPerSecond` is pre-computed on the backend so a future
/// adapter that needs a different formula (e.g. wall-clock vs.
/// eval-only) can centralise the choice without the UI duplicating
/// it. Backend yields `null` when the value cannot be measured
/// (e.g. eval_duration == 0).
export type ChatStats = {
  outputTokens: number | null;
  evalMs: number | null;
  tokensPerSecond: number | null;
  promptTokens: number | null;
  promptMs: number | null;
};

export type ChatDoneEvent = {
  id: ChatStreamId;
  /** Equals the count of `chat/token` events the stream emitted. */
  seq: number;
  finish: ChatFinish;
  /** `null` if the stream errored before reading any frame. */
  modelId: string | null;
  /** Backend-measured wall-clock duration in ms. */
  durationMs: number;
  /** Present iff `finish === 'error'`. */
  error: string | null;
  /** Generation telemetry from the runtime's final frame.
   * Populated only on `finish === 'stop'`; `null` for cancelled,
   * length-truncated, and error finishes. */
  stats: ChatStats | null;
};

/// Optional read-only attachment folded into the last user message.
///
/// D8 shipped `projectFile` with just `relPath`; D10 added the
/// optional `startLine` / `endLine` pair. When both are present
/// the backend slices the redacted content to that 1-based
/// inclusive range before folding. Half a range (one without the
/// other) is rejected with `BadArgument`; pass either both or
/// neither.
///
/// Tagged so future kinds (recent terminal output, clipboard
/// snippet, …) extend additively without a breaking contract
/// change. The `relPath` is project-relative; the backend
/// validates it (no `..`, no leading slash, no NUL, ≤ 1024 chars)
/// before reaching disk.
export type ChatAttachment = {
  kind: 'projectFile';
  relPath: string;
  /// 1-based inclusive start of the requested line range.
  /// Omit (alongside `endLine`) for a whole-file attachment.
  startLine?: number;
  /// 1-based inclusive end of the requested line range.
  endLine?: number;
};

/// D15: response-shape mode for `chat.send`. Carried on the
/// payload; the backend prepends a mode-specific system message
/// before AGENTS.md. Defaults to `'chat'` (the D7.1 path) when
/// omitted. New modes are additive; the backend rejects unknown
/// variants with `BadArgument` at the serde layer.
///
/// `'proposeDiff'` instructs the model to respond with a unified
/// diff inside a single fenced code block. The chat panel
/// renders the diff with per-line coloring and a *disabled*
/// Apply button — Plume does NOT apply patches in D15.
export type ChatMode = 'chat' | 'proposeDiff';

export type ChatContextOwner = {
  scope: 'local' | 'project';
  sessionId: string;
};

export type ChatSendPayload = {
  /// Client-minted opaque id. Use `mintStreamId()` unless you have
  /// a specific reason to do otherwise. The backend rejects empty,
  /// overlong, or already-in-flight ids with `BadArgument`.
  streamId: ChatStreamId;
  providerId: string;
  modelId: string;
  messages: ChatMessage[];
  /// D45 (optional). MLX-LM server handle id from
  /// `providers.startServer`. REQUIRED when `providerId === 'mlx-lm'`;
  /// ignored for `'ollama'`. Backend rejects mlx-lm sends without
  /// a handleId with `BadArgument` and unknown handle ids with
  /// `NotFound` so the UI can drive a "start the server again"
  /// flow. (D46 frontend reads the handle from `useMlxServers` and
  /// threads it through here.)
  handleId?: string;
  /// Optional. When provided, the backend folds the file content
  /// (read via the Rust-private prompt-read path + secret
  /// redactor) into the last user message before sending to the
  /// model. Omitted = D7.1 text-only path.
  attachment?: ChatAttachment;
  /** Ordered explicit references. Rust resolves content at send time. */
  contextSources?: ContextSourceRef[];
  /** Persisted chat that owns explicit Browser evidence. */
  contextOwner?: ChatContextOwner;
  /// Optional. Defaults to `'chat'` (existing D7.1 path). See
  /// `ChatMode` for the propose-diff response-shape constraint.
  mode?: ChatMode;
  /// Optional. Defaults to `true`. No-project chat passes `false`
  /// so a previously-open trusted project does not leak AGENTS.md,
  /// memory, or attachment eligibility into the plain chat surface.
  includeProjectContext?: boolean;
};

type ChatCancelPayload = {
  streamId: ChatStreamId;
};

/// Mint a fresh stream id. Used by the chat hook before subscribing
/// to events. Prefers `crypto.randomUUID()` when available (modern
/// WebKit / Chromium); falls back to a timestamp + random suffix
/// otherwise so older WebViews still work.
///
/// The ids are opaque on the wire — backend treats them as raw
/// strings — so the exact format does not matter as long as ids
/// are distinct per concurrent stream.
export function mintStreamId(): ChatStreamId {
  if (typeof crypto !== 'undefined' && 'randomUUID' in crypto) {
    return crypto.randomUUID();
  }
  return `${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 10)}`;
}

/// Start a streaming chat. Resolves with the same stream id once
/// the backend has accepted the call and started the streaming
/// task — not when the assistant reply finishes. The caller is
/// expected to have subscribed to `chat/token` / `chat/done`
/// events for this id BEFORE invoking `startChatStream` (so a fast
/// local model can't beat the listener registration).
export function startChatStream(payload: ChatSendPayload): Promise<ChatSendStartedResponse> {
  return invokeIpc<ChatSendPayload, ChatSendStartedResponse>('chat_send', payload);
}

/// Cooperatively cancel an in-flight stream. Idempotent: cancelling
/// a finished or unknown stream resolves successfully. The
/// corresponding `chat/done` event will fire with
/// `finish: 'cancelled'` shortly after if the stream was live.
export function cancelChatStream(payload: ChatCancelPayload): Promise<void> {
  return invokeIpc<ChatCancelPayload, void>('chat_cancel', payload);
}

/// D12: read-only preflight for `chat.send`. Mirrors what the
/// backend's prompt assembly would surface (AGENTS.md probe +
/// attachment resolution + line-range validation) without
/// invoking a model or registering a stream id. Used by the
/// chat panel's context-preview area so the user can see "what
/// will ride along on my next send" before typing the prompt.
///
/// Surface rule: attachment rejections come back IN-BAND as
/// `attachment.status === 'blocked'`. The IPC `Promise` only
/// rejects for payload-shape problems (the same `BadArgument`
/// the actual send raises for a malformed relPath) or for
/// out-of-contract conditions (version mismatch). Trust gating
/// and prompt-read policy rejections surface as a structured
/// `blocked` outcome with a typed `reason` code.
export type ChatContextRequest = {
  /** Exact selected model identity, used only for screenshot capability preflight. */
  providerId?: string;
  modelId?: string;
  /// Mirrors `ChatSendPayload.attachment`. Omit for a "just
  /// preview the project instructions" call.
  attachment?: ChatAttachment;
  contextSources?: ContextSourceRef[];
  contextOwner?: ChatContextOwner;
  /// Mirrors `ChatSendPayload.includeProjectContext`. Defaults to
  /// true for the trusted-project chat surface; no-project chat
  /// passes false so preview stays genuinely project-free.
  includeProjectContext?: boolean;
};

/// Stable codes for the `blocked` reasons. The frontend switches
/// on this code; the human-readable `message` is rendered as the
/// tooltip / hint text. New variants are additive — a future
/// reason the frontend doesn't recognise should be treated as a
/// generic "blocked" with the human message as the diagnostic.
export type ChatContextBlockReason =
  | 'notFound'
  | 'pathEscape'
  | 'blocked'
  | 'badArgument'
  | 'needsApproval'
  | 'internal';

export type ChatContextInstructionsPreview = {
  /** Filename relative to the project root; "AGENTS.md" today. */
  source: string;
  /** Bytes on disk before the redactor ran. */
  originalBytes: number;
  /** Number of secret-pattern matches the redactor masked. */
  redactionCount: number;
};

export type ChatContextAttachmentReady = {
  status: 'ready';
  relPath: string;
  /** `null` means whole file; both fields are either set together or both null. */
  startLine: number | null;
  endLine: number | null;
  originalBytes: number;
  redactionCount: number;
};

export type ChatContextAttachmentBlocked = {
  status: 'blocked';
  relPath: string;
  reason: ChatContextBlockReason;
  /** Short human-readable text echoed from the typed error
   * `chat.send` would have raised. Renders in the tooltip; never
   * parsed. */
  message: string;
};

export type ChatContextAttachmentPreview =
  | ChatContextAttachmentReady
  | ChatContextAttachmentBlocked;

export type ChatContextResponse = {
  /** Forward-looking AGENTS.md preview. `null` covers every honest
   * skip (no trusted project, no AGENTS.md, AGENTS.md unreadable). */
  instructions: ChatContextInstructionsPreview | null;
  /** Per-attachment preview. `null` when the request omitted
   * `attachment` entirely; otherwise either `ready` or `blocked`. */
  attachment: ChatContextAttachmentPreview | null;
  /** D42: forward-looking project-memory preview. `null` covers
   * every honest skip (no trusted project, no memory store, store
   * unreadable, no entries). Shape mirrors `ChatMemoryUsage`. */
  memory: ChatMemoryUsage | null;
  /** D72: forward-looking curated topic-file preview (INDEX/USER/
   * SOUL). `null` on the same honest skips as `memory`. Shape mirrors
   * `ChatTopicsUsage`. */
  topics: ChatTopicsUsage | null;
  contextSources: ContextSourcePreviewItem[];
};

export type ContextSourcePreviewItem =
  | { status: 'ready'; source: ContextSourceManifestItem }
  | {
      status: 'blocked';
      ref: ContextSourceRef;
      reason: ChatContextBlockReason;
      message: string;
    };

/// Fetch the read-only context preview. Returns the same numbers
/// `chat.send` would log on its next successful accept (no model
/// call, no stream id). The frontend treats the result as
/// disposable — call it again whenever the chip or AGENTS.md
/// state changes.
export function previewChatContext(
  payload: ChatContextRequest,
): Promise<ChatContextResponse> {
  return invokeIpc<ChatContextRequest, ChatContextResponse>('chat_context', payload);
}

export type ChatStreamHandlers = {
  /** Fires for each `chat/token` event with matching id. */
  onToken: (event: ChatTokenEvent) => void;
  /** Fires for the single terminal `chat/done` event. */
  onDone: (event: ChatDoneEvent) => void;
};

/// Subscribe to a stream's events, filtering by id. Returns an
/// unsubscribe function that detaches both listeners; callers
/// should call it after `onDone` fires or when the component
/// unmounts.
///
/// **Important.** Subscribing AFTER the backend has emitted token
/// events for this id is unsafe — Tauri events are not replayed.
/// The correct order is:
///
///   1. `streamId = mintStreamId()`
///   2. `unlisten = await subscribeChatStream(streamId, ...)`
///   3. `await startChatStream({ streamId, ... })`
///
/// Reversing 2 and 3 reintroduces the race that motivated the
/// client-minted id in the first place.
export async function subscribeChatStream(
  streamId: ChatStreamId,
  handlers: ChatStreamHandlers,
): Promise<UnlistenFn> {
  const unlistenToken = await listen<ChatTokenEvent>(CHAT_TOKEN_EVENT, (e) => {
    if (e.payload.id === streamId) handlers.onToken(e.payload);
  });
  const unlistenDone = await listen<ChatDoneEvent>(CHAT_DONE_EVENT, (e) => {
    if (e.payload.id === streamId) handlers.onDone(e.payload);
  });
  return () => {
    unlistenToken();
    unlistenDone();
  };
}
