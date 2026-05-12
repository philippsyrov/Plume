// D7.1: streaming read-only chat surface for the agent workspace.
//
// "Read-only" still means: this surface does not touch disk, does
// not run commands, does not apply patches. It is a streamed text
// round-trip to the selected local model.
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
//   * Window-local transcript. Closing the project drops it.
//   * No file context, no attachments.
//
// The component takes the currently selected model as a prop so
// disabled state is computed once at the caller (App.tsx hoists
// D6's `useSelectedModel`).

import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type FormEvent,
  type KeyboardEvent,
} from 'react';

import { type ChatEntry, useChat } from './useChat';
import type { SelectedModel } from '../model-picker/useSelectedModel';

const SUPPORTED_PROVIDER_ID = 'ollama';

export type ChatPanelProps = {
  selected: SelectedModel | null;
};

export function ChatPanel({ selected }: ChatPanelProps) {
  const { entries, status, activeStreamId, send, cancel, clear } = useChat();
  const [draft, setDraft] = useState('');
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

  const disabledReason = computeDisabledReason(selected, status);
  const isStreaming = status === 'streaming';
  const canSend = disabledReason === null && draft.trim().length > 0 && !isStreaming;

  const submit = useCallback(
    (e?: FormEvent) => {
      if (e) e.preventDefault();
      if (!canSend || !selected) return;
      const text = draft;
      setDraft('');
      void send(selected.providerId, selected.modelId, text);
    },
    [canSend, draft, selected, send],
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
        Plume streams tokens from the selected model. No file access, no
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
  const { message, modelUsed, durationMs } = entry;
  const isAssistant = message.role === 'assistant';
  return (
    <li
      className={`plume-chat-entry plume-chat-entry-${message.role}`}
      aria-label={`${message.role} message`}
    >
      <span className="plume-chat-entry-role">{message.role}</span>
      <p className="plume-chat-entry-content">{message.content}</p>
      {isAssistant && (modelUsed || typeof durationMs === 'number') ? (
        <p className="plume-chat-entry-meta">
          {modelUsed ? <span>served by {modelUsed}</span> : null}
          {typeof durationMs === 'number' ? <span>· {formatDuration(durationMs)}</span> : null}
        </p>
      ) : null}
    </li>
  );
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
