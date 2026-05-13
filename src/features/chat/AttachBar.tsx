// Read-only file-context attachment bar above the textarea.
//
// D22 extraction: pulled `AttachBar`, `chipMatchesSelection`,
// `describeAttachCandidate`, `formatChipPath`, `attachButtonLabel`,
// `attachButtonTitle`, `attachHintText` out of `ChatPanel.tsx`.
// The `ChipState` and `AttachCandidate` types live here too so
// the panel imports the shapes it sets and reads.

import { PROMPT_READ_MAX_BYTES } from '../../lib/api/chat';
import type { EditorLineRange } from '../editor/ReadOnlyEditor';
import type { SelectionState } from '../file-tree/FileBrowser';
import { formatBytes } from './formatters';

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
export type ChipState = {
  relPath: string;
  /** Bytes on disk at the moment of attach. Surface-only — the
   * backend re-reads on send so the live count can differ. */
  bytes: number;
  lineRange: EditorLineRange | null;
};

export type AttachCandidate =
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

export function describeAttachCandidate(
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

export function AttachBar({ chip, candidate, onAttach, onClear, disabled }: AttachBarProps) {
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
