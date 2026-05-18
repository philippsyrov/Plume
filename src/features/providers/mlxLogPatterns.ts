// D57: heuristic detection of common mlx_lm.server failure shapes
// from the supervisor's stdout/stderr ring buffer.
//
// Why frontend-only: these patterns are pure-prose strings from
// upstream mlx_lm. Upstream changes them every couple of releases.
// A heuristic that ages out into a "no specific hint" is fine — the
// raw log is right there in the disclosure body. Doing the matching
// in Rust would require either a typed IPC change every time we
// extend the heuristic, or duplicating the patterns on both sides.
// Pure-frontend keeps the detector iterable.
//
// What this is NOT: a definitive "the model is unsupported" verdict.
// It's a contextual hint that surfaces above the log tail so the
// user doesn't have to read the traceback to know what category of
// problem they hit. The detector returns `null` whenever it can't
// pattern-match — we never invent a failure.
//
// Verified-against patterns (D56 smoke against mlx-community
// `gemma-4-e4b-it-4bit`, an unsupported `Gemma4ForConditionalGeneration`
// variant on mlx_lm 0.31.3):
//
//   ValueError: Received 126 parameters not in model:
//   language_model.model.layers.24.self_attn.k_norm.weight, ...
//
// Other mlx_lm.utils failure modes we map opportunistically:
//
//   - `Missing N parameters from model:` (mirror direction; same root
//     cause as Received-but-extra, namely architecture mismatch)
//   - `KeyError: '<model_type>'` from mlx_lm.utils.MODEL_REMAPPING or
//     model dispatch (unknown architecture entirely)
//   - `ImportError: cannot import name '<class>'` from `mlx_lm.models.*`
//     (architecture has a python module but it's missing a class —
//     usually a version skew between the on-disk weights and the
//     installed mlx_lm).

export type MlxLogHint = {
  /** Stable kind for callers to switch on. New kinds appended only
   *  with a backward-compatible UI default. */
  kind:
    | 'unsupported-architecture'
    | 'unknown-model-type'
    | 'import-error'
    | 'cuda-missing';
  /**
   * Short user-facing label. Rendered in the diagnostics disclosure
   * above the log `<pre>`. Plain prose, no markdown — the disclosure
   * is a `<dd>` text node, not a markdown renderer.
   */
  label: string;
  /**
   * One-line action hint. Tells the user what to try next. Stays
   * short — the disclosure already has room for the full log.
   */
  suggestion: string;
};

/**
 * Try to classify the supervisor's captured stdout/stderr into a
 * specific failure kind. Returns `null` when no pattern fires.
 *
 * The order of checks matters: we check the most specific patterns
 * first so an architecture mismatch isn't shadowed by a generic
 * `ImportError`. Each check is a substring or regex against the
 * raw log; nothing here parses Python tracebacks formally — that
 * would be brittle for no benefit.
 */
export function detectMlxLogHint(logTail: string): MlxLogHint | null {
  if (!logTail) return null;

  // D56 verified shape: ValueError raised from mlx.nn.layers.base
  // load_weights when the on-disk weight namespace doesn't match
  // the model class mlx_lm dispatched to.
  if (/Received \d+ parameters? not in model/.test(logTail)) {
    return {
      kind: 'unsupported-architecture',
      label: 'mlx-lm does not recognize this model architecture.',
      suggestion:
        'Use a text-only chat model whose architecture mlx-lm supports (e.g. text Gemma 2 / Llama 3 / Qwen 2.5). See docs/MLX_RUNTIME.md § Model architecture support.',
    };
  }

  // Mirror direction — same root cause, different message side.
  if (/Missing \d+ parameters? from model/.test(logTail)) {
    return {
      kind: 'unsupported-architecture',
      label: 'mlx-lm is missing weights the loaded model class expects.',
      suggestion:
        'This usually means the model architecture differs from what mlx-lm dispatched to. Pick a model whose architecture mlx-lm supports — see docs/MLX_RUNTIME.md § Model architecture support.',
    };
  }

  // mlx_lm.utils.MODEL_REMAPPING failure / unknown model_type.
  // Upstream's exception class varies: KeyError on the dict, or a
  // ValueError naming the model_type. Match both shapes loosely.
  if (
    /KeyError:\s*['"][^'"]+['"]/.test(logTail) &&
    /mlx_lm[/.](utils|models)/.test(logTail)
  ) {
    return {
      kind: 'unknown-model-type',
      label: "mlx-lm doesn't have a python module for this model_type.",
      suggestion:
        'The model\'s config.json declares a model_type mlx-lm doesn\'t know yet. Try a different model or update mlx-lm (`pip install -U mlx-lm`).',
    };
  }
  if (/Model type [^\s]+ not supported/i.test(logTail)) {
    return {
      kind: 'unknown-model-type',
      label: "mlx-lm doesn't support this model_type.",
      suggestion:
        "The model's config.json declares a model_type mlx-lm doesn't know yet. Try a different model or update mlx-lm.",
    };
  }

  // ImportError from mlx_lm.models.* — typically version skew
  // between the on-disk weights and the installed mlx_lm.
  if (
    /ImportError/.test(logTail) &&
    /mlx_lm[/.]models/.test(logTail)
  ) {
    return {
      kind: 'import-error',
      label: 'mlx-lm could not import the model class for this architecture.',
      suggestion:
        "Often a version skew. Update mlx-lm (`pip install -U mlx-lm`) and retry. If the model is brand-new, mlx-lm may not have caught up yet.",
    };
  }

  // CUDA-not-available — rare on Apple Silicon but possible if a
  // user activates the wrong venv. We surface a hint so the user
  // realises they need the Metal-backed install.
  if (/RuntimeError:.*CUDA|No CUDA GPUs|cuda is not available/i.test(logTail)) {
    return {
      kind: 'cuda-missing',
      label: 'mlx-lm process is trying to use CUDA, which is not available on this host.',
      suggestion:
        "Make sure the active venv has the Metal-backed mlx-metal install, not a CUDA build (`pip install -U mlx-lm` in your venv).",
    };
  }

  return null;
}
