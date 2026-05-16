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
//
// D22 split this file into focused siblings (`AttachBar`,
// `ChatEntryRow`, `ContextPreview`, `DiffPreview`, `ModeToggle`,
// `InstructionsBadge`, `CopyReplyButton`, `formatters.ts`,
// `disabledReason.ts`). This file owns the orchestration: state,
// effects, IPC plumbing, and the top-level JSX skeleton.

import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type FormEvent,
  type KeyboardEvent,
} from 'react';

import { AttachBar, describeAttachCandidate, type ChipState } from './AttachBar';
import { ChatEntryRow } from './ChatEntryRow';
import { ContextPreview } from './ContextPreview';
import {
  chatStatusText,
  computeDisabledReason,
  inputPlaceholder,
  isInputDisabled,
  isProviderChecking,
  isProviderUnreachable,
} from './disabledReason';
import { InstructionsBadge, MemoryBadge, instructionsSubtitleHint } from './InstructionsBadge';
import { ModeToggle } from './ModeToggle';
import { useChat } from './useChat';
import { useChatContextPreview } from './useChatContextPreview';
import { useProviderReachability } from './useProviderReachability';
import type { ChatAttachment, ChatMode } from '../../lib/api/chat';
import type { EditorLineRange } from '../editor/ReadOnlyEditor';
import type { SelectionState } from '../file-tree/FileBrowser';
import type { SelectedModel } from '../model-picker/useSelectedModel';
import {
  MLX_LM_PROVIDER_ID,
  type MlxServersApi,
} from '../providers/useMlxServers';

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
  /** D46: bus used to look up the live MLX server handle for the
   * currently-selected model. When `selected.providerId === 'mlx-lm'`
   * the chat panel reads `handleOf(modelId)` and threads its id
   * into `chat.send` via the D45 `handleId` field. */
  mlxServers: MlxServersApi;
};

export function ChatPanel({
  selected,
  inspectorSelection,
  inspectorLineRange,
  projectHasInstructions,
  mlxServers,
}: ChatPanelProps) {
  const {
    entries,
    status,
    activeStreamId,
    lastInstructionsIncluded,
    lastMemoryUsed,
    send,
    cancel,
    clear,
  } = useChat();
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

  // D46 Codex fix: for an `mlx-lm` selection, the disabled-reason
  // gate looks at the supervisor handle registry instead of the
  // Ollama-shaped reachability probe. We pre-compute presence here
  // (rather than passing the whole `mlxServers` API through) so
  // `disabledReason.ts` can stay pure / hook-free.
  const mlxHandlePresent =
    selected?.providerId === MLX_LM_PROVIDER_ID
      ? mlxServers.handleOf(selected.modelId) !== null
      : false;

  const disabledReason = computeDisabledReason(
    selected,
    status,
    providerUnreachable,
    providerChecking,
    mlxHandlePresent,
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
      // D46: when chat dispatch is going to the MLX adapter, pass
      // the bound server handle id along. We look it up here, not
      // in `useChat`, so the hook stays agnostic about which
      // provider needs which extra field. A missing handle for an
      // mlx-lm selection still goes through — the backend rejects
      // with `BadArgument` and the inline error tells the user to
      // start the server.
      const mlxHandle =
        selected.providerId === MLX_LM_PROVIDER_ID
          ? mlxServers.handleOf(selected.modelId)
          : null;
      void send(selected.providerId, selected.modelId, text, {
        ...(attachment ? { attachment } : {}),
        ...(mode !== 'chat' ? { mode } : {}),
        ...(mlxHandle ? { handleId: mlxHandle.id } : {}),
      }).then((outcome) => {
        if (outcome === 'rejected' && pendingChip !== null) {
          // Only restore if the user hasn't attached something
          // new in the meantime — they may have grabbed a
          // different file while the rejection was in flight.
          setChip((current) => (current === null ? pendingChip : current));
        }
      });
    },
    [canSend, chip, draft, mode, mlxServers, selected, send],
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
          <MemoryBadge
            preview={contextPreview.data?.memory ?? null}
            lastUsed={lastMemoryUsed}
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
