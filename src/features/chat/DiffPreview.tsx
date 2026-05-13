// D15: extract the unified diff body from an assistant reply.
// Looks for a single fenced ```diff or ```patch code block; if
// found, returns the inner content. Otherwise returns null and
// the caller renders the raw text with a "no diff detected"
// hint. We deliberately don't try to parse raw diffs without a
// fence — that boundary keeps the parser simple, and the system
// message instructs the model to use a fence anyway.
//
// D22 extraction: pulled `extractDiffBlock`, `DiffPreview`,
// `useDiffValidation`, and `DiffValidationPill` out of
// `ChatPanel.tsx` so the diff path can evolve without churning
// the surrounding panel.

import { useEffect, useMemo, useState } from 'react';

import { ipcErrorMessage, isIpcError } from '../../lib/api/errors';
import type { PatchTouch, PatchValidationError } from '../../lib/api/patch';
import { validatePatch } from '../../lib/api/patch';

/// The regex is intentionally lenient: any case for the language
/// tag, an optional language tag at all (so a bare ``` followed
/// by what looks like a diff still works if the model forgot the
/// `diff` tag but otherwise complied), and trailing whitespace
/// inside the fence is preserved.
export function extractDiffBlock(reply: string): string | null {
  // Try the explicit `diff` / `patch` tagged fence first.
  const tagged = /```(?:diff|patch)\s*\n([\s\S]*?)```/i.exec(reply);
  if (tagged && tagged[1]) {
    return tagged[1].replace(/\n$/, '');
  }
  // Fallback: any fenced block whose first line looks like a
  // unified-diff header (`--- ` followed by `+++ ` on the next).
  // This catches models that drop the language tag but still
  // produce a valid diff inside a fence.
  const untagged = /```(?:[a-zA-Z]*)\s*\n(--- [^\n]+\n\+\+\+ [^\n]+\n[\s\S]*?)```/i.exec(reply);
  if (untagged && untagged[1]) {
    return untagged[1].replace(/\n$/, '');
  }
  return null;
}

/// D15: render a unified diff with per-line coloring. Each line
/// is classified by its first character:
///   `+` — addition
///   `-` — deletion
///   `@` — hunk header (`@@ -1,4 +1,5 @@`)
///   `-` or `+` followed by `--` / `++` is a file header (the
///       regex above already routes those through; we treat them
///       as headers, not as add/remove)
///   anything else — context
///
/// The renderer is intentionally simple: it does NOT validate the
/// diff applies cleanly, does NOT match hunks against any file,
/// does NOT highlight syntax inside the changed lines. It just
/// gives the user a readable visual.
///
/// The "Apply" button is rendered **disabled** with a tooltip
/// naming the boundary. Plume does not apply patches in D15. The
/// existing Copy button on the parent assistant entry already
/// covers "grab the diff and apply by hand."
type DiffLineKind = 'add' | 'del' | 'hunk' | 'header' | 'context';

function classifyDiffLine(line: string): DiffLineKind {
  if (line.startsWith('+++') || line.startsWith('---')) return 'header';
  if (line.startsWith('@@')) return 'hunk';
  if (line.startsWith('+')) return 'add';
  if (line.startsWith('-')) return 'del';
  return 'context';
}

export function DiffPreview({ diff, replyText }: { diff: string; replyText: string }) {
  const lines = useMemo(() => diff.split('\n'), [diff]);
  const validation = useDiffValidation(replyText);
  return (
    <div className="plume-chat-diff" role="group" aria-label="Proposed diff preview">
      <pre className="plume-chat-diff-body">
        {lines.map((line, i) => {
          const kind = classifyDiffLine(line);
          return (
            <span
              key={i}
              className={`plume-chat-diff-line plume-chat-diff-line-${kind}`}
              role={kind === 'add' || kind === 'del' ? 'text' : undefined}
              aria-label={
                kind === 'add'
                  ? `Added: ${line.slice(1)}`
                  : kind === 'del'
                    ? `Removed: ${line.slice(1)}`
                    : undefined
              }
            >
              {line}
              {'\n'}
            </span>
          );
        })}
      </pre>
      <DiffValidationPill validation={validation} />
      <div className="plume-chat-diff-actions">
        <button
          type="button"
          className="ink-button plume-chat-diff-apply"
          disabled
          aria-label={
            validation.state === 'valid'
              ? 'Apply this diff (disabled — validation passed but apply is future)'
              : 'Apply this diff (disabled — preview only)'
          }
          title={
            validation.state === 'valid'
              ? 'Validation passed, but Plume does not apply patches yet. Use the Copy button on the assistant turn to grab this diff and apply it by hand.'
              : "Plume can't apply diffs yet — preview only. Use the Copy button on the assistant turn to grab this diff and apply it by hand."
          }
        >
          Apply
        </button>
        <span className="plume-chat-diff-actions-note" role="status">
          preview only — no writes
        </span>
      </div>
    </div>
  );
}

/// D16: thin hook that runs `patch.validate` once per finalized
/// propose-diff reply and exposes a small `'loading' | 'valid' |
/// 'invalid' | 'failed'` state for the pill.
///
/// `replyText` is the full assistant reply (including the fenced
/// markers) so the backend sees what the user would copy. The hook
/// fires once on mount; subsequent re-renders are no-ops because
/// the reply text on a finalized message entry never changes. The
/// `Internal` / `NeedsApproval` paths from the IPC layer surface
/// as `'failed'` with the human message — the UI shouldn't
/// disappear or block the diff renderer just because validation
/// couldn't complete.
type DiffValidationState =
  | { state: 'loading' }
  | { state: 'valid'; touches: PatchTouch[]; hunks: number }
  | { state: 'invalid'; errors: PatchValidationError[] }
  | { state: 'failed'; message: string };

function useDiffValidation(replyText: string): DiffValidationState {
  const [validation, setValidation] = useState<DiffValidationState>({ state: 'loading' });
  useEffect(() => {
    let cancelled = false;
    setValidation({ state: 'loading' });
    validatePatch({ diff: replyText })
      .then((resp) => {
        if (cancelled) return;
        if (resp.ok) {
          setValidation({ state: 'valid', touches: resp.touches, hunks: resp.hunks });
        } else {
          setValidation({ state: 'invalid', errors: resp.errors });
        }
      })
      .catch((err: unknown) => {
        if (cancelled) return;
        const message = isIpcError(err) ? ipcErrorMessage(err) : 'validation failed';
        setValidation({ state: 'failed', message });
      });
    return () => {
      cancelled = true;
    };
  }, [replyText]);
  return validation;
}

function DiffValidationPill({ validation }: { validation: DiffValidationState }) {
  if (validation.state === 'loading') {
    return (
      <p
        className="plume-chat-diff-validation plume-chat-diff-validation-loading"
        role="status"
        aria-live="polite"
      >
        validating diff…
      </p>
    );
  }
  if (validation.state === 'valid') {
    const fileWord = validation.touches.length === 1 ? 'file' : 'files';
    const hunkWord = validation.hunks === 1 ? 'hunk' : 'hunks';
    return (
      <p
        className="plume-chat-diff-validation plume-chat-diff-validation-valid"
        role="status"
        aria-live="polite"
      >
        valid diff · {validation.touches.length} {fileWord} · {validation.hunks} {hunkWord}
      </p>
    );
  }
  if (validation.state === 'invalid') {
    const headline = validation.errors[0];
    return (
      <p
        className="plume-chat-diff-validation plume-chat-diff-validation-invalid"
        role="status"
        aria-live="polite"
        title={validation.errors.map((e) => e.message).join('\n')}
      >
        invalid diff: {headline.message}
      </p>
    );
  }
  return (
    <p
      className="plume-chat-diff-validation plume-chat-diff-validation-failed"
      role="status"
      aria-live="polite"
    >
      validation unavailable: {validation.message}
    </p>
  );
}
