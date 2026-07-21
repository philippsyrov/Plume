import { describe, expect, it } from 'vitest';

import { detectMlxLogHint } from './mlxLogPatterns';

describe('detectMlxLogHint', () => {
  it('classifies the D56 Gemma4 unsupported-architecture traceback', () => {
    const logTail = String.raw`
Traceback (most recent call last):
  File "/Users/example/.venvs/mlx-env/lib/python3.13/site-packages/mlx_lm/server.py", line 1042, in create_chat_completion
    response = self.model_provider.generate(messages, **generation_args)
  File "/Users/example/.venvs/mlx-env/lib/python3.13/site-packages/mlx_lm/utils.py", line 730, in load_model
    model.load_weights(list(weights.items()))
  File "/Users/example/.venvs/mlx-env/lib/python3.13/site-packages/mlx/nn/layers/base.py", line 181, in load_weights
    raise ValueError(
ValueError: Received 126 parameters not in model:
language_model.model.layers.24.self_attn.k_norm.weight, language_model.model.layers.24.self_attn.v_norm.weight
`;

    expect(detectMlxLogHint(logTail)?.kind).toBe('unsupported-architecture');
  });

  it('classifies unknown model_type failures from mlx_lm dispatch', () => {
    const logTail = `Traceback\n  File "mlx_lm/utils.py", line 230, in get_model_path\nKeyError: 'qwen9000'`;

    expect(detectMlxLogHint(logTail)?.kind).toBe('unknown-model-type');
  });

  it('classifies import errors from mlx_lm model modules', () => {
    const logTail =
      "ImportError: cannot import name 'Gemma4ForConditionalGeneration' from 'mlx_lm.models.gemma4'";

    expect(detectMlxLogHint(logTail)?.kind).toBe('import-error');
  });

  it('classifies CUDA-only environment failures', () => {
    const logTail = 'RuntimeError: CUDA is not available on this host';

    expect(detectMlxLogHint(logTail)?.kind).toBe('cuda-missing');
  });

  it('returns null for benign logs', () => {
    expect(detectMlxLogHint('INFO:     127.0.0.1:64606 - "GET /health" 200 OK')).toBeNull();
    expect(detectMlxLogHint('')).toBeNull();
  });
});
