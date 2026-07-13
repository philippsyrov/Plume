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

import type { ChatMemoryUsage, ChatTopicsUsage } from '../../lib/api/chat';

type InstructionsBadgeProps = {
  projectHasInstructions: boolean;
  lastIncluded: boolean | null;
};

export function InstructionsBadge({
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
export function instructionsSubtitleHint(
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
  const truncMarker = usage.truncated ? '⚠ ' : '';
  const label =
    state === 'available'
      ? `✱ Memory · ${usage.entryCount} ${pluralEntry(usage.entryCount)}`
      : `✱ Memory · ${truncMarker}${usage.entryCount} ${pluralEntry(usage.entryCount)} · ${usage.bytes} B`;
  const aria =
    state === 'available'
      ? `Project memory available: ${usage.entryCount} ${pluralEntry(usage.entryCount)}, ${usage.bytes} bytes — will ride along on the next send.`
      : `Project memory included on the last send: ${usage.entryCount} ${pluralEntry(usage.entryCount)}, ${usage.bytes} bytes${usage.truncated ? ', some older entries dropped to fit the cap' : ''}.`;
  const tooltip =
    state === 'available'
      ? `${usage.entryCount} memory ${pluralEntry(usage.entryCount)} (${usage.bytes} of ${usage.byteCap} byte cap) will fold in as system context on your next send.`
      : `Backend confirmed ${usage.entryCount} memory ${pluralEntry(usage.entryCount)} (${usage.bytes} of ${usage.byteCap} byte cap) were folded in as system context on the last send.${usage.truncated ? ' Older entries were dropped to stay within the cap.' : ''}`;
  return (
    <details className="plume-chat-context-manifest">
      <summary className="ink-badge plume-chat-memory-badge" title={tooltip}>
        <span role="status" aria-label={aria}>{label}</span>
      </summary>
      <div className="plume-chat-context-manifest-popover">
        {lastUsed ? <MemoryManifestSection label="Last send" usage={lastUsed} /> : null}
        {preview ? <MemoryManifestSection label="Next send" usage={preview} /> : null}
      </div>
    </details>
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
  const truncMarker = usage.truncated ? '⚠ ' : '';
  const label =
    state === 'available'
      ? `✱ Topics · ${usage.fileCount} ${pluralFile(usage.fileCount)}`
      : `✱ Topics · ${truncMarker}${usage.fileCount} ${pluralFile(usage.fileCount)} · ${usage.bytes} B`;
  const aria =
    state === 'available'
      ? `Curated topic files available: ${usage.fileCount} ${pluralFile(usage.fileCount)} — will ride along on the next send.`
      : `Curated topic files included on the last send: ${usage.fileCount} ${pluralFile(usage.fileCount)}, ${usage.bytes} bytes${usage.truncated ? ', trimmed to fit the cap' : ''}.`;
  const tooltip =
    state === 'available'
      ? `${usage.fileCount} curated topic ${pluralFile(usage.fileCount)} (INDEX/USER/SOUL, ${usage.bytes} of ${usage.byteCap} byte cap) will fold in as system context on your next send.`
      : `Backend confirmed ${usage.fileCount} curated topic ${pluralFile(usage.fileCount)} (${usage.bytes} of ${usage.byteCap} byte cap) were folded in as system context on the last send.${usage.truncated ? ' A file was trimmed to stay within the cap.' : ''}`;
  return (
    <details className="plume-chat-context-manifest">
      <summary className="ink-badge plume-chat-topics-badge" title={tooltip}>
        <span role="status" aria-label={aria}>{label}</span>
      </summary>
      <div className="plume-chat-context-manifest-popover">
        {lastUsed ? <TopicsManifestSection label="Last send" usage={lastUsed} /> : null}
        {preview ? <TopicsManifestSection label="Next send" usage={preview} /> : null}
      </div>
    </details>
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
