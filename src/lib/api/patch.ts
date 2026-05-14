// Typed wrappers for the `patch.*` IPC family.
//
// D16 — `patch.validate`: read-only validator for model-emitted
// unified diffs. No model call, no disk writes.
//
// D31 — `patch.apply`: applies a previously-validated diff inside
// the trusted project root. Same envelope / camelCase / in-band
// error conventions as `patch.validate`.
//
// Surface rule (shared across the family): structured outcomes
// come back IN-BAND. The `Promise` only rejects for IPC-shape
// problems (`Version`, `BadArgument`) or trust gating
// (`NeedsApproval` — no trusted project open). Path escapes,
// malformed diffs, pre-image drift, mid-apply write failures —
// all in-band.
//
// See `docs/IPC_CONTRACT.md § patch` for the full shape.

import { invokeIpc } from './ipc';

export type PatchChangeType = 'modify' | 'create' | 'delete' | 'rename';

export type PatchTouch = {
  /** Project-relative, normalised (no `./`) path. */
  path: string;
  /** Hunks targeting this file. */
  hunks: number;
  changeType: PatchChangeType;
  /** Set only when `changeType === 'rename'`. Carries the
   * source (pre-rename) path. */
  renamedFrom?: string;
};

/** Stable kind codes for validation failures. The frontend
 * switches on these; `message` is for display only.
 *
 * New variants are additive — a kind the frontend doesn't yet
 * recognise should be rendered as a generic "invalid diff" with
 * `message` as the diagnostic. */
export type PatchValidationErrorKind =
  | 'noDiffBlock'
  | 'noHunks'
  | 'malformed'
  | 'devNullBoth'
  | 'pathEscape'
  | 'absolutePath';

export type PatchValidationError = {
  kind: PatchValidationErrorKind;
  /** Always populated. Surface-only — `kind` is the
   * machine-stable discriminator. */
  message: string;
  /** Diff-side path the error attached to, when applicable. */
  path?: string;
  /** 1-based line offset in the input, when applicable. */
  line?: number;
};

export type PatchValidateResponse =
  | {
      ok: true;
      touches: PatchTouch[];
      /** Total hunks across all touched files. */
      hunks: number;
    }
  | {
      ok: false;
      /** At least one entry. `errors[0]` is the headline. */
      errors: PatchValidationError[];
    };

export type PatchValidateRequest = {
  /** Raw assistant reply text or a bare unified diff. The
   * backend extracts the fenced ```diff/```patch block when
   * present, otherwise treats the payload as a raw diff. */
  diff: string;
};

export function validatePatch(
  payload: PatchValidateRequest,
): Promise<PatchValidateResponse> {
  return invokeIpc<PatchValidateRequest, PatchValidateResponse>(
    'patch_validate',
    payload,
  );
}

// ─── D31: patch.apply ────────────────────────────────────────────────────────

/** Stable failure codes for `patch.apply`. The frontend switches
 * on these; `details[].message` is for display only.
 *
 * Mirrors `PatchApplyFailure` in `src-tauri/src/patch/apply.rs`.
 * New variants are additive — a kind the frontend doesn't yet
 * recognise should be rendered as a generic "apply failed" with
 * the first detail message. */
export type PatchApplyFailure =
  | 'validationFailed'
  | 'preImageMismatch'
  | 'checkpointFailed'
  | 'writeFailed'
  | 'scopeUnsupported';

export type PatchFailureDetail = {
  path: string;
  /** 1-based hunk index within the file, when the failure
   * attaches to a specific hunk. */
  hunkIndex?: number;
  /** Surface-only. The `kind`-style discriminator lives one
   * level up on `PatchApplyResponse.reason`. */
  message: string;
};

export type PatchAppliedFile = {
  path: string;
  changeType: PatchChangeType;
  /** Post-apply file size on disk. `0` for delete. */
  bytesWritten: number;
};

export type PatchApplyResponse =
  | {
      applied: true;
      /** Opaque id of the pre-apply checkpoint. The frontend
       * stores this on the assistant turn so the Revert button
       * has something to send. D33 wired the actual revert verb;
       * D31 only stored the id. */
      checkpoint: string;
      touched: PatchAppliedFile[];
    }
  | {
      applied: false;
      reason: PatchApplyFailure;
      /** Per-file (or per-hunk) detail. May be empty for failure
       * kinds that don't carry per-file breakdown
       * (e.g. `checkpointFailed`). */
      details: PatchFailureDetail[];
    };

export type PatchApplyRequest = {
  /** Raw assistant reply text or a bare unified diff. Same shape
   * `validatePatch` accepts — the backend re-runs the validator
   * server-side, so the frontend's cached validation result is
   * a UI hint, not a security artifact. */
  diff: string;
};

export function applyPatch(
  payload: PatchApplyRequest,
): Promise<PatchApplyResponse> {
  return invokeIpc<PatchApplyRequest, PatchApplyResponse>(
    'patch_apply',
    payload,
  );
}

/**
 * D33 — `patch.revert` wire shape.
 *
 * Reasons:
 * - `unknownCheckpoint`: id missing from the store, malformed
 *   (path-escape, empty), or the checkpoint dir was GC'd since
 *   apply. Idempotent revert calls collapse here when GC has run.
 * - `drift`: at least one touched file's current content does
 *   not match the post-apply state Plume captured at apply time.
 *   The user has edited since apply; a silent revert would
 *   destroy that work. No `override: 'discardLocalEdits'` flag
 *   ships yet — the design defers that to a slice that adds the
 *   matching approval prompt.
 * - `writeFailed`: a mid-revert disk write failed. Revert
 *   captures a pre-revert snapshot in memory and rolled it back;
 *   if rollback also failed the message carries both errors.
 * - `unsupportedCheckpoint`: checkpoint was created by D31 or
 *   earlier — no `post/` tree, no manifest version. Revert can't
 *   drift-detect without the expected-post-apply signature. The
 *   pre-image is still under `.plume/checkpoints/<id>/files/`
 *   for manual recovery if the user really needs to roll back.
 */
export type PatchRevertFailure =
  | 'unknownCheckpoint'
  | 'drift'
  | 'writeFailed'
  | 'unsupportedCheckpoint';

export type PatchRestoredFile = {
  /** Project-relative path of the file the revert touched. For
   * rename revert this is the OLD path (where the file is after
   * revert), matching the user's mental model of "we undid the
   * rename." */
  path: string;
  changeType: PatchChangeType;
};

export type PatchRevertResponse =
  | {
      reverted: true;
      restored: PatchRestoredFile[];
    }
  | {
      reverted: false;
      reason: PatchRevertFailure;
      details: PatchFailureDetail[];
    };

export type PatchRevertRequest = {
  /** Opaque checkpoint id from a previous `applyPatch` success. */
  checkpoint: string;
};

export function revertPatch(
  payload: PatchRevertRequest,
): Promise<PatchRevertResponse> {
  return invokeIpc<PatchRevertRequest, PatchRevertResponse>(
    'patch_revert',
    payload,
  );
}
