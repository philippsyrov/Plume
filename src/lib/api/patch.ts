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
      /** Opaque id of the pre-apply checkpoint. Reserved for
       * `patch.revert` in D32; D31 stores it on the assistant
       * turn but does not yet expose a Revert button. */
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
