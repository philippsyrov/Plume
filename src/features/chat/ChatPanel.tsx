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
import { ChatModelSelector } from './ChatModelSelector';
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
import { ipcErrorMessage, isIpcError } from '../../lib/api/errors';
import type { ChatContextOwner, ChatMode, ContextSourceRef } from '../../lib/api/chat';
import { QWEN_CATALOG_ID } from '../../lib/api/providers';
import { exportResearchArtifact } from '../../lib/api/research';
import type { EditorLineRange } from '../editor/ReadOnlyEditor';
import type { SelectionState } from '../file-tree/FileBrowser';
import type { SelectedModel } from '../model-picker/useSelectedModel';
import {
  MLX_LM_PROVIDER_ID,
  type MlxServersApi,
} from '../providers/useMlxServers';
import { ResearchProgress } from '../research/ResearchProgress';
import { isMarkdownExportRequest, researchQuestion } from '../research/researchIntent';
import { useResearchRun } from '../research/useResearchRun';

export type ChatPanelProps = {
  selected: SelectedModel | null;
  /** D62: clear lives in the chat model selector now, not in a
   * separate workspace banner. */
  onClearSelection: () => void;
  /** Opens the shell-owned model chooser from an empty chat. */
  onChooseModel?: () => void;
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
  /** Persisted chat that owns this surface's Browser evidence. */
  contextOwner?: ChatContextOwner;
  /** Opens a cited web source inside this chat's Browser workspace. */
  onOpenResearchSource?: (url: string) => void;
};

export function ChatPanel({
  selected,
  onClearSelection,
  onChooseModel,
  inspectorSelection,
  inspectorLineRange,
  projectHasInstructions,
  mlxServers,
  includeProjectContext = true,
  variant = 'workspace',
  chat,
  emphasizedContextKey = null,
  contextOwner,
  onOpenResearchSource,
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
    appendEntries,
    send,
    cancel,
    clear,
  } = chat ?? internalChat;
  const research = useResearchRun(contextOwner ?? null);
  const [draft, setDraft] = useState('');
  const [contextActionError, setContextActionError] = useState<string | null>(null);
  // D15: response-shape mode for the next send. Window-local
  // state; closing the project resets to 'chat'. Mid-stream
  // toggling is allowed but only affects the NEXT send — the
  // in-flight one keeps the mode it was started with.
  const [mode, setMode] = useState<ChatMode>('chat');
  const listRef = useRef<HTMLOListElement | null>(null);

  useEffect(() => {
    if (!includeProjectContext || selected === null) setMode('chat');
  }, [includeProjectContext, selected]);

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
    ...(contextOwner ? { contextOwner } : {}),
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
  const researchActive = ['starting', 'running', 'stopping'].includes(research.status);
  const researchSources = contextSources
    .filter(
      (source): source is Extract<ContextSourceRef, { kind: 'browserTextEvidence' }> =>
        source.kind === 'browserTextEvidence',
    )
    .map((source) => ({ kind: source.kind, evidenceId: source.evidenceId }));
  const researchModelSupported =
    selected?.providerId === 'apple-foundation' && selected.modelId === 'system' ||
    selected?.providerId === MLX_LM_PROVIDER_ID && selected.modelId === QWEN_CATALOG_ID;
  const researchDisabledReason = researchUnavailableReason({
    contextOwner,
    selected,
    researchModelSupported,
    researchSourceCount: researchSources.length,
    researchActive,
    isStreaming,
    mlxHandlePresent,
  });
  const latestResearchEntry = useMemo(
    () => [...entries].reverse().find((entry) => entry.kind === 'researchArtifact') ?? null,
    [entries],
  );
  const canSend =
    disabledReason === null &&
    draft.trim().length > 0 &&
    !isStreaming &&
    !researchActive;

  const appendedResearchRefs = useRef(new Set<string>());
  useEffect(() => {
    if (research.artifact === null || contextOwner === undefined) return;
    const { artifactId, version } = research.artifact.artifact;
    const key = `${contextOwner.scope}:${contextOwner.sessionId}:${artifactId}:${version}`;
    const alreadyVisible = entries.some(
      (entry) => entry.kind === 'researchArtifact' && entry.owner.scope === contextOwner.scope &&
        entry.owner.sessionId === contextOwner.sessionId && entry.artifactId === artifactId &&
        entry.version === version,
    );
    if (alreadyVisible || appendedResearchRefs.current.has(key)) return;
    appendedResearchRefs.current.add(key);
    appendEntries([{ kind: 'researchArtifact', owner: contextOwner, artifactId, version }]);
  }, [appendEntries, contextOwner, entries, research.artifact]);

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
      const text = draft.trim();
      if (text.length === 0 || isStreaming || researchActive) return;
      if (isMarkdownExportRequest(text)) {
        setDraft('');
        const userEntry = { kind: 'message' as const, message: { role: 'user' as const, content: text } };
        if (latestResearchEntry === null) {
          appendEntries([userEntry, { kind: 'error', message: 'Research something before exporting it.' }]);
          return;
        }
        appendEntries([userEntry]);
        void exportResearchArtifact({
          owner: latestResearchEntry.owner,
          artifactId: latestResearchEntry.artifactId,
          version: latestResearchEntry.version,
        }).then((outcome) => {
          if (outcome.status !== 'saved') return;
          appendEntries([{
            kind: 'researchExport',
            owner: latestResearchEntry.owner,
            artifactId: latestResearchEntry.artifactId,
            version: latestResearchEntry.version,
            fileName: outcome.fileName,
          }]);
        }).catch((error: unknown) => {
          appendEntries([{ kind: 'error', message: researchProductError(error) }]);
        });
        return;
      }
      const question = researchQuestion(text);
      if (question !== null) {
        setDraft('');
        const userEntry = { kind: 'message' as const, message: { role: 'user' as const, content: text } };
        if (researchDisabledReason !== null || selected === null) {
          appendEntries([
            userEntry,
            { kind: 'error', message: researchDisabledReason ?? 'Choose a model first.' },
          ]);
          return;
        }
        appendEntries([userEntry]);
        const mlxHandle =
          selected.providerId === MLX_LM_PROVIDER_ID
            ? mlxServers.handleOf(selected.modelId)
            : null;
        void research.start({
          question,
          providerId: selected.providerId,
          modelId: selected.modelId,
          ...(mlxHandle ? { handleId: mlxHandle.id } : {}),
          sources: researchSources,
        });
        return;
      }
      if (!canSend || !selected) return;
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
        ...(includeProjectContext && mode !== 'chat' ? { mode } : {}),
        ...(mlxHandle ? { handleId: mlxHandle.id } : {}),
        ...(includeProjectContext ? {} : { includeProjectContext: false }),
        ...(contextOwner ? { contextOwner } : {}),
      });
    },
    [
      canSend,
      appendEntries,
      draft,
      includeProjectContext,
      isStreaming,
      latestResearchEntry,
      mode,
      mlxServers,
      research,
      researchActive,
      researchDisabledReason,
      researchSources,
      selected,
      send,
    ],
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
  const statusText = chatStatusText(selected, disabledReason, isStreaming);

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
                preview={contextPreview.data?.instructions ?? null}
                previewStatus={contextPreview.status}
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
                    preview={contextPreview.data?.instructions ?? null}
                    previewStatus={contextPreview.status}
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
            {instructionsSubtitleHint(
              projectHasInstructions,
              lastInstructionsIncluded,
              contextPreview.status,
              contextPreview.data?.instructions ?? null,
            )}
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
            <strong>What can I help you with?</strong>
            {selected === null && onChooseModel ? (
              <button
                type="button"
                className="ink-button plume-chat-choose-model"
                onClick={onChooseModel}
              >
                Choose a model
              </button>
            ) : null}
          </li>
        ) : (
          entries.map((entry, i) => (
            <ChatEntryRow
              key={i}
              entry={entry}
              {...(onOpenResearchSource ? { onOpenResearchSource } : {})}
              {...(entry.kind === 'researchExport' ? {
                onOpenResearchExport: () => {
                  void exportResearchArtifact({
                    owner: entry.owner,
                    artifactId: entry.artifactId,
                    version: entry.version,
                  });
                },
              } : {})}
            />
          ))
        )}
        {research.status !== 'idle' && research.artifact === null ? (
          <li className="plume-chat-entry plume-chat-entry-assistant">
            <ResearchProgress
              status={research.status}
              steps={research.steps}
              details={research.details}
              error={research.error}
              onStop={() => void research.stop()}
            />
          </li>
        ) : null}
      </ol>

      <form className="plume-chat-form" onSubmit={submit} aria-controls={transcriptId}>
        {includeProjectContext &&
        inspectorSelection !== null &&
        inspectorSelection.kind !== 'empty' ? (
          <AttachBar
            chip={null}
            candidate={attachCandidate}
            onAttach={onAttach}
            onClear={() => undefined}
            disabled={isStreaming || researchActive}
            placement="chatShelf"
          />
        ) : null}
        <ContextShelf
          sources={contextSources}
          preview={contextPreview.data?.contextSources ?? []}
          loading={contextPreview.status === 'loading'}
          disabled={isStreaming || researchActive}
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
        {includeProjectContext ? (
          <ContextPreview
            instructions={null}
            attachment={null}
            loading={contextPreview.status === 'loading' && contextPreview.data === null}
            error={contextPreview.status === 'error' ? contextPreview.error : null}
          />
        ) : null}
        <label className="plume-chat-input-label">
          <span className="plume-visually-hidden">
            Message to send
          </span>
          <textarea
            className="plume-chat-input"
            value={draft}
            onChange={(e) => setDraft(e.target.value)}
            onKeyDown={onKeyDown}
            placeholder={inputPlaceholder(selected, disabledReason)}
            disabled={isInputDisabled(disabledReason) || researchActive}
            aria-label="Message to send"
            rows={3}
          />
        </label>
        <div className="plume-chat-form-bar">
          {includeProjectContext && selected !== null ? (
            <ModeToggle
              mode={mode}
              onChange={setMode}
              disabled={isStreaming || researchActive}
            />
          ) : null}
          {statusText ? (
            <span className="plume-chat-status" role="status" aria-live="polite">
              {statusText}
            </span>
          ) : (
            <span className="plume-chat-form-spacer" aria-hidden="true" />
          )}
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

function researchUnavailableReason({
  contextOwner,
  selected,
  researchModelSupported,
  researchSourceCount,
  researchActive,
  isStreaming,
  mlxHandlePresent,
}: {
  contextOwner: ChatContextOwner | undefined;
  selected: SelectedModel | null;
  researchModelSupported: boolean;
  researchSourceCount: number;
  researchActive: boolean;
  isStreaming: boolean;
  mlxHandlePresent: boolean;
}): string | null {
  if (researchActive || isStreaming) return 'Wait for the current work to finish.';
  if (contextOwner === undefined) return 'Save this chat before creating a research note.';
  if (!researchModelSupported || selected === null) {
    return 'Choose Apple On-Device or the included Qwen model.';
  }
  if (selected.providerId === MLX_LM_PROVIDER_ID && !mlxHandlePresent) {
    return 'Start the included Qwen model first.';
  }
  if (researchSourceCount === 0) return 'Attach captured page text first.';
  if (researchSourceCount > 10) return 'Remove captured sources until 10 or fewer remain.';
  return null;
}

function researchProductError(error: unknown): string {
  if (isIpcError(error)) return ipcErrorMessage(error);
  if (error instanceof Error) return error.message;
  return 'The research note could not be exported.';
}
