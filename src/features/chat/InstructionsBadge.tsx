// D11: badge rendered next to the read-only badge in the chat
// header. Three states avoid the "claim from metadata alone" trap
// the first iteration of this slice hit:
//
//   * `projectHasInstructions === false` → no badge. The project
//     has no AGENTS.md, end of story.
//   * `projectHasInstructions === true && lastIncluded === null`
//     → "AGENTS.md available". Forward-looking promise based on
//     the static `ProjectMeta.hasAgentsMd` flag; no send has
//     resolved yet so we can't say "included" honestly.
//   * `projectHasInstructions === true && lastIncluded === true`
//     → "AGENTS.md included". Backend confirmed the file was
//     folded into the most recent accepted send.
//   * `projectHasInstructions === true && lastIncluded === false`
//     → "AGENTS.md skipped". Backend reported a skip (file
//     present but unreadable — oversize, binary, hardlink,
//     etc.). Visually distinguished so the user notices and can
//     investigate.
//
// D22 extraction: pulled `InstructionsBadge` and
// `instructionsSubtitleHint` out of `ChatPanel.tsx`.
// D42 addendum: `MemoryBadge` lives here as the sibling chip for
// the same chat-header band.

import type {
  ChatContextInstructionsPreview,
  ChatMemoryUsage,
  ChatTopicsUsage,
} from '../../lib/api/chat';
import { Disclosure } from '../project-shell/Disclosure';
import { Icon } from '../project-shell/Icon';
import { formatBytes } from './formatters';

type InstructionsBadgeProps = {
  projectHasInstructions: boolean;
  lastIncluded: boolean | null;
  preview?: ChatContextInstructionsPreview | null;
};

export function InstructionsBadge({
  projectHasInstructions,
  lastIncluded,
  preview = null,
}: InstructionsBadgeProps) {
  if (!projectHasInstructions) return null;
  const state: 'available' | 'included' | 'skipped' =
    lastIncluded === null ? 'available' : lastIncluded ? 'included' : 'skipped';
  const aria =
    state === 'available'
      ? 'Project instructions available; they will be used on the next send.'
      : state === 'included'
        ? 'Project instructions were used on the most recent send.'
        : 'Project instructions could not be used on the most recent send.';
  const className =
    state === 'skipped'
      ? 'ink-badge plume-summary-chip plume-chat-instructions-badge plume-chat-instructions-badge-skipped'
      : 'ink-badge plume-summary-chip plume-chat-instructions-badge';
  const detail =
    state === 'available'
      ? 'Plume will use these instructions on your next message.'
      : state === 'included'
        ? 'Plume used these instructions on your most recent message.'
        : 'Plume could not read these instructions on your most recent message.';
  return (
    <Disclosure
      className="plume-chat-context-manifest plume-chat-instructions-manifest"
      summary={
        <span className={className} role="status" aria-label={aria}>
          <Icon name="files" size={13} />
          <span>Project instructions</span>
        </span>
      }
    >
      <div className="plume-chat-instructions-details">
        <strong>{preview?.source ?? 'AGENTS.md'}</strong>
        {preview ? (
          <span className="plume-chat-context-manifest-meta">
            {formatBytes(preview.originalBytes)}
            {preview.redactionCount > 0
              ? ` · ${preview.redactionCount} ${preview.redactionCount === 1 ? 'redaction' : 'redactions'}`
              : ''}
          </span>
        ) : null}
        <span>{detail}</span>
      </div>
    </Disclosure>
  );
}

/// Subtitle hint mirrors the badge: "available" before the first
/// send, "included on the last send" once a send has resolved
/// successfully, "skipped on the last send" if the backend
/// reported a skip. Suppressed entirely when the project has no
/// AGENTS.md.
export function instructionsSubtitleHint(
  projectHasInstructions: boolean,
  lastIncluded: boolean | null,
): string {
  if (!projectHasInstructions) return '';
  if (lastIncluded === null) {
    return 'Project instructions will be used on your next message. ';
  }
  if (lastIncluded === true) {
    return 'Project instructions were used on the last message. ';
  }
  return 'Project instructions could not be used on the last message. Open Details to inspect them. ';
}

// D42: project-memory badge. Two states (skipped is implicit —
// memory failures surface as `null` so we just hide the chip):
//
//   * `preview === null && lastUsed === null` → no badge.
//   * `preview !== null` → a "Next send" manifest backed by the
//     latest chat.context preview.
//   * `lastUsed !== null` → a separate historical "Last send"
//     manifest backed by the most recent chat.send response.
//
// We deliberately do NOT render a "skipped" memory state. A
// missing or unreadable memory store is the expected idle case
// (most projects never call `memory.remember`), so a chip that
// said "Memory skipped" on every project would be noise. If the
// store IS present but unreadable, the Memory panel surfaces the
// store error; the chat header stays quiet.

type MemoryBadgeProps = {
  /** Forward-looking preview from the most recent `chat.context`. */
  preview: ChatMemoryUsage | null;
  /** Confirmed usage from the most recent accepted `chat.send`. */
  lastUsed: ChatMemoryUsage | null;
};

export function MemoryBadge({ preview, lastUsed }: MemoryBadgeProps) {
  const usage = lastUsed ?? preview;
  if (!usage) return null;
  const state: 'available' | 'included' = lastUsed ? 'included' : 'available';
  const label = `Memory · ${usage.entryCount} ${pluralEntry(usage.entryCount)}`;
  const aria =
    state === 'available'
      ? `Project memory available: ${usage.entryCount} ${pluralEntry(usage.entryCount)} will be used on the next send.`
      : `Project memory used on the last send: ${usage.entryCount} ${pluralEntry(usage.entryCount)}${usage.truncated ? ', with older entries omitted' : ''}.`;
  return (
    <Disclosure
      className="plume-chat-context-manifest"
      summary={
        <span className="ink-badge plume-summary-chip plume-chat-memory-badge">
          <Icon name="knowledge" size={13} />
          <span role="status" aria-label={aria}>{label}</span>
        </span>
      }
    >
      <div className="plume-chat-context-manifest-popover">
        {lastUsed ? <MemoryManifestSection label="Last send" usage={lastUsed} /> : null}
        {preview ? <MemoryManifestSection label="Next send" usage={preview} /> : null}
      </div>
    </Disclosure>
  );
}

function MemoryManifestSection({ label, usage }: { label: string; usage: ChatMemoryUsage }) {
  return (
    <section className="plume-chat-context-manifest-section">
      <strong>{label}</strong>
      <ul className="plume-chat-context-manifest-list">
        {usage.entries.map((entry) => (
          <li key={entry.id}>
            <span className="plume-chat-context-manifest-preview">{entry.preview}</span>
            <span className="plume-chat-context-manifest-meta">
              …{entry.id.slice(-4)} · {entry.textBytes} B
            </span>
          </li>
        ))}
      </ul>
    </section>
  );
}

function pluralEntry(n: number): string {
  return n === 1 ? 'entry' : 'entries';
}

// D73: curated topic-file badge. Sibling chip to MemoryBadge, same
// two-state posture (skipped is implicit — failures surface as `null`
// so the chip just hides):
//
//   * `preview === null && lastUsed === null` → no badge.
//   * `preview !== null` → a forward-looking "Next send" manifest.
//   * `lastUsed !== null` → a separate confirmed "Last send"
//     manifest, without hiding a refreshed preview.

type TopicsBadgeProps = {
  preview: ChatTopicsUsage | null;
  lastUsed: ChatTopicsUsage | null;
};

export function TopicsBadge({ preview, lastUsed }: TopicsBadgeProps) {
  const usage = lastUsed ?? preview;
  if (!usage) return null;
  const state: 'available' | 'included' = lastUsed ? 'included' : 'available';
  const label = `Topics · ${usage.fileCount} ${pluralFile(usage.fileCount)}`;
  const aria =
    state === 'available'
      ? `Curated topic files available: ${usage.fileCount} ${pluralFile(usage.fileCount)} — will ride along on the next send.`
      : `Curated topic files used on the last send: ${usage.fileCount} ${pluralFile(usage.fileCount)}${usage.truncated ? ', trimmed to fit' : ''}.`;
  return (
    <Disclosure
      className="plume-chat-context-manifest"
      summary={
        <span className="ink-badge plume-summary-chip plume-chat-topics-badge">
          <Icon name="library" size={13} />
          <span role="status" aria-label={aria}>{label}</span>
        </span>
      }
    >
      <div className="plume-chat-context-manifest-popover">
        {lastUsed ? <TopicsManifestSection label="Last send" usage={lastUsed} /> : null}
        {preview ? <TopicsManifestSection label="Next send" usage={preview} /> : null}
      </div>
    </Disclosure>
  );
}

function TopicsManifestSection({ label, usage }: { label: string; usage: ChatTopicsUsage }) {
  return (
    <section className="plume-chat-context-manifest-section">
      <strong>{label}</strong>
      <ul className="plume-chat-context-manifest-list">
        {usage.files.map((file) => (
          <li key={file.name}>
            <span className="plume-chat-context-manifest-preview">{file.name}</span>
            <span className="plume-chat-context-manifest-meta">{file.bytes} B</span>
          </li>
        ))}
      </ul>
    </section>
  );
}

function pluralFile(n: number): string {
  return n === 1 ? 'file' : 'files';
}
