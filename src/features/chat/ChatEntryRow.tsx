// Per-row renderer for the chat transcript.
//
// D22 extraction: lifted out of `ChatPanel.tsx`. Owns the
// streaming / cancelled / error / completed branches and the
// per-turn metadata (attachment chip, model+duration footer,
// stats footer, propose-diff fallback hint).

import { CopyReplyButton } from './CopyReplyButton';
import { DiffPreview, extractDiffBlock } from './DiffPreview';
import { formatDuration, formatStatsLine, formatStatsTitle } from './formatters';
import type { ChatEntry } from './useChat';

export function ChatEntryRow({ entry }: { entry: ChatEntry }) {
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
