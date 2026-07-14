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
//   * Two providers: Ollama and Plume-managed MLX-LM (D45). If the
//     selected model is from any other provider the input is
//     disabled with a clear explanation.
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
import { ContextShelf } from './ContextShelf';
import { ContextPreview } from './ContextPreview';
import {
  chatStatusText,
  computeDisabledReason,
  inputPlaceholder,
  isInputDisabled,
  isProviderChecking,
  isProviderUnreachable,
} from './disabledReason';
import {
  InstructionsBadge,
  MemoryBadge,
  TopicsBadge,
  instructionsSubtitleHint,
} from './InstructionsBadge';
import { ModeToggle } from './ModeToggle';
import { useChat, type ChatApi } from './useChat';
import { useChatContextPreview } from './useChatContextPreview';
import { useProviderReachability } from './useProviderReachability';
import type { ChatMode, ContextSourceRef } from '../../lib/api/chat';
import type { EditorLineRange } from '../editor/ReadOnlyEditor';
import type { SelectionState } from '../file-tree/FileBrowser';
import type { SelectedModel } from '../model-picker/useSelectedModel';
import {
  MLX_LM_PROVIDER_ID,
  type MlxServersApi,
  type MlxServerStatus,
} from '../providers/useMlxServers';

export type ChatPanelProps = {
  selected: SelectedModel | null;
  /** D62: clear lives in the chat model selector now, not in a
   * separate workspace banner. */
  onClearSelection: () => void;
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
  /** Defaults to true. No-project chat passes false so this panel
   * stays a plain chat surface even if the backend still remembers
   * a trusted project from earlier in the same window. */
  includeProjectContext?: boolean;
  /** Workspace mode keeps the full project chat chrome. Simple mode
   * is the no-project launch chat: quiet transcript + composer, with
   * model/settings handled by the outer shell. */
  variant?: 'workspace' | 'simple';
  /** D63B: externally-owned chat instance. The session shell hoists
   * `useChat()` so persistence (`usePersistedChat`) can observe
   * transcript boundaries and restore loaded sessions; when set, this
   * panel renders that instance instead of its own. Omitted, the
   * panel behaves exactly as before D63B (window-local, unpersisted). */
  chat?: ChatApi;
  /** One-shot presentation key used after a cross-view drop. */
  emphasizedContextKey?: string | null;
};

export function ChatPanel({
  selected,
  onClearSelection,
  inspectorSelection,
  inspectorLineRange,
  projectHasInstructions,
  mlxServers,
  includeProjectContext = true,
  variant = 'workspace',
  chat,
  emphasizedContextKey = null,
}: ChatPanelProps) {
  // Hooks must run unconditionally; when the shell passes an external
  // instance the internal one stays idle and unobserved.
  const internalChat = useChat();
  const {
    entries,
    status,
    activeStreamId,
    lastInstructionsIncluded,
    lastMemoryUsed,
    lastTopicsUsed,
    contextSources,
    addContextSource,
    removeContextSource,
    send,
    cancel,
    clear,
  } = chat ?? internalChat;
  const [draft, setDraft] = useState('');
  const [contextActionError, setContextActionError] = useState<string | null>(null);
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

  const matchingFileSource = useMemo<ChipState | null>(() => {
    if (inspectorSelection?.kind !== 'ready') return null;
    const match = contextSources.find((source) => {
      if (source.kind !== 'projectFile' || source.relPath !== inspectorSelection.path) {
        return false;
      }
      if (inspectorLineRange === null) {
        return source.startLine === undefined && source.endLine === undefined;
      }
      return (
        source.startLine === inspectorLineRange.startLine &&
        source.endLine === inspectorLineRange.endLine
      );
    });
    return match
      ? {
          relPath: match.kind === 'projectFile' ? match.relPath : '',
          bytes: inspectorSelection.content.bytes,
          lineRange: inspectorLineRange,
        }
      : null;
  }, [contextSources, inspectorLineRange, inspectorSelection]);

  const attachCandidate = useMemo(
    () => describeAttachCandidate(inspectorSelection, inspectorLineRange, matchingFileSource),
    [inspectorSelection, inspectorLineRange, matchingFileSource],
  );

  // D12: ask the backend what would ride along on the next send.
  // The hook re-fires when the chip changes or the project's
  // AGENTS.md state flips. We pass primitives, not the chip object,
  // so the effect only fires when the relevant fields actually
  // change (object identity would re-fire on every render).
  const contextPreview = useChatContextPreview({
    relPath: null,
    startLine: null,
    endLine: null,
    contextSources,
    projectHasInstructions,
    includeProjectContext,
    ...(selected
      ? { providerId: selected.providerId, modelId: selected.modelId }
      : {}),
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
  const mlxServerStatus =
    selected?.providerId === MLX_LM_PROVIDER_ID
      ? mlxServers.statusOf(selected.modelId)
      : null;

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
    const source: ContextSourceRef = {
      kind: 'projectFile',
      relPath: attachCandidate.relPath,
      ...(attachCandidate.lineRange
        ? {
            startLine: attachCandidate.lineRange.startLine,
            endLine: attachCandidate.lineRange.endLine,
          }
        : {}),
    };
    const result = addContextSource(source);
    setContextActionError(
      result === 'full'
        ? 'Context shelf is full (16 items). Remove one before adding another.'
        : result === 'unavailable'
          ? 'Wait for the current reply to finish before changing context.'
          : null,
    );
  }, [addContextSource, attachCandidate]);

  const submit = useCallback(
    (e?: FormEvent) => {
      if (e) e.preventDefault();
      if (!canSend || !selected) return;
      const text = draft;
      setDraft('');
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
        ...(mode !== 'chat' ? { mode } : {}),
        ...(mlxHandle ? { handleId: mlxHandle.id } : {}),
        ...(includeProjectContext ? {} : { includeProjectContext: false }),
      });
    },
    [canSend, draft, includeProjectContext, mode, mlxServers, selected, send],
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
  const isSimple = variant === 'simple';

  return (
    <section
      className={`plume-chat ${isSimple ? 'plume-chat-simple' : 'ink-panel'}`}
      aria-label="Chat with selected model"
      aria-describedby="plume-chat-subtitle"
    >
      {isSimple ? (
        <div id="plume-chat-subtitle" className="plume-chat-simple-bar">
          {includeProjectContext ? (
            <div className="plume-chat-simple-context" aria-label="Project context">
              <InstructionsBadge
                projectHasInstructions={projectHasInstructions}
                lastIncluded={lastInstructionsIncluded}
              />
              <MemoryBadge
                preview={contextPreview.data?.memory ?? null}
                lastUsed={lastMemoryUsed}
              />
              <TopicsBadge
                preview={contextPreview.data?.topics ?? null}
                lastUsed={lastTopicsUsed}
              />
            </div>
          ) : null}
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
      ) : (
        <>
          <header className="plume-chat-header">
            <div className="plume-chat-title">
              <h3>Chat</h3>
              <span className="ink-badge plume-chat-readonly-badge">read-only</span>
              {includeProjectContext ? (
                <>
                  <InstructionsBadge
                    projectHasInstructions={projectHasInstructions}
                    lastIncluded={lastInstructionsIncluded}
                  />
                  <MemoryBadge
                    preview={contextPreview.data?.memory ?? null}
                    lastUsed={lastMemoryUsed}
                  />
                  <TopicsBadge
                    preview={contextPreview.data?.topics ?? null}
                    lastUsed={lastTopicsUsed}
                  />
                </>
              ) : null}
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
          <ChatModelSelector
            selected={selected}
            mlxStatus={mlxServerStatus}
            onClear={onClearSelection}
            onStop={
              selected?.providerId === MLX_LM_PROVIDER_ID
                ? () => void mlxServers.stop(selected.modelId)
                : undefined
            }
          />
          <p id="plume-chat-subtitle" className="plume-chat-subtitle">
            Read-only chat.{' '}
            {instructionsSubtitleHint(projectHasInstructions, lastInstructionsIncluded)}
            Add explicit project context when needed; Plume resolves and redacts
            every source before sending. No file writes or commands.
          </p>
        </>
      )}

      <ol
        id={transcriptId}
        className="plume-chat-transcript"
        ref={listRef}
        aria-live="polite"
        aria-relevant="additions text"
      >
        {entries.length === 0 ? (
          <li className="plume-chat-empty" role="status">
            {includeProjectContext
              ? 'Project chat. Project context is enabled for messages.'
              : 'Local chat. No project context is included.'}{' '}
            Type below to start a streaming read-only chat.
          </li>
        ) : (
          entries.map((entry, i) => <ChatEntryRow key={i} entry={entry} />)
        )}
      </ol>

      <form className="plume-chat-form" onSubmit={submit} aria-controls={transcriptId}>
        {includeProjectContext ? (
          <>
            <AttachBar
              chip={null}
              candidate={attachCandidate}
              onAttach={onAttach}
              onClear={() => undefined}
              disabled={isStreaming}
              placement="chatShelf"
            />
            <ContextShelf
              sources={contextSources}
              preview={contextPreview.data?.contextSources ?? []}
              loading={contextPreview.status === 'loading'}
              disabled={isStreaming}
              emphasizedContextKey={emphasizedContextKey}
              onRemove={(source) => {
                removeContextSource(source);
                setContextActionError(null);
              }}
            />
            {contextActionError ? (
              <p className="plume-context-shelf-error" role="status">
                {contextActionError}
              </p>
            ) : null}
            <ContextPreview
              instructions={contextPreview.data?.instructions ?? null}
              attachment={null}
              loading={contextPreview.status === 'loading' && contextPreview.data === null}
              error={contextPreview.status === 'error' ? contextPreview.error : null}
            />
          </>
        ) : null}
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

function ChatModelSelector({
  selected,
  mlxStatus,
  onClear,
  onStop,
}: {
  selected: SelectedModel | null;
  mlxStatus: MlxServerStatus | null;
  onClear: () => void;
  onStop: (() => void) | undefined;
}) {
  if (selected === null) {
    return (
      <div className="plume-chat-model-selector" aria-label="Current model">
        <span className="plume-chat-model-empty">No model selected</span>
      </div>
    );
  }

  const running = mlxStatus?.kind === 'running' ? mlxStatus.handle : null;
  const isBusy = mlxStatus?.kind === 'starting' || mlxStatus?.kind === 'stopping';

  return (
    <div className="plume-chat-model-selector" aria-label="Current model">
      <span className="plume-chat-model-label">Model</span>
      <span className="plume-chat-model-provider">{selected.providerDisplayName}</span>
      <span className="plume-chat-model-name" title={selected.modelId}>
        {selected.modelId}
      </span>
      {running ? (
        <span
          className="ink-badge plume-chat-model-port"
          title={`mlx-lm bound to 127.0.0.1:${running.port} (pid ${running.pid})`}
        >
          port {running.port}
        </span>
      ) : null}
      {isBusy ? (
        <span className="plume-chat-model-status" role="status">
          {mlxStatus.kind === 'starting' ? 'starting…' : 'stopping…'}
        </span>
      ) : null}
      {running && onStop ? (
        <button
          type="button"
          className="ink-button plume-chat-model-stop"
          onClick={onStop}
        >
          Stop
        </button>
      ) : null}
      <button
        type="button"
        className="ink-button plume-chat-model-clear"
        onClick={onClear}
        aria-label={`Clear selected model ${selected.providerDisplayName} ${selected.modelId}`}
      >
        Change
      </button>
    </div>
  );
}
