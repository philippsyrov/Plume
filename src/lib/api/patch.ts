// D16: typed wrapper for `patch.validate` — read-only validator
// for model-emitted unified diffs.
//
// Plume's propose-diff mode (D15) renders the diff. D16 layers a
// validator: same IPC contract conventions (envelope, error
// model, camelCase wire), no model call, no disk writes, no
// patch apply.
//
// Surface rule: structured validation errors come back IN-BAND on
// `{ ok: false }`. The `Promise` only rejects for IPC-shape
// problems (`Version`, `BadArgument`) or trust gating
// (`NeedsApproval` — no trusted project open). Path escapes,
// malformed diffs, missing hunks etc. all land in `errors[]`.
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
