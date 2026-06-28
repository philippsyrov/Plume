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
// (terminal) on success.
//
// D33: wired a Revert button. Shows only after a successful
// apply. On click, calls `patch.revert({ checkpoint })`. The pill
// shadows the apply state with the revert state: `reverting…` →
// `reverted · <N> file(s)` on success, or `revert failed
// (<reason>)` on a drift / write / unsupported-checkpoint
// failure. The Apply button stays `Applied` (terminal) regardless
// — re-applying the same diff would hit a pre-image mismatch
// anyway, and surfacing both Apply-terminal and Revert-state is
// clearer than oscillating one button. Revert is a separate
// terminal state for the turn; the user can't re-revert the same
// checkpoint (idempotency is intentionally not supported — a
// second revert hits drift or unknownCheckpoint, see the design).

import { useCallback, useEffect, useState } from 'react';

import { ipcErrorMessage, isIpcError } from '../../lib/api/errors';
import type {
  PatchAppliedFile,
  PatchApplyFailure,
  PatchFailureDetail,
  PatchRestoredFile,
  PatchRevertFailure,
  PatchTouch,
  PatchValidationError,
} from '../../lib/api/patch';
import { applyPatch, revertPatch, validatePatch } from '../../lib/api/patch';
import { DiffBody } from '../diff/DiffBody';

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

/// D15: render a unified diff with per-line coloring. The diff body
/// renderer (`DiffBody`, D101) is shared with the single-step agent
/// panel; this component layers the validate/apply/revert lifecycle
/// (the pill + action buttons) on top of it.
export function DiffPreview({ diff, replyText }: { diff: string; replyText: string }) {
  const validation = useDiffValidation(replyText);
  const apply = useDiffApply(replyText);
  // D33: Revert is bound to the checkpoint id from a successful
  // apply. The hook accepts `null` and short-circuits its `run`
  // — that way the same hook lives in the render tree from the
  // start and React doesn't conditionally mount it once apply
  // succeeds (which would lose React's hook-order invariant).
  const checkpoint = apply.state === 'applied' ? apply.checkpoint : null;
  const revert = useDiffRevert(checkpoint);

  // Apply is gated on validation passing AND apply not being in
  // flight / already succeeded. Apply error is recoverable — the
  // user can re-prompt the model and try a new diff; we don't
  // lock the button after a failure.
  const applyButtonState = applyButtonStateFor(validation, apply);
  const revertButtonState = revertButtonStateFor(apply, revert);

  return (
    <div className="plume-chat-diff" role="group" aria-label="Proposed diff preview">
      <DiffBody diff={diff} />
      <DiffStatusPill validation={validation} apply={apply} revert={revert} />
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
        {revertButtonState ? (
          <button
            type="button"
            className="ink-button plume-chat-diff-revert"
            disabled={revertButtonState.disabled}
            onClick={revertButtonState.disabled ? undefined : revert.run}
            aria-label={revertButtonState.ariaLabel}
            title={revertButtonState.title}
          >
            {revertButtonState.label}
          </button>
        ) : null}
        <span className="plume-chat-diff-actions-note" role="status">
          {revertButtonState?.note ?? applyButtonState.note}
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
/// checkpoint id visible for a future Revert button.
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

// ─── D33: revert hook ──────────────────────────────────────────────────────

/// State machine for the Revert button + post-revert pill content.
/// Mirrors `useDiffApply`'s shape so the rendering logic can
/// share a single render surface (the validation pill).
///
///   idle ── click ──▶ reverting ── ok ──▶ reverted (terminal)
///                                  │
///                                  └── err ──▶ failed
///                                  │
///                                  └── ipcErr ──▶ ipcFailed
///
/// `failed` and `ipcFailed` are recoverable in the same sense as
/// apply's: the user can fix the underlying state (re-validate
/// the disk content, restart the daemon, etc.) and click again.
/// `reverted` is terminal — a second revert of the same
/// checkpoint hits drift or unknownCheckpoint anyway, so the
/// button stays disabled.
type DiffRevertState =
  | { state: 'idle' }
  | { state: 'reverting' }
  | { state: 'reverted'; restored: PatchRestoredFile[] }
  | { state: 'failed'; reason: PatchRevertFailure; details: PatchFailureDetail[] }
  | { state: 'ipcFailed'; message: string };

type DiffRevertHandle = DiffRevertState & {
  run: () => void;
};

function useDiffRevert(checkpoint: string | null): DiffRevertHandle {
  const [state, setState] = useState<DiffRevertState>({ state: 'idle' });

  const run = useCallback(() => {
    if (!checkpoint) return;
    setState((prev) => {
      if (prev.state === 'reverting' || prev.state === 'reverted') return prev;
      return { state: 'reverting' };
    });
    revertPatch({ checkpoint })
      .then((resp) => {
        if (resp.reverted) {
          setState({ state: 'reverted', restored: resp.restored });
        } else {
          setState({
            state: 'failed',
            reason: resp.reason,
            details: resp.details,
          });
        }
      })
      .catch((err: unknown) => {
        const message = isIpcError(err) ? ipcErrorMessage(err) : 'revert failed';
        setState({ state: 'ipcFailed', message });
      });
  }, [checkpoint]);

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
      ariaLabel: 'Patch applied; the Revert button to the right undoes it.',
      title: `Applied. Checkpoint ${apply.checkpoint.slice(0, 8)}… saved; click Revert to undo.`,
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
          : 'writes files; Revert button appears on success',
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

/// D33: render state for the Revert button. Returns `null` when
/// the button shouldn't render at all — pre-apply or on an apply
/// failure. Once apply succeeds we have a checkpoint id and the
/// button takes over the action slot. Mirrors `ApplyButtonState`'s
/// shape so the render branch stays symmetric.
function revertButtonStateFor(
  apply: DiffApplyHandle,
  revert: DiffRevertHandle,
): ApplyButtonState | null {
  if (apply.state !== 'applied') return null;
  if (revert.state === 'reverted') {
    const word = revert.restored.length === 1 ? 'file' : 'files';
    return {
      label: 'Reverted',
      ariaLabel: 'Patch reverted',
      title: `Reverted ${revert.restored.length} ${word} to their pre-apply state.`,
      note: 'restored',
      disabled: true,
    };
  }
  if (revert.state === 'reverting') {
    return {
      label: 'Reverting…',
      ariaLabel: 'Reverting patch',
      title: 'Restoring files from the pre-apply checkpoint…',
      note: 'reverting…',
      disabled: true,
    };
  }
  // idle / failed / ipcFailed — clickable. Failure-recoverable
  // (the user can fix drift by reverting their edits, etc.).
  const note =
    revert.state === 'failed' || revert.state === 'ipcFailed'
      ? 'try again — the last revert failed'
      : 'undo this patch';
  return {
    label: 'Revert',
    ariaLabel: 'Revert this patch using its checkpoint',
    title:
      'Drift-detect against the post-apply state, then restore the pre-apply files all-or-nothing.',
    note,
    disabled: false,
  };
}

function DiffStatusPill({
  validation,
  apply,
  revert,
}: {
  validation: DiffValidationState;
  apply: DiffApplyHandle;
  revert: DiffRevertHandle;
}) {
  // Render priority (highest first):
  //   1. Revert active/terminal/failure → revert pill (D33)
  //   2. Apply applied / applying / failed → apply pill (D31)
  //   3. Validation pill otherwise (D16)
  //
  // Revert shadowing apply is intentional: once the user has
  // pressed Revert, the most relevant state on the diff is the
  // revert's progress, not the (still-true) "this was applied
  // earlier" fact. Apply state remains queryable via the Apply
  // button's terminal label.
  if (revert.state === 'reverted') {
    const fileWord = revert.restored.length === 1 ? 'file' : 'files';
    return (
      <p
        className="plume-chat-diff-validation plume-chat-diff-validation-valid"
        role="status"
        aria-live="polite"
        title={revert.restored.map((r) => r.path).join('\n')}
      >
        reverted · {revert.restored.length} {fileWord} restored
      </p>
    );
  }
  if (revert.state === 'reverting') {
    return (
      <p
        className="plume-chat-diff-validation plume-chat-diff-validation-loading"
        role="status"
        aria-live="polite"
      >
        reverting patch…
      </p>
    );
  }
  if (revert.state === 'failed') {
    const headline = revert.details[0]?.message ?? 'unknown failure';
    return (
      <p
        className="plume-chat-diff-validation plume-chat-diff-validation-invalid"
        role="status"
        aria-live="polite"
        title={revert.details.map((d) => `${d.path}: ${d.message}`).join('\n')}
      >
        revert failed ({revertReasonLabel(revert.reason)}): {headline}
      </p>
    );
  }
  if (revert.state === 'ipcFailed') {
    return (
      <p
        className="plume-chat-diff-validation plume-chat-diff-validation-failed"
        role="status"
        aria-live="polite"
      >
        revert unavailable: {revert.message}
      </p>
    );
  }
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

function revertReasonLabel(reason: PatchRevertFailure): string {
  switch (reason) {
    case 'unknownCheckpoint':
      return 'unknown checkpoint';
    case 'drift':
      return 'post-apply drift';
    case 'writeFailed':
      return 'write';
    case 'unsupportedCheckpoint':
      return 'checkpoint format';
    default:
      return 'unknown';
  }
}
