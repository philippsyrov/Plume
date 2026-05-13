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
