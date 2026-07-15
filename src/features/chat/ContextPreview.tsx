// D12: small "what will ride along" preview area between the
// attach bar and the textarea. Renders one row per piece of
// context (AGENTS.md when present, the attached file/selection
// when set) with concrete byte sizes + redaction counts so the
// user sees what will actually be sent before they press Send.
//
// Honest about empties: when neither AGENTS.md nor an attachment
// is in play, the area collapses entirely. We deliberately do
// NOT render an "empty preview" placeholder — it would just be
// noise. The user can still see they have no context to send
// because the chip bar above is empty and the badge is the
// authority on AGENTS.md state.
//
// Honest about blocked attachments: the chip bar shows the user
// what they tried to attach; this preview shows what the BACKEND
// would do with it. A "blocked" status here surfaces the typed
// reason from the prompt-read pipeline (secret filename, oversize,
// path escape, needs approval, …) so the user knows the chip is
// effectively a no-op for that send. Visually distinguished with
// a warn variant so it's not confused with a ready item.
//
// D22 extraction: pulled `ContextPreview`,
// `InstructionsPreviewItem`, `AttachmentPreviewItem`,
// `blockedReasonLabel`, `formatAttachmentLabel` out of
// `ChatPanel.tsx`.

import type {
  ChatContextAttachmentPreview,
  ChatContextBlockReason,
  ChatContextInstructionsPreview,
} from '../../lib/api/chat';
import { formatBytes } from './formatters';
import { InstructionsBadge } from './InstructionsBadge';
import { Disclosure } from '../project-shell/Disclosure';
import { Icon } from '../project-shell/Icon';

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

export function ContextPreview({
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
  return (
    <InstructionsBadge
      projectHasInstructions
      lastIncluded={null}
      preview={instructions}
      previewStatus="ready"
    />
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
    const humanLabel = readableAttachmentLabel(
      attachment.relPath,
      attachment.startLine,
      attachment.endLine,
    );
    return (
      <Disclosure
        className="plume-chat-context-preview-item plume-chat-context-item-details"
        summary={
          <span className="ink-badge plume-summary-chip plume-chat-context-preview-summary" role="status">
            <Icon name="files" size={13} />
            <span>{humanLabel}</span>
          </span>
        }
      >
        <span className="plume-chat-context-preview-name">{label}</span>
        <span className="plume-chat-context-preview-meta">
          {sizeLabel}{redactionLabel}
        </span>
      </Disclosure>
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

function readableAttachmentLabel(
  relPath: string,
  startLine: number | null,
  endLine: number | null,
): string {
  const name = relPath.split('/').filter(Boolean).at(-1) ?? relPath;
  if (startLine === null || endLine === null) return name;
  if (startLine === endLine) return `${name} · line ${startLine}`;
  return `${name} · lines ${startLine}–${endLine}`;
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
