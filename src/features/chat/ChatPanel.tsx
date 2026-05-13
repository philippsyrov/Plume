// D7.1 + D8: streaming read-only chat surface for the agent workspace.
//
// "Read-only" still means: this surface does not touch disk for
// display, does not run commands, does not apply patches. It is a
// streamed text round-trip to the selected local model.
//
// D8 adds an explicit "Attach current file" control. The file is
// read on the backend through the Rust-private prompt-read path —
// no raw bytes ever cross IPC into the frontend. The visible chip
// on the chat panel is the source of truth for "what got sent";
// clearing it removes the context from the next send.
//
// Today's behavior:
//   * One provider (Ollama). If the selected model is from another
//     provider the input is disabled with a clear explanation.
//   * Streaming. Tokens appear as the model produces them; the
//     panel scrolls to the bottom on each delta.
//   * Visible Stop button while streaming; clicking it cancels the
//     active stream cooperatively. The partial reply stays in the
//     transcript with a "(stopped)" marker so the user can see what
//     came back before they hit Stop.
//   * Optional attached file. The chip shows the project-relative
//     path and a × clear control. Disabled when no eligible file is
//     selected in the inspector — binary / oversize / blocked / not-
//     a-file selections cannot attach.
//   * Window-local transcript. Closing the project drops it.
//
// The component takes the currently selected model + the file
// inspector's selection state as props. AgentWorkspace owns the
// wiring; ChatPanel never reaches into the navigator.

import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type FormEvent,
  type KeyboardEvent,
} from 'react';

import { type ChatEntry, useChat } from './useChat';
import type { ChatAttachment } from '../../lib/api/chat';
import { PROMPT_READ_MAX_BYTES } from '../../lib/api/chat';
import type { SelectionState } from '../file-tree/FileBrowser';
import type { SelectedModel } from '../model-picker/useSelectedModel';

const SUPPORTED_PROVIDER_ID = 'ollama';

export type ChatPanelProps = {
  selected: SelectedModel | null;
  /** Current file inspector selection; null when the navigator hook
   * isn't mounted (test scaffolds, the future agent-only view). */
  inspectorSelection: SelectionState | null;
};

/// One-shot attached file the next send will include. Cleared
/// automatically after a successful send so a follow-up turn
/// doesn't silently reattach the same file the user already saw
/// the model react to — the contract is "one attachment per
/// instruction", not "sticky context."
type ChipState = {
  relPath: string;
  /** Bytes on disk at the moment of attach. Surface-only — the
   * backend re-reads on send so the live count can differ. */
  bytes: number;
};

export function ChatPanel({ selected, inspectorSelection }: ChatPanelProps) {
  const { entries, status, activeStreamId, send, cancel, clear } = useChat();
  const [draft, setDraft] = useState('');
  const [chip, setChip] = useState<ChipState | null>(null);
  const listRef = useRef<HTMLOListElement | null>(null);

  // Auto-scroll the transcript to the bottom on new content (token
  // arrivals as well as new turns). Skip if the user has scrolled
  // away — that would steal their reading position. The "near
  // bottom" threshold is intentionally loose so a small upward
  // scroll doesn't fight the streaming append.
  useEffect(() => {
    const el = listRef.current;
    if (!el) return;
    const nearBottom = el.scrollHeight - el.scrollTop - el.clientHeight < 48;
    if (nearBottom) el.scrollTop = el.scrollHeight;
  }, [entries]);

  // When the project is closed or the inspector cleared, drop the
  // chip too. Keeping a stale chip across navigator resets would
  // attach a path that's no longer relevant.
  useEffect(() => {
    if (chip !== null && inspectorSelection?.kind === 'empty') {
      // The empty state happens when the navigator changes project
      // root. The chip's relPath was rooted to the previous project
      // — clear it.
      setChip(null);
    }
  }, [chip, inspectorSelection]);

  const attachCandidate = useMemo(
    () => describeAttachCandidate(inspectorSelection, chip),
    [inspectorSelection, chip],
  );
  const disabledReason = computeDisabledReason(selected, status);
  const isStreaming = status === 'streaming';
  const canSend = disabledReason === null && draft.trim().length > 0 && !isStreaming;

  const onAttach = useCallback(() => {
    if (attachCandidate.kind !== 'eligible') return;
    setChip({
      relPath: attachCandidate.relPath,
      bytes: attachCandidate.bytes,
    });
  }, [attachCandidate]);

  const onClearChip = useCallback(() => setChip(null), []);

  const submit = useCallback(
    (e?: FormEvent) => {
      if (e) e.preventDefault();
      if (!canSend || !selected) return;
      const text = draft;
      const attachment: ChatAttachment | undefined = chip
        ? { kind: 'projectFile', relPath: chip.relPath }
        : undefined;
      setDraft('');
      // The chip is one-shot per send. Clearing it BEFORE awaiting
      // mirrors how the textarea clears — the user sees a clean
      // slate immediately and can attach a different file mid-
      // stream if they want. If `send` returns false (busy / empty
      // input) we don't restore the chip; the user can re-attach.
      setChip(null);
      void send(selected.providerId, selected.modelId, text, attachment ? { attachment } : {});
    },
    [canSend, chip, draft, selected, send],
  );

  // Enter sends; Shift+Enter inserts a newline (the textarea handles
  // that natively, we just don't intercept it).
  const onKeyDown = useCallback(
    (e: KeyboardEvent<HTMLTextAreaElement>) => {
      if (e.key !== 'Enter') return;
      if (e.shiftKey) return;
      e.preventDefault();
      submit();
    },
    [submit],
  );

  const transcriptId = 'plume-chat-transcript';

  return (
    <section
      className="plume-chat ink-panel"
      aria-label="Chat with selected model"
      aria-describedby="plume-chat-subtitle"
    >
      <header className="plume-chat-header">
        <div className="plume-chat-title">
          <h3>Chat</h3>
          <span className="ink-badge plume-chat-readonly-badge">read-only</span>
        </div>
        {entries.length > 0 ? (
          <button
            type="button"
            className="ink-button plume-chat-clear"
            onClick={clear}
            disabled={isStreaming}
            aria-label="Clear chat transcript"
          >
            Clear
          </button>
        ) : null}
      </header>
      <p id="plume-chat-subtitle" className="plume-chat-subtitle">
        Plume streams tokens from the selected model. Optionally attach
        one project file as read-only context — Plume redacts known secret
        patterns before sending. No file writes, no command execution, no
        patches. The transcript lives in this window only.
      </p>

      <ol
        id={transcriptId}
        className="plume-chat-transcript"
        ref={listRef}
        aria-live="polite"
        aria-relevant="additions text"
      >
        {entries.length === 0 ? (
          <li className="plume-chat-empty" role="status">
            No messages yet. Type below to start a streaming read-only chat.
          </li>
        ) : (
          entries.map((entry, i) => <ChatEntryRow key={i} entry={entry} />)
        )}
      </ol>

      <form className="plume-chat-form" onSubmit={submit} aria-controls={transcriptId}>
        <AttachBar
          chip={chip}
          candidate={attachCandidate}
          onAttach={onAttach}
          onClear={onClearChip}
          disabled={isStreaming}
        />
        <label className="plume-chat-input-label">
          <span className="plume-visually-hidden">Message to send</span>
          <textarea
            className="plume-chat-input"
            value={draft}
            onChange={(e) => setDraft(e.target.value)}
            onKeyDown={onKeyDown}
            placeholder={inputPlaceholder(selected, disabledReason)}
            disabled={disabledReason !== null}
            aria-label="Message to send"
            rows={3}
          />
        </label>
        <div className="plume-chat-form-bar">
          <span className="plume-chat-status" role="status">
            {chatStatusText(selected, disabledReason, isStreaming)}
          </span>
          {isStreaming && activeStreamId !== null ? (
            <button
              type="button"
              className="ink-button plume-chat-stop"
              onClick={() => void cancel()}
              aria-label="Stop streaming reply"
            >
              Stop
            </button>
          ) : (
            <button
              type="submit"
              className="ink-button plume-chat-send"
              disabled={!canSend}
              aria-label="Send message"
            >
              Send
            </button>
          )}
        </div>
      </form>
    </section>
  );
}

type AttachCandidate =
  | {
      kind: 'eligible';
      relPath: string;
      bytes: number;
    }
  | {
      kind: 'ineligible';
      /** One-line reason rendered in the disabled button's title. */
      reason: string;
    }
  | { kind: 'already-attached'; relPath: string }
  | { kind: 'none' };

function describeAttachCandidate(
  selection: SelectionState | null,
  chip: ChipState | null,
): AttachCandidate {
  if (selection === null || selection.kind === 'empty') {
    return { kind: 'none' };
  }
  if (selection.kind === 'loading') {
    return {
      kind: 'ineligible',
      reason: 'File is still loading in the inspector.',
    };
  }
  if (selection.kind === 'error') {
    return {
      kind: 'ineligible',
      reason: `Inspector failed to load: ${selection.message}`,
    };
  }
  // selection.kind === 'ready'
  if (chip !== null && chip.relPath === selection.path) {
    return { kind: 'already-attached', relPath: chip.relPath };
  }
  if (selection.content.encoding !== 'utf-8') {
    return {
      kind: 'ineligible',
      reason: 'Binary files cannot be attached as text context.',
    };
  }
  if (selection.content.bytes > PROMPT_READ_MAX_BYTES) {
    return {
      kind: 'ineligible',
      reason: `File is ${formatBytes(selection.content.bytes)}; prompt attachments are capped at ${formatBytes(
        PROMPT_READ_MAX_BYTES,
      )}.`,
    };
  }
  return {
    kind: 'eligible',
    relPath: selection.path,
    bytes: selection.content.bytes,
  };
}

type AttachBarProps = {
  chip: ChipState | null;
  candidate: AttachCandidate;
  onAttach: () => void;
  onClear: () => void;
  disabled: boolean;
};

function AttachBar({ chip, candidate, onAttach, onClear, disabled }: AttachBarProps) {
  const attachLabel = chip ? 'Replace attachment' : 'Attach current file';
  const attachDisabled = disabled || candidate.kind !== 'eligible';
  const attachTitle = attachButtonTitle(candidate, disabled);
  return (
    <div className="plume-chat-attach" aria-label="Read-only file context">
      <button
        type="button"
        className="ink-button plume-chat-attach-button"
        onClick={onAttach}
        disabled={attachDisabled}
        aria-label={attachLabel}
        title={attachTitle}
      >
        {attachLabel}
      </button>
      {chip ? (
        <span
          className="ink-badge plume-chat-attach-chip"
          role="status"
          aria-label={`Attached file: ${chip.relPath}`}
        >
          <span className="plume-chat-attach-chip-icon" aria-hidden>
            ¶
          </span>
          <span className="plume-chat-attach-chip-path" title={chip.relPath}>
            {chip.relPath}
          </span>
          <span className="plume-chat-attach-chip-meta">
            · {formatBytes(chip.bytes)}
          </span>
          <button
            type="button"
            className="plume-chat-attach-chip-clear"
            onClick={onClear}
            disabled={disabled}
            aria-label={`Remove attached file ${chip.relPath}`}
            title="Remove attached file"
          >
            ×
          </button>
        </span>
      ) : (
        <span className="plume-chat-attach-hint" role="status">
          {attachHintText(candidate)}
        </span>
      )}
    </div>
  );
}

function attachButtonTitle(candidate: AttachCandidate, disabledByStream: boolean): string {
  if (disabledByStream) return 'Cannot change attachment while streaming.';
  switch (candidate.kind) {
    case 'eligible':
      return `Attach ${candidate.relPath} (${formatBytes(candidate.bytes)}) to your next message.`;
    case 'ineligible':
      return candidate.reason;
    case 'already-attached':
      return `${candidate.relPath} is already attached.`;
    case 'none':
      return 'Select a UTF-8 text file in the inspector to enable.';
  }
}

function attachHintText(candidate: AttachCandidate): string {
  switch (candidate.kind) {
    case 'eligible':
      return `Inspector has ${candidate.relPath} ready to attach.`;
    case 'ineligible':
      return candidate.reason;
    case 'already-attached':
      return `Attached: ${candidate.relPath}.`;
    case 'none':
      return 'Select a file in the inspector to attach it as context.';
  }
}

function ChatEntryRow({ entry }: { entry: ChatEntry }) {
  if (entry.kind === 'error') {
    return (
      <li className="plume-chat-entry plume-chat-entry-error" role="alert">
        <span className="plume-chat-entry-role">error</span>
        <p className="plume-chat-entry-content">{entry.message}</p>
      </li>
    );
  }
  if (entry.kind === 'streaming') {
    return (
      <li
        className="plume-chat-entry plume-chat-entry-assistant plume-chat-entry-streaming"
        aria-label="streaming assistant message"
      >
        <span className="plume-chat-entry-role">assistant</span>
        <p className="plume-chat-entry-content">
          {entry.content}
          <span className="plume-chat-cursor" aria-hidden>
            ▍
          </span>
        </p>
        <p className="plume-chat-entry-meta">streaming…</p>
      </li>
    );
  }
  if (entry.kind === 'cancelled') {
    return (
      <li
        className="plume-chat-entry plume-chat-entry-assistant plume-chat-entry-cancelled"
        aria-label="cancelled assistant message"
      >
        <span className="plume-chat-entry-role">assistant</span>
        <p className="plume-chat-entry-content">{entry.partial || '(no tokens received)'}</p>
        <p className="plume-chat-entry-meta">
          <span>stopped by you</span>
          {entry.modelUsed ? <span>· {entry.modelUsed}</span> : null}
          {typeof entry.durationMs === 'number' ? (
            <span>· {formatDuration(entry.durationMs)}</span>
          ) : null}
        </p>
      </li>
    );
  }
  const { message, modelUsed, durationMs, attachmentRelPath, stats } = entry;
  const isAssistant = message.role === 'assistant';
  // D9: the stats footer is only shown when there's at least one
  // useful number to display. `formatStatsLine` returns null when
  // both `outputTokens` and `tokensPerSecond` are absent — the
  // duration alone is already in the model/duration row above.
  const statsLine = isAssistant && stats ? formatStatsLine(stats) : null;
  const statsTitle = isAssistant && stats ? formatStatsTitle(stats) : undefined;
  return (
    <li
      className={`plume-chat-entry plume-chat-entry-${message.role}`}
      aria-label={`${message.role} message`}
    >
      <span className="plume-chat-entry-role">{message.role}</span>
      {attachmentRelPath ? (
        <span
          className="ink-badge plume-chat-entry-attachment"
          aria-label={`Attached: ${attachmentRelPath}`}
          title={`Attached as read-only context: ${attachmentRelPath}`}
        >
          ¶ {attachmentRelPath}
        </span>
      ) : null}
      <p className="plume-chat-entry-content">{message.content}</p>
      {isAssistant && (modelUsed || typeof durationMs === 'number') ? (
        <p className="plume-chat-entry-meta">
          {modelUsed ? <span>served by {modelUsed}</span> : null}
          {typeof durationMs === 'number' ? <span>· {formatDuration(durationMs)}</span> : null}
        </p>
      ) : null}
      {statsLine !== null ? (
        <p
          className="plume-chat-entry-meta plume-chat-entry-stats"
          title={statsTitle}
        >
          {statsLine}
        </p>
      ) : null}
    </li>
  );
}

/// Render the one-line stats footer. Returns `null` when the stats
/// object has no information worth displaying — that suppresses the
/// `<p>` entirely so a `chat.done` with all-null stats doesn't add
/// noise to the transcript.
///
/// Format keeps the "feel" of a status strip: short numbers, dots
/// between, no labels on the numbers themselves (the title attribute
/// carries the full prompt-eval breakdown for the curious).
function formatStatsLine(stats: import('../../lib/api/chat').ChatStats): string | null {
  const parts: string[] = [];
  if (typeof stats.outputTokens === 'number') {
    parts.push(`${stats.outputTokens} ${stats.outputTokens === 1 ? 'token' : 'tokens'}`);
  }
  if (typeof stats.tokensPerSecond === 'number') {
    parts.push(`${stats.tokensPerSecond.toFixed(1)} tok/s`);
  }
  if (parts.length === 0) return null;
  return parts.join(' · ');
}

/// Title attribute for the stats footer — pulled out so the
/// hover-state surface stays auditable in one place. Includes the
/// prompt-eval breakdown that doesn't fit on the visible line.
function formatStatsTitle(stats: import('../../lib/api/chat').ChatStats): string | undefined {
  const lines: string[] = [];
  if (typeof stats.outputTokens === 'number' && typeof stats.evalMs === 'number') {
    lines.push(`Output: ${stats.outputTokens} tokens in ${formatDuration(stats.evalMs)}`);
  }
  if (typeof stats.promptTokens === 'number' && typeof stats.promptMs === 'number') {
    lines.push(`Prompt: ${stats.promptTokens} tokens in ${formatDuration(stats.promptMs)}`);
  }
  return lines.length === 0 ? undefined : lines.join('\n');
}

type DisabledReason = 'no-selection' | 'unsupported-provider' | 'streaming' | null;

function computeDisabledReason(
  selected: SelectedModel | null,
  status: 'idle' | 'streaming' | 'error',
): DisabledReason {
  if (status === 'streaming') return 'streaming';
  if (selected === null) return 'no-selection';
  if (selected.providerId !== SUPPORTED_PROVIDER_ID) return 'unsupported-provider';
  return null;
}

function inputPlaceholder(
  selected: SelectedModel | null,
  disabledReason: DisabledReason,
): string {
  switch (disabledReason) {
    case 'no-selection':
      return 'Pick a model on the left to enable chat.';
    case 'unsupported-provider':
      return `Chat is only wired for Ollama today (selected: ${selected?.providerDisplayName ?? 'unknown'}).`;
    case 'streaming':
      return 'Streaming reply… click Stop to cancel.';
    case null:
      return `Send a message to ${selected?.modelId ?? 'the model'}…`;
  }
}

function chatStatusText(
  selected: SelectedModel | null,
  disabledReason: DisabledReason,
  isStreaming: boolean,
): string {
  if (isStreaming) return 'Streaming reply…';
  switch (disabledReason) {
    case 'no-selection':
      return 'No model selected.';
    case 'unsupported-provider':
      return 'Selected provider has no chat adapter yet (Ollama only).';
    case 'streaming':
      return 'Streaming reply…';
    case null:
      return selected
        ? `Ready · ${selected.providerDisplayName} · ${selected.modelId}`
        : 'Ready.';
  }
}

function formatDuration(ms: number): string {
  if (ms < 1000) return `${ms} ms`;
  const seconds = ms / 1000;
  if (seconds < 60) return `${seconds.toFixed(1)} s`;
  const minutes = Math.floor(seconds / 60);
  const remaining = Math.round(seconds % 60);
  return `${minutes} m ${remaining} s`;
}

function formatBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  return `${(n / (1024 * 1024)).toFixed(1)} MB`;
}
