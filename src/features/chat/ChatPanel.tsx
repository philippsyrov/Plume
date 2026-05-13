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
import {
  useProviderReachability,
  type ProviderReachabilityState,
} from './useProviderReachability';
import type {
  ChatAttachment,
  ChatContextAttachmentPreview,
  ChatContextBlockReason,
  ChatContextInstructionsPreview,
  ChatMode,
} from '../../lib/api/chat';
import { PROMPT_READ_MAX_BYTES } from '../../lib/api/chat';
import { ipcErrorMessage, isIpcError } from '../../lib/api/errors';
import type { PatchTouch, PatchValidationError } from '../../lib/api/patch';
import { validatePatch } from '../../lib/api/patch';
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
  // D15: response-shape mode for the next send. Window-local
  // state; closing the project resets to 'chat'. Mid-stream
  // toggling is allowed but only affects the NEXT send — the
  // in-flight one keeps the mode it was started with.
  const [mode, setMode] = useState<ChatMode>('chat');
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

  // D14: pre-flight the selected model's provider so the chat
  // panel can show "Ollama not running" BEFORE the user types and
  // hits Send. Probes once on mount and whenever the selected
  // provider changes; the user can also click "Recheck" to
  // re-probe after starting the daemon outside Plume.
  const reachability = useProviderReachability(selected?.providerId ?? null);
  const providerUnreachable = isProviderUnreachable(selected, reachability);
  const providerChecking = isProviderChecking(selected, reachability);

  const disabledReason = computeDisabledReason(
    selected,
    status,
    providerUnreachable,
    providerChecking,
  );
  const isStreaming = status === 'streaming';
  const canSend = disabledReason === null && draft.trim().length > 0 && !isStreaming;

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
      // D14: capture the chip BEFORE clearing so we can restore it
      // if the backend rejects synchronously. Pre-D14, the chip
      // was a hard one-shot: the panel cleared it before the IPC
      // resolved, and a rejection (Ollama down, provider mismatch,
      // …) left the user re-attaching the same file. With the
      // `SendOutcome`-aware restore, the chip is one-shot only
      // when the request was actually accepted.
      const pendingChip = chip;
      setDraft('');
      // Clearing immediately mirrors how the textarea clears so
      // the user sees a clean slate. We restore below on
      // `'rejected'`; `'accepted'` keeps the chip consumed.
      setChip(null);
      void send(selected.providerId, selected.modelId, text, {
        ...(attachment ? { attachment } : {}),
        ...(mode !== 'chat' ? { mode } : {}),
      }).then((outcome) => {
        if (outcome === 'rejected' && pendingChip !== null) {
          // Only restore if the user hasn't attached something
          // new in the meantime — they may have grabbed a
          // different file while the rejection was in flight.
          setChip((current) => (current === null ? pendingChip : current));
        }
      });
    },
    [canSend, chip, draft, mode, selected, send],
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
        <div className="plume-chat-header-controls">
          <ModeToggle mode={mode} onChange={setMode} disabled={isStreaming} />
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
        </div>
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
            disabled={isInputDisabled(disabledReason)}
            aria-label="Message to send"
            rows={3}
          />
        </label>
        <div className="plume-chat-form-bar">
          <span className="plume-chat-status" role="status" aria-live="polite">
            {chatStatusText(selected, disabledReason, isStreaming)}
          </span>
          {disabledReason === 'provider-unreachable' ||
          disabledReason === 'provider-checking' ? (
            <button
              type="button"
              className="ink-button plume-chat-recheck"
              onClick={reachability.refresh}
              disabled={reachability.status === 'loading'}
              aria-label={`Recheck ${selected?.providerDisplayName ?? 'provider'} reachability`}
              title={
                reachability.status === 'loading'
                  ? `Probing ${selected?.providerDisplayName ?? 'the provider'}…`
                  : `Re-probe ${selected?.providerDisplayName ?? 'the provider'} now. Plume probed on chat panel mount; clicking refetches without remounting the project.`
              }
            >
              {reachability.status === 'loading' ? 'Rechecking…' : 'Recheck'}
            </button>
          ) : null}
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
  // D15: dispatch on the mode the turn was sent in. User turns
  // get a small "Propose diff" hint inline; assistant turns get
  // the diff renderer when their requesting send used that mode.
  // Falls through to plain-text content rendering otherwise.
  const wasProposeDiff = entry.sentInMode === 'proposeDiff';
  const parsedDiff =
    isAssistant && wasProposeDiff ? extractDiffBlock(message.content) : null;

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
      {message.role === 'user' && wasProposeDiff ? (
        <span
          className="ink-badge plume-chat-entry-mode"
          aria-label="Sent in propose-diff mode"
          title="This turn asked the model to respond with a unified diff."
        >
          ¶ propose diff
        </span>
      ) : null}
      {parsedDiff !== null ? (
        <DiffPreview diff={parsedDiff} replyText={message.content} />
      ) : (
        <p className="plume-chat-entry-content">{message.content}</p>
      )}
      {isAssistant ? <CopyReplyButton text={message.content} /> : null}
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
      {isAssistant && wasProposeDiff && parsedDiff === null ? (
        <p
          className="plume-chat-entry-meta plume-chat-entry-mode-note"
          role="status"
          aria-label="Model did not return a unified diff in propose-diff mode"
        >
          No diff fence detected — model returned prose. Try again or
          rephrase the request.
        </p>
      ) : null}
    </li>
  );
}

/// D15: segmented mode toggle in the chat header. Two visible
/// states today (`'chat'` and `'proposeDiff'`); the array shape
/// lets future modes (`'scopedEdit'`, `'agentLoop'`) plug in
/// without restructuring the component. Disabled while a stream
/// is in flight — flipping mode mid-stream would be confusing
/// because the in-flight turn keeps the mode it was started with.
type ModeOption = {
  value: ChatMode;
  label: string;
  description: string;
};

const MODE_OPTIONS: readonly ModeOption[] = [
  {
    value: 'chat',
    label: 'Chat',
    description: 'Free-form text reply. The default Plume conversation mode.',
  },
  {
    value: 'proposeDiff',
    label: 'Propose diff',
    description:
      'Ask the model for a unified-diff preview. Plume renders the diff inline; it does NOT apply patches in this slice.',
  },
];

function ModeToggle({
  mode,
  onChange,
  disabled,
}: {
  mode: ChatMode;
  onChange: (next: ChatMode) => void;
  disabled: boolean;
}) {
  return (
    <div
      className="plume-chat-mode-toggle"
      role="radiogroup"
      aria-label="Response mode for next send"
    >
      {MODE_OPTIONS.map((opt) => {
        const active = opt.value === mode;
        return (
          <button
            key={opt.value}
            type="button"
            role="radio"
            aria-checked={active}
            className={
              active
                ? 'plume-chat-mode-option plume-chat-mode-option-active'
                : 'plume-chat-mode-option'
            }
            disabled={disabled}
            onClick={() => onChange(opt.value)}
            title={opt.description}
          >
            {opt.label}
          </button>
        );
      })}
    </div>
  );
}

/// D15: extract the unified diff body from an assistant reply.
/// Looks for a single fenced ```diff or ```patch code block; if
/// found, returns the inner content. Otherwise returns null and
/// the caller renders the raw text with a "no diff detected"
/// hint. We deliberately don't try to parse raw diffs without a
/// fence — that boundary keeps the parser simple, and the system
/// message instructs the model to use a fence anyway.
///
/// The regex is intentionally lenient: any case for the language
/// tag, an optional language tag at all (so a bare ``` followed
/// by what looks like a diff still works if the model forgot the
/// `diff` tag but otherwise complied), and trailing whitespace
/// inside the fence is preserved.
function extractDiffBlock(reply: string): string | null {
  // Try the explicit `diff` / `patch` tagged fence first.
  const tagged = /```(?:diff|patch)\s*\n([\s\S]*?)```/i.exec(reply);
  if (tagged && tagged[1]) {
    return tagged[1].replace(/\n$/, '');
  }
  // Fallback: any fenced block whose first line looks like a
  // unified-diff header (`--- ` followed by `+++ ` on the next).
  // This catches models that drop the language tag but still
  // produce a valid diff inside a fence.
  const untagged = /```(?:[a-zA-Z]*)\s*\n(--- [^\n]+\n\+\+\+ [^\n]+\n[\s\S]*?)```/i.exec(reply);
  if (untagged && untagged[1]) {
    return untagged[1].replace(/\n$/, '');
  }
  return null;
}

/// D15: render a unified diff with per-line coloring. Each line
/// is classified by its first character:
///   `+` — addition
///   `-` — deletion
///   `@` — hunk header (`@@ -1,4 +1,5 @@`)
///   `-` or `+` followed by `--` / `++` is a file header (the
///       regex above already routes those through; we treat them
///       as headers, not as add/remove)
///   anything else — context
///
/// The renderer is intentionally simple: it does NOT validate the
/// diff applies cleanly, does NOT match hunks against any file,
/// does NOT highlight syntax inside the changed lines. It just
/// gives the user a readable visual.
///
/// The "Apply" button is rendered **disabled** with a tooltip
/// naming the boundary. Plume does not apply patches in D15. The
/// existing Copy button on the parent assistant entry already
/// covers "grab the diff and apply by hand."
type DiffLineKind = 'add' | 'del' | 'hunk' | 'header' | 'context';

function classifyDiffLine(line: string): DiffLineKind {
  if (line.startsWith('+++') || line.startsWith('---')) return 'header';
  if (line.startsWith('@@')) return 'hunk';
  if (line.startsWith('+')) return 'add';
  if (line.startsWith('-')) return 'del';
  return 'context';
}

function DiffPreview({ diff, replyText }: { diff: string; replyText: string }) {
  const lines = useMemo(() => diff.split('\n'), [diff]);
  const validation = useDiffValidation(replyText);
  return (
    <div className="plume-chat-diff" role="group" aria-label="Proposed diff preview">
      <pre className="plume-chat-diff-body">
        {lines.map((line, i) => {
          const kind = classifyDiffLine(line);
          return (
            <span
              key={i}
              className={`plume-chat-diff-line plume-chat-diff-line-${kind}`}
              role={kind === 'add' || kind === 'del' ? 'text' : undefined}
              aria-label={
                kind === 'add'
                  ? `Added: ${line.slice(1)}`
                  : kind === 'del'
                    ? `Removed: ${line.slice(1)}`
                    : undefined
              }
            >
              {line}
              {'\n'}
            </span>
          );
        })}
      </pre>
      <DiffValidationPill validation={validation} />
      <div className="plume-chat-diff-actions">
        <button
          type="button"
          className="ink-button plume-chat-diff-apply"
          disabled
          aria-label={
            validation.state === 'valid'
              ? 'Apply this diff (disabled — validation passed but apply is future)'
              : 'Apply this diff (disabled — preview only)'
          }
          title={
            validation.state === 'valid'
              ? 'Validation passed, but Plume does not apply patches yet. Use the Copy button on the assistant turn to grab this diff and apply it by hand.'
              : "Plume can't apply diffs yet — preview only. Use the Copy button on the assistant turn to grab this diff and apply it by hand."
          }
        >
          Apply
        </button>
        <span className="plume-chat-diff-actions-note" role="status">
          preview only — no writes
        </span>
      </div>
    </div>
  );
}

/// D16: thin hook that runs `patch.validate` once per finalized
/// propose-diff reply and exposes a small `'loading' | 'valid' |
/// 'invalid' | 'failed'` state for the pill.
///
/// `replyText` is the full assistant reply (including the fenced
/// markers) so the backend sees what the user would copy. The hook
/// fires once on mount; subsequent re-renders are no-ops because
/// the reply text on a finalized message entry never changes. The
/// `Internal` / `NeedsApproval` paths from the IPC layer surface
/// as `'failed'` with the human message — the UI shouldn't
/// disappear or block the diff renderer just because validation
/// couldn't complete.
type DiffValidationState =
  | { state: 'loading' }
  | { state: 'valid'; touches: PatchTouch[]; hunks: number }
  | { state: 'invalid'; errors: PatchValidationError[] }
  | { state: 'failed'; message: string };

function useDiffValidation(replyText: string): DiffValidationState {
  const [validation, setValidation] = useState<DiffValidationState>({ state: 'loading' });
  useEffect(() => {
    let cancelled = false;
    setValidation({ state: 'loading' });
    validatePatch({ diff: replyText })
      .then((resp) => {
        if (cancelled) return;
        if (resp.ok) {
          setValidation({ state: 'valid', touches: resp.touches, hunks: resp.hunks });
        } else {
          setValidation({ state: 'invalid', errors: resp.errors });
        }
      })
      .catch((err: unknown) => {
        if (cancelled) return;
        const message = isIpcError(err) ? ipcErrorMessage(err) : 'validation failed';
        setValidation({ state: 'failed', message });
      });
    return () => {
      cancelled = true;
    };
  }, [replyText]);
  return validation;
}

function DiffValidationPill({ validation }: { validation: DiffValidationState }) {
  if (validation.state === 'loading') {
    return (
      <p
        className="plume-chat-diff-validation plume-chat-diff-validation-loading"
        role="status"
        aria-live="polite"
      >
        validating diff…
      </p>
    );
  }
  if (validation.state === 'valid') {
    const fileWord = validation.touches.length === 1 ? 'file' : 'files';
    const hunkWord = validation.hunks === 1 ? 'hunk' : 'hunks';
    return (
      <p
        className="plume-chat-diff-validation plume-chat-diff-validation-valid"
        role="status"
        aria-live="polite"
      >
        valid diff · {validation.touches.length} {fileWord} · {validation.hunks} {hunkWord}
      </p>
    );
  }
  if (validation.state === 'invalid') {
    const headline = validation.errors[0];
    return (
      <p
        className="plume-chat-diff-validation plume-chat-diff-validation-invalid"
        role="status"
        aria-live="polite"
        title={validation.errors.map((e) => e.message).join('\n')}
      >
        invalid diff: {headline.message}
      </p>
    );
  }
  return (
    <p
      className="plume-chat-diff-validation plume-chat-diff-validation-failed"
      role="status"
      aria-live="polite"
    >
      validation unavailable: {validation.message}
    </p>
  );
}

/// D14: per-reply Copy button on completed assistant turns. Only
/// uses `navigator.clipboard.writeText` — no new dependencies, no
/// IPC. The two-second "Copied!" state gives the user a quick
/// confirmation without a toast/modal. Streaming and cancelled
/// turns deliberately don't get a button — copying a partial
/// reply mid-stream would be a footgun (the user could miss
/// content that arrives moments later).
function CopyReplyButton({ text }: { text: string }) {
  const [state, setState] = useState<'idle' | 'copied' | 'failed'>('idle');
  const onCopy = useCallback(async () => {
    if (!text) return;
    try {
      // `navigator.clipboard` is available in Tauri's webview;
      // gate on its existence anyway so a future headless test
      // harness doesn't crash here.
      if (typeof navigator !== 'undefined' && navigator.clipboard) {
        await navigator.clipboard.writeText(text);
        setState('copied');
      } else {
        setState('failed');
      }
    } catch {
      setState('failed');
    }
    // Auto-revert after a beat so subsequent copies don't appear
    // stuck on the previous status. 2 s is the same window the
    // attachment chip uses for its transient labels.
    window.setTimeout(() => setState('idle'), 2000);
  }, [text]);
  const label =
    state === 'idle'
      ? 'Copy'
      : state === 'copied'
        ? 'Copied!'
        : 'Copy failed';
  return (
    <button
      type="button"
      className="plume-chat-copy-button"
      onClick={onCopy}
      aria-label="Copy assistant reply text to clipboard"
      title="Copy the reply text to your clipboard."
    >
      {label}
    </button>
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

/// D14: `'provider-unreachable'` joins the older disabled states.
/// It only fires when the user has picked a supported model AND
/// the reachability probe came back as offline / not-configured.
/// The provider name is generic here so the same code path covers
/// the future LM Studio + llama.cpp adapters without a new variant.
///
/// `'provider-checking'` is the transient state between clicking
/// `Recheck` (or first mount) and the probe resolving. Pre-fix
/// this wasn't a distinct reason: `isProviderUnreachable` only
/// returned `true` on `status === 'ready' && reachability !==
/// 'available'`, so the moment the user clicked Recheck the hook
/// flipped to `loading`, the disabled-reason dropped to `null`,
/// the Recheck button vanished, and Send briefly enabled before
/// the new probe result landed. That contradicted the SMOKE
/// expectation of a stable `Rechecking…` button and was a real
/// flicker for the user. The distinct state keeps the Recheck
/// affordance visible (and disabled) while the probe is in
/// flight, and Send stays gated.
type DisabledReason =
  | 'no-selection'
  | 'unsupported-provider'
  | 'streaming'
  | 'provider-checking'
  | 'provider-unreachable'
  | null;

function computeDisabledReason(
  selected: SelectedModel | null,
  status: 'idle' | 'streaming' | 'error',
  providerUnreachable: boolean,
  providerChecking: boolean,
): DisabledReason {
  if (status === 'streaming') return 'streaming';
  if (selected === null) return 'no-selection';
  if (selected.providerId !== SUPPORTED_PROVIDER_ID) return 'unsupported-provider';
  // Order matters: unreachable wins over checking. If the previous
  // probe already returned "not available" we surface that copy
  // immediately and the user can act; the in-flight refresh just
  // updates the Recheck button label.
  if (providerUnreachable) return 'provider-unreachable';
  if (providerChecking) return 'provider-checking';
  return null;
}

/// Treat the probe result as "unreachable" only when we have a
/// definitive answer. `loading`, `idle`, and `error` all collapse
/// to "we don't know" — better to let the user try Send and learn
/// from the actual transport error than to lock them out on a
/// flaky `providers.health` IPC.
function isProviderUnreachable(
  selected: SelectedModel | null,
  reachability: ProviderReachabilityState,
): boolean {
  if (selected === null) return false;
  if (selected.providerId !== SUPPORTED_PROVIDER_ID) return false;
  if (reachability.status !== 'ready') return false;
  return reachability.reachability !== 'available';
}

/// `true` while a reachability probe is in flight for the
/// currently-selected supported provider. Keeps the UI on the
/// Recheck-aware code path during the brief window between
/// clicking Recheck and the new snapshot landing. `'idle'` and
/// `'error'` deliberately don't qualify — those are "we don't
/// know" states that fall through to the optimistic null branch.
function isProviderChecking(
  selected: SelectedModel | null,
  reachability: ProviderReachabilityState,
): boolean {
  if (selected === null) return false;
  if (selected.providerId !== SUPPORTED_PROVIDER_ID) return false;
  return reachability.status === 'loading';
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
    case 'provider-checking':
      return `Type your message — checking ${selected?.providerDisplayName ?? 'the daemon'} reachability…`;
    case 'provider-unreachable':
      // Textarea stays ENABLED for this state (see `isInputDisabled`
      // helper) so the user can compose while starting the
      // daemon. The placeholder tells them how to unblock Send.
      return `Type your message — start ${selected?.providerDisplayName ?? 'the daemon'} and click Recheck to send.`;
    case null:
      return `Send a message to ${selected?.modelId ?? 'the model'}…`;
  }
}

/// `disabledReason !== null` is too broad for the textarea — the
/// `'provider-unreachable'` and `'provider-checking'` cases should
/// still let the user type so they can compose a prompt while the
/// daemon comes up or while the probe is in flight. Send stays
/// disabled regardless. Pulled into a helper so the next state
/// that wants the same treatment can opt in by name.
function isInputDisabled(reason: DisabledReason): boolean {
  if (reason === null) return false;
  if (reason === 'provider-unreachable') return false;
  if (reason === 'provider-checking') return false;
  return true;
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
    case 'provider-checking':
      return `Checking ${selected?.providerDisplayName ?? 'provider'} reachability…`;
    case 'provider-unreachable':
      return `${selected?.providerDisplayName ?? 'Provider'} not reachable — start the daemon and click Recheck.`;
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
      <span className="plume-chat-context-preview-label">Context preview:</span>
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
        !
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
