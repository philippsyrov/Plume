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
import { useChatContextPreview } from './useChatContextPreview';
import type {
  ChatAttachment,
  ChatContextAttachmentPreview,
  ChatContextBlockReason,
  ChatContextInstructionsPreview,
} from '../../lib/api/chat';
import { PROMPT_READ_MAX_BYTES } from '../../lib/api/chat';
import type { EditorLineRange } from '../editor/ReadOnlyEditor';
import type { SelectionState } from '../file-tree/FileBrowser';
import type { SelectedModel } from '../model-picker/useSelectedModel';

const SUPPORTED_PROVIDER_ID = 'ollama';

export type ChatPanelProps = {
  selected: SelectedModel | null;
  /** Current file inspector selection; null when the navigator hook
   * isn't mounted (test scaffolds, the future agent-only view). */
  inspectorSelection: SelectionState | null;
  /** D10: current non-empty text selection inside the inspector's
   * editor as 1-based line numbers, or `null` when the user has
   * only a point cursor / no file open. Lets the chat panel
   * flip the attach button between "Attach selection (lines X-Y)"
   * and the D8 "Attach current file" default. */
  inspectorLineRange: EditorLineRange | null;
  /** D11: `true` when the trusted project has a root `AGENTS.md`
   * the backend will fold in as a system message on every send.
   * The chat panel renders a small "Project instructions"
   * indicator when this is set. False (or absent project)
   * suppresses the indicator. */
  projectHasInstructions: boolean;
};

/// One-shot attached file the next send will include. Cleared
/// automatically after a successful send so a follow-up turn
/// doesn't silently reattach the same file the user already saw
/// the model react to — the contract is "one attachment per
/// instruction", not "sticky context."
///
/// `lineRange` carries the optional D10 narrowing — when set, the
/// send includes `startLine` / `endLine` and the backend slices
/// the redacted content. The chip renders `relPath:start–end`
/// instead of the path alone.
type ChipState = {
  relPath: string;
  /** Bytes on disk at the moment of attach. Surface-only — the
   * backend re-reads on send so the live count can differ. */
  bytes: number;
  lineRange: EditorLineRange | null;
};

export function ChatPanel({
  selected,
  inspectorSelection,
  inspectorLineRange,
  projectHasInstructions,
}: ChatPanelProps) {
  const { entries, status, activeStreamId, lastInstructionsIncluded, send, cancel, clear } =
    useChat();
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
    () => describeAttachCandidate(inspectorSelection, inspectorLineRange, chip),
    [inspectorSelection, inspectorLineRange, chip],
  );
  const disabledReason = computeDisabledReason(selected, status);
  const isStreaming = status === 'streaming';
  const canSend = disabledReason === null && draft.trim().length > 0 && !isStreaming;

  // D12: ask the backend what would ride along on the next send.
  // The hook re-fires when the chip changes or the project's
  // AGENTS.md state flips. We pass primitives, not the chip object,
  // so the effect only fires when the relevant fields actually
  // change (object identity would re-fire on every render).
  const contextPreview = useChatContextPreview({
    relPath: chip?.relPath ?? null,
    startLine: chip?.lineRange?.startLine ?? null,
    endLine: chip?.lineRange?.endLine ?? null,
    projectHasInstructions,
  });

  const onAttach = useCallback(() => {
    if (attachCandidate.kind !== 'eligible') return;
    setChip({
      relPath: attachCandidate.relPath,
      bytes: attachCandidate.bytes,
      lineRange: attachCandidate.lineRange,
    });
  }, [attachCandidate]);

  const onClearChip = useCallback(() => setChip(null), []);

  const submit = useCallback(
    (e?: FormEvent) => {
      if (e) e.preventDefault();
      if (!canSend || !selected) return;
      const text = draft;
      const attachment: ChatAttachment | undefined = chip
        ? {
            kind: 'projectFile',
            relPath: chip.relPath,
            // D10: include the line range only when the chip
            // carries one. Half-a-range (just startLine or
            // endLine) is rejected by the backend's validator, so
            // we send either both or neither.
            ...(chip.lineRange
              ? {
                  startLine: chip.lineRange.startLine,
                  endLine: chip.lineRange.endLine,
                }
              : {}),
          }
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
          <InstructionsBadge
            projectHasInstructions={projectHasInstructions}
            lastIncluded={lastInstructionsIncluded}
          />
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
        Plume streams tokens from the selected model.{' '}
        {instructionsSubtitleHint(projectHasInstructions, lastInstructionsIncluded)}
        Optionally attach one project file as read-only context — Plume
        redacts known secret patterns before sending. No file writes, no
        command execution, no patches. The transcript lives in this window
        only.
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
        <ContextPreview
          instructions={contextPreview.data?.instructions ?? null}
          attachment={contextPreview.data?.attachment ?? null}
          loading={contextPreview.status === 'loading' && contextPreview.data === null}
          error={contextPreview.status === 'error' ? contextPreview.error : null}
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
      /** D10: when non-null, the next attach uses the user's
       * current text selection; the chip will carry the range and
       * the send will include startLine / endLine. Null falls back
       * to the D8 whole-file behavior. */
      lineRange: EditorLineRange | null;
    }
  | {
      kind: 'ineligible';
      /** One-line reason rendered in the disabled button's title. */
      reason: string;
    }
  | {
      kind: 'already-attached';
      relPath: string;
      lineRange: EditorLineRange | null;
    }
  | { kind: 'none' };

/// Check whether the chip already reflects the user's current
/// selection, including its line range. Returning `true` makes the
/// attach button disable as "already attached" rather than offering
/// a no-op re-attach.
function chipMatchesSelection(
  chip: ChipState,
  selectionPath: string,
  lineRange: EditorLineRange | null,
): boolean {
  if (chip.relPath !== selectionPath) return false;
  if (chip.lineRange === null && lineRange === null) return true;
  if (chip.lineRange === null || lineRange === null) return false;
  return (
    chip.lineRange.startLine === lineRange.startLine &&
    chip.lineRange.endLine === lineRange.endLine
  );
}

function describeAttachCandidate(
  selection: SelectionState | null,
  lineRange: EditorLineRange | null,
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
  if (selection.content.encoding !== 'utf-8') {
    return {
      kind: 'ineligible',
      reason: 'Binary files cannot be attached as text context.',
    };
  }
  // Size cap is the WHOLE FILE on disk. Even when the user is
  // attaching just a range, the backend still has to load the
  // whole file (so the redactor sees lines outside the range), so
  // the same cap applies.
  if (selection.content.bytes > PROMPT_READ_MAX_BYTES) {
    return {
      kind: 'ineligible',
      reason: `File is ${formatBytes(selection.content.bytes)}; prompt attachments are capped at ${formatBytes(
        PROMPT_READ_MAX_BYTES,
      )}.`,
    };
  }
  if (chip !== null && chipMatchesSelection(chip, selection.path, lineRange)) {
    return {
      kind: 'already-attached',
      relPath: chip.relPath,
      lineRange: chip.lineRange,
    };
  }
  return {
    kind: 'eligible',
    relPath: selection.path,
    bytes: selection.content.bytes,
    lineRange,
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
  const attachLabel = attachButtonLabel(candidate, chip);
  const attachDisabled = disabled || candidate.kind !== 'eligible';
  const attachTitle = attachButtonTitle(candidate, disabled);
  const chipLabel = chip ? formatChipPath(chip) : null;
  const chipAria =
    chip && chip.lineRange
      ? `Attached selection: ${chipLabel}`
      : chip
        ? `Attached file: ${chipLabel}`
        : null;
  const chipRemoveAria =
    chip && chip.lineRange
      ? `Remove attached selection ${chipLabel}`
      : chip
        ? `Remove attached file ${chipLabel}`
        : '';
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
      {chip && chipLabel && chipAria ? (
        <span
          className="ink-badge plume-chat-attach-chip"
          role="status"
          aria-label={chipAria}
        >
          <span className="plume-chat-attach-chip-icon" aria-hidden>
            ¶
          </span>
          <span className="plume-chat-attach-chip-path" title={chipLabel}>
            {chipLabel}
          </span>
          <span className="plume-chat-attach-chip-meta">
            · {formatBytes(chip.bytes)}
          </span>
          <button
            type="button"
            className="plume-chat-attach-chip-clear"
            onClick={onClear}
            disabled={disabled}
            aria-label={chipRemoveAria}
            title={chip.lineRange ? 'Remove attached selection' : 'Remove attached file'}
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

/// Format the chip's primary label, e.g. `src/main.rs` or
/// `src/main.rs:12–18`. The line-range form uses an en-dash so it
/// reads as a span, not a subtraction.
function formatChipPath(chip: ChipState): string {
  if (chip.lineRange === null) return chip.relPath;
  const { startLine, endLine } = chip.lineRange;
  if (startLine === endLine) return `${chip.relPath}:${startLine}`;
  return `${chip.relPath}:${startLine}–${endLine}`;
}

function attachButtonLabel(candidate: AttachCandidate, chip: ChipState | null): string {
  // While a chip is set the button replaces; the wording for
  // "replace" depends on whether the live selection would attach
  // a range or the whole file.
  const isRangeCandidate =
    candidate.kind === 'eligible' && candidate.lineRange !== null;
  if (chip) {
    return isRangeCandidate ? 'Replace with selection' : 'Replace with current file';
  }
  return isRangeCandidate ? 'Attach selection' : 'Attach current file';
}

function attachButtonTitle(candidate: AttachCandidate, disabledByStream: boolean): string {
  if (disabledByStream) return 'Cannot change attachment while streaming.';
  switch (candidate.kind) {
    case 'eligible': {
      const target =
        candidate.lineRange === null
          ? candidate.relPath
          : `${candidate.relPath} lines ${candidate.lineRange.startLine}–${candidate.lineRange.endLine}`;
      return `Attach ${target} (${formatBytes(candidate.bytes)}) to your next message.`;
    }
    case 'ineligible':
      return candidate.reason;
    case 'already-attached':
      return candidate.lineRange === null
        ? `${candidate.relPath} is already attached.`
        : `${candidate.relPath} lines ${candidate.lineRange.startLine}–${candidate.lineRange.endLine} are already attached.`;
    case 'none':
      return 'Select a UTF-8 text file in the inspector to enable.';
  }
}

function attachHintText(candidate: AttachCandidate): string {
  switch (candidate.kind) {
    case 'eligible':
      if (candidate.lineRange === null) {
        return `Inspector has ${candidate.relPath} ready to attach.`;
      }
      return `Inspector has lines ${candidate.lineRange.startLine}–${candidate.lineRange.endLine} of ${candidate.relPath} selected.`;
    case 'ineligible':
      return candidate.reason;
    case 'already-attached':
      return candidate.lineRange === null
        ? `Attached: ${candidate.relPath}.`
        : `Attached: ${candidate.relPath} (lines ${candidate.lineRange.startLine}–${candidate.lineRange.endLine}).`;
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
  const {
    message,
    modelUsed,
    durationMs,
    attachmentRelPath,
    attachmentLineRange,
    stats,
  } = entry;
  const isAssistant = message.role === 'assistant';
  // D9: the stats footer is only shown when there's at least one
  // useful number to display. `formatStatsLine` returns null when
  // both `outputTokens` and `tokensPerSecond` are absent — the
  // duration alone is already in the model/duration row above.
  const statsLine = isAssistant && stats ? formatStatsLine(stats) : null;
  const statsTitle = isAssistant && stats ? formatStatsTitle(stats) : undefined;
  // D10: build the chip label so single-line and range attachments
  // both render compactly. `attachmentLineRange` is only set when
  // the user attached a selection.
  const attachmentLabel =
    attachmentRelPath !== undefined
      ? attachmentLineRange
        ? attachmentLineRange.startLine === attachmentLineRange.endLine
          ? `${attachmentRelPath}:${attachmentLineRange.startLine}`
          : `${attachmentRelPath}:${attachmentLineRange.startLine}–${attachmentLineRange.endLine}`
        : attachmentRelPath
      : null;
  return (
    <li
      className={`plume-chat-entry plume-chat-entry-${message.role}`}
      aria-label={`${message.role} message`}
    >
      <span className="plume-chat-entry-role">{message.role}</span>
      {attachmentLabel ? (
        <span
          className="ink-badge plume-chat-entry-attachment"
          aria-label={`Attached: ${attachmentLabel}`}
          title={`Attached as read-only context: ${attachmentLabel}`}
        >
          ¶ {attachmentLabel}
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

/// D12: small "what will ride along" preview area between the
/// attach bar and the textarea. Renders one row per piece of
/// context (AGENTS.md when present, the attached file/selection
/// when set) with concrete byte sizes + redaction counts so the
/// user sees what will actually be sent before they press Send.
///
/// Honest about empties: when neither AGENTS.md nor an attachment
/// is in play, the area collapses entirely. We deliberately do
/// NOT render an "empty preview" placeholder — it would just be
/// noise. The user can still see they have no context to send
/// because the chip bar above is empty and the badge is the
/// authority on AGENTS.md state.
///
/// Honest about blocked attachments: the chip bar shows the user
/// what they tried to attach; this preview shows what the BACKEND
/// would do with it. A "blocked" status here surfaces the typed
/// reason from the prompt-read pipeline (secret filename, oversize,
/// path escape, needs approval, …) so the user knows the chip is
/// effectively a no-op for that send. Visually distinguished with
/// a warn variant so it's not confused with a ready item.
type ContextPreviewProps = {
  instructions: ChatContextInstructionsPreview | null;
  attachment: ChatContextAttachmentPreview | null;
  /** Initial-load spinner state. Only used before the first
   * successful response; subsequent refetches keep the previous
   * data visible to avoid flicker. */
  loading: boolean;
  /** Hook-level error (IPC failure, etc.). Rendered as a small
   * one-line hint; doesn't block the send. */
  error: string | null;
};

function ContextPreview({
  instructions,
  attachment,
  loading,
  error,
}: ContextPreviewProps) {
  // Render nothing when there's truly nothing to show. The chat
  // panel works fine with no AGENTS.md and no attachment; an
  // always-visible empty box would just be chrome.
  const hasInstructions = instructions !== null;
  const hasAttachment = attachment !== null;
  if (!hasInstructions && !hasAttachment && !loading && error === null) {
    return null;
  }

  return (
    <div className="plume-chat-context-preview" aria-label="Context preview for next send">
      <span className="plume-chat-context-preview-label">Will ride along:</span>
      {loading && !hasInstructions && !hasAttachment ? (
        <span className="plume-chat-context-preview-loading" role="status">
          Probing…
        </span>
      ) : null}
      {instructions !== null ? (
        <InstructionsPreviewItem instructions={instructions} />
      ) : null}
      {attachment !== null ? (
        <AttachmentPreviewItem attachment={attachment} />
      ) : null}
      {error !== null ? (
        <span
          className="plume-chat-context-preview-error"
          role="status"
          title={error}
        >
          Preview unavailable
        </span>
      ) : null}
    </div>
  );
}

function InstructionsPreviewItem({
  instructions,
}: {
  instructions: ChatContextInstructionsPreview;
}) {
  const sizeLabel = formatBytes(instructions.originalBytes);
  const redactionLabel =
    instructions.redactionCount === 0
      ? ''
      : ` · ${instructions.redactionCount} ${
          instructions.redactionCount === 1 ? 'redaction' : 'redactions'
        }`;
  const tooltip = `${instructions.source}: ${sizeLabel}${redactionLabel} — folded in as system context.`;
  return (
    <span
      className="ink-badge plume-chat-context-preview-item"
      role="status"
      aria-label={`AGENTS.md will ride along, ${sizeLabel}${redactionLabel}.`}
      title={tooltip}
    >
      <span className="plume-chat-context-preview-icon" aria-hidden>
        ¶
      </span>
      <span className="plume-chat-context-preview-name">{instructions.source}</span>
      <span className="plume-chat-context-preview-meta">· {sizeLabel}</span>
      {instructions.redactionCount > 0 ? (
        <span className="plume-chat-context-preview-meta">{redactionLabel}</span>
      ) : null}
    </span>
  );
}

function AttachmentPreviewItem({
  attachment,
}: {
  attachment: ChatContextAttachmentPreview;
}) {
  if (attachment.status === 'ready') {
    const label = formatAttachmentLabel(
      attachment.relPath,
      attachment.startLine,
      attachment.endLine,
    );
    const sizeLabel = formatBytes(attachment.originalBytes);
    const redactionLabel =
      attachment.redactionCount === 0
        ? ''
        : ` · ${attachment.redactionCount} ${
            attachment.redactionCount === 1 ? 'redaction' : 'redactions'
          }`;
    const tooltip = `${label}: ${sizeLabel}${redactionLabel} — read-only context.`;
    return (
      <span
        className="ink-badge plume-chat-context-preview-item"
        role="status"
        aria-label={`Attachment ready: ${label}, ${sizeLabel}${redactionLabel}.`}
        title={tooltip}
      >
        <span className="plume-chat-context-preview-icon" aria-hidden>
          ¶
        </span>
        <span className="plume-chat-context-preview-name">{label}</span>
        <span className="plume-chat-context-preview-meta">· {sizeLabel}</span>
        {attachment.redactionCount > 0 ? (
          <span className="plume-chat-context-preview-meta">{redactionLabel}</span>
        ) : null}
      </span>
    );
  }
  // Blocked.
  const label = attachment.relPath;
  const reason = blockedReasonLabel(attachment.reason);
  return (
    <span
      className="ink-badge plume-chat-context-preview-item plume-chat-context-preview-blocked"
      role="status"
      aria-label={`Attachment would be blocked: ${label} — ${reason}. ${attachment.message}`}
      title={`Backend would reject: ${attachment.message}`}
    >
      <span className="plume-chat-context-preview-icon" aria-hidden>
        ⚠
      </span>
      <span className="plume-chat-context-preview-name">{label}</span>
      <span className="plume-chat-context-preview-meta">· would be blocked</span>
      <span className="plume-chat-context-preview-meta">· {reason}</span>
    </span>
  );
}

/// Pull the human-readable short label from a typed block reason.
/// Kept in one place so the badge + tooltip don't drift.
function blockedReasonLabel(reason: ChatContextBlockReason): string {
  switch (reason) {
    case 'notFound':
      return 'file not found';
    case 'pathEscape':
      return 'path escape';
    case 'blocked':
      return 'safety policy';
    case 'badArgument':
      return 'invalid request';
    case 'needsApproval':
      return 'trust required';
    case 'internal':
      return 'preview failed';
    // Forward compatibility: a future reason the frontend doesn't
    // recognise still renders honestly as "would be blocked", with
    // the human-readable message visible in the tooltip.
    default:
      return 'unknown reason';
  }
}

/// Format the attachment label same way the chip does: `path` or
/// `path:N` or `path:start–end`. Reused so the preview and chip
/// don't drift.
function formatAttachmentLabel(
  relPath: string,
  startLine: number | null,
  endLine: number | null,
): string {
  if (startLine === null || endLine === null) return relPath;
  if (startLine === endLine) return `${relPath}:${startLine}`;
  return `${relPath}:${startLine}–${endLine}`;
}

/// D11: badge rendered next to the read-only badge in the chat
/// header. Three states avoid the "claim from metadata alone" trap
/// the first iteration of this slice hit:
///
///   * `projectHasInstructions === false` → no badge. The project
///     has no AGENTS.md, end of story.
///   * `projectHasInstructions === true && lastIncluded === null`
///     → "AGENTS.md available". Forward-looking promise based on
///     the static `ProjectMeta.hasAgentsMd` flag; no send has
///     resolved yet so we can't say "included" honestly.
///   * `projectHasInstructions === true && lastIncluded === true`
///     → "AGENTS.md included". Backend confirmed the file was
///     folded into the most recent accepted send.
///   * `projectHasInstructions === true && lastIncluded === false`
///     → "AGENTS.md skipped". Backend reported a skip (file
///     present but unreadable — oversize, binary, hardlink,
///     etc.). Visually distinguished so the user notices and can
///     investigate.
type InstructionsBadgeProps = {
  projectHasInstructions: boolean;
  lastIncluded: boolean | null;
};

function InstructionsBadge({
  projectHasInstructions,
  lastIncluded,
}: InstructionsBadgeProps) {
  if (!projectHasInstructions) return null;
  const state: 'available' | 'included' | 'skipped' =
    lastIncluded === null ? 'available' : lastIncluded ? 'included' : 'skipped';
  const label =
    state === 'available'
      ? '¶ AGENTS.md available'
      : state === 'included'
        ? '¶ AGENTS.md included'
        : '¶ AGENTS.md skipped';
  const aria =
    state === 'available'
      ? 'Project AGENTS.md available; will be folded in on the next send.'
      : state === 'included'
        ? 'Project AGENTS.md was included as system context on the most recent send.'
        : 'Project AGENTS.md was skipped on the most recent send — check that the file is readable text under 256 KiB.';
  const tooltip =
    state === 'available'
      ? "The project has an AGENTS.md at its root. Plume will read and fold it in as a system message on your next send."
      : state === 'included'
        ? "Backend confirmed AGENTS.md was folded in as a system message on the last send."
        : "Backend reported the last send did NOT include AGENTS.md. Likely the file is oversize, binary, or unreadable.";
  const className =
    state === 'skipped'
      ? 'ink-badge plume-chat-instructions-badge plume-chat-instructions-badge-skipped'
      : 'ink-badge plume-chat-instructions-badge';
  return (
    <span className={className} role="status" aria-label={aria} title={tooltip}>
      {label}
    </span>
  );
}

/// Subtitle hint mirrors the badge: "available" before the first
/// send, "included on the last send" once a send has resolved
/// successfully, "skipped on the last send" if the backend
/// reported a skip. Suppressed entirely when the project has no
/// AGENTS.md.
function instructionsSubtitleHint(
  projectHasInstructions: boolean,
  lastIncluded: boolean | null,
): string {
  if (!projectHasInstructions) return '';
  if (lastIncluded === null) {
    return "The project's AGENTS.md will ride along as read-only system context on your next send. ";
  }
  if (lastIncluded === true) {
    return "The project's AGENTS.md was folded into the last send as read-only system context. ";
  }
  return "The project's AGENTS.md was skipped on the last send — check that it's readable text under 256 KiB. ";
}
