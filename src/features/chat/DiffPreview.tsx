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
//
// D31: wired the Apply button to `patch.apply`. The validation
// pill is now the single render surface for BOTH validation AND
// apply state on a given diff. Success surfaces the checkpoint
// id and the touched-file count; failure replaces the pill
// content with a typed apply-error pill. The button itself is
// enabled only when validation is `valid` and apply is `idle`;
// it disables for the in-flight call and flips to `Applied`
// (terminal) on success. Revert verb / UI is reserved for D32.

import { useCallback, useEffect, useMemo, useState } from 'react';

import { ipcErrorMessage, isIpcError } from '../../lib/api/errors';
import type {
  PatchAppliedFile,
  PatchApplyFailure,
  PatchFailureDetail,
  PatchTouch,
  PatchValidationError,
} from '../../lib/api/patch';
import { applyPatch, validatePatch } from '../../lib/api/patch';

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
  const apply = useDiffApply(replyText);

  // Apply is gated on validation passing AND apply not being in
  // flight / already succeeded. Apply error is recoverable — the
  // user can re-prompt the model and try a new diff; we don't
  // lock the button after a failure.
  const applyButtonState = applyButtonStateFor(validation, apply);

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
      <DiffStatusPill validation={validation} apply={apply} />
      <div className="plume-chat-diff-actions">
        <button
          type="button"
          className="ink-button plume-chat-diff-apply"
          disabled={applyButtonState.disabled}
          onClick={applyButtonState.disabled ? undefined : apply.run}
          aria-label={applyButtonState.ariaLabel}
          title={applyButtonState.title}
        >
          {applyButtonState.label}
        </button>
        <span className="plume-chat-diff-actions-note" role="status">
          {applyButtonState.note}
        </span>
      </div>
    </div>
  );
}

// ─── Validation hook (D16, unchanged shape) ────────────────────────────────

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

// ─── D31: apply hook ───────────────────────────────────────────────────────

/// State machine for the Apply button + post-apply pill content:
///
///   idle ── click ──▶ applying ── ok ──▶ applied (terminal)
///                                 │
///                                 └── err ──▶ failed
///                                 │
///                                 └── ipcErr ──▶ ipcFailed
///
/// `failed` and `ipcFailed` allow another click (the user might
/// have fixed the underlying state — restarted the daemon,
/// resolved a drift via re-prompt, etc.). `applied` is terminal:
/// once a checkpoint exists, re-applying the same diff would
/// fail with `preImageMismatch` anyway, and the UI keeps the
/// checkpoint id visible for D32's Revert button.
type DiffApplyState =
  | { state: 'idle' }
  | { state: 'applying' }
  | { state: 'applied'; checkpoint: string; touched: PatchAppliedFile[] }
  | { state: 'failed'; reason: PatchApplyFailure; details: PatchFailureDetail[] }
  | { state: 'ipcFailed'; message: string };

type DiffApplyHandle = DiffApplyState & {
  run: () => void;
};

function useDiffApply(replyText: string): DiffApplyHandle {
  const [state, setState] = useState<DiffApplyState>({ state: 'idle' });

  const run = useCallback(() => {
    // Guard against double-click while applying or terminal.
    setState((prev) => {
      if (prev.state === 'applying' || prev.state === 'applied') return prev;
      return { state: 'applying' };
    });
    applyPatch({ diff: replyText })
      .then((resp) => {
        if (resp.applied) {
          setState({
            state: 'applied',
            checkpoint: resp.checkpoint,
            touched: resp.touched,
          });
        } else {
          setState({
            state: 'failed',
            reason: resp.reason,
            details: resp.details,
          });
        }
      })
      .catch((err: unknown) => {
        const message = isIpcError(err) ? ipcErrorMessage(err) : 'apply failed';
        setState({ state: 'ipcFailed', message });
      });
  }, [replyText]);

  return { ...state, run };
}

// ─── Button + pill rendering ───────────────────────────────────────────────

type ApplyButtonState = {
  label: string;
  ariaLabel: string;
  title: string;
  note: string;
  disabled: boolean;
};

function applyButtonStateFor(
  validation: DiffValidationState,
  apply: DiffApplyHandle,
): ApplyButtonState {
  if (apply.state === 'applied') {
    return {
      label: 'Applied',
      ariaLabel: 'Patch applied (Revert will land in D32)',
      title: `Applied. Checkpoint ${apply.checkpoint.slice(0, 8)}… saved; Revert is roadmap.`,
      note: 'written to disk',
      disabled: true,
    };
  }
  if (apply.state === 'applying') {
    return {
      label: 'Applying…',
      ariaLabel: 'Applying patch',
      title: 'Writing files…',
      note: 'writing…',
      disabled: true,
    };
  }
  // Idle / failed / ipcFailed — the button is clickable iff
  // validation has confirmed the diff is valid.
  if (validation.state === 'valid') {
    return {
      label: 'Apply',
      ariaLabel: 'Apply this diff to the project',
      title:
        'Validate the diff again server-side and write each file atomically. A checkpoint is taken before any write.',
      note:
        apply.state === 'failed' || apply.state === 'ipcFailed'
          ? 'try again — the last attempt failed'
          : 'writes files; checkpoint kept for D32 revert',
      disabled: false,
    };
  }
  return {
    label: 'Apply',
    ariaLabel: 'Apply this diff (disabled — validation has not passed)',
    title:
      validation.state === 'loading'
        ? 'Waiting for validation…'
        : validation.state === 'invalid'
          ? 'Validation rejected this diff; Apply is disabled.'
          : "Validation couldn't run; Apply is disabled until it succeeds.",
    note: 'preview only — validation has not passed',
    disabled: true,
  };
}

function DiffStatusPill({
  validation,
  apply,
}: {
  validation: DiffValidationState;
  apply: DiffApplyHandle;
}) {
  // Apply state shadows validation once it starts. Render priority:
  //   1. Apply applied / applying / failed → apply pill
  //   2. Validation pill otherwise
  if (apply.state === 'applied') {
    const fileWord = apply.touched.length === 1 ? 'file' : 'files';
    const short = apply.checkpoint.slice(0, 8);
    return (
      <p
        className="plume-chat-diff-validation plume-chat-diff-validation-valid"
        role="status"
        aria-live="polite"
        title={`Checkpoint ${apply.checkpoint}`}
      >
        applied · {apply.touched.length} {fileWord} · checkpoint {short}…
      </p>
    );
  }
  if (apply.state === 'applying') {
    return (
      <p
        className="plume-chat-diff-validation plume-chat-diff-validation-loading"
        role="status"
        aria-live="polite"
      >
        applying patch…
      </p>
    );
  }
  if (apply.state === 'failed') {
    const headline = apply.details[0]?.message ?? 'unknown failure';
    return (
      <p
        className="plume-chat-diff-validation plume-chat-diff-validation-invalid"
        role="status"
        aria-live="polite"
        title={apply.details.map((d) => `${d.path}: ${d.message}`).join('\n')}
      >
        apply failed ({applyReasonLabel(apply.reason)}): {headline}
      </p>
    );
  }
  if (apply.state === 'ipcFailed') {
    return (
      <p
        className="plume-chat-diff-validation plume-chat-diff-validation-failed"
        role="status"
        aria-live="polite"
      >
        apply unavailable: {apply.message}
      </p>
    );
  }
  // Fall through to validation pill.
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

function applyReasonLabel(reason: PatchApplyFailure): string {
  switch (reason) {
    case 'validationFailed':
      return 'validation';
    case 'preImageMismatch':
      return 'pre-image drift';
    case 'checkpointFailed':
      return 'checkpoint';
    case 'writeFailed':
      return 'write';
    case 'scopeUnsupported':
      return 'scope';
    default:
      return 'unknown';
  }
}
