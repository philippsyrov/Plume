```research-metadata
{
  "family": "local-inference",
  "sourceDate": "2026-07-13",
  "hygiene": "official-public",
  "sources": ["https://github.com/ml-explore/mlx-lm", "../RUNTIME_COMPARISON.md", "../MODEL_BENCHMARKS.md"],
  "refreshTrigger": "Meaningful runtime release or new measured target-hardware evidence"
}
```

# Local Inference

## Observed behavior

MLX-LM provides the Apple-Silicon-native server surface Plume currently
supervises. llama.cpp, Ollama, LM Studio, and vLLM cover useful compatibility or
alternative deployment shapes. Runtime availability and theoretical fit do not
prove product-level coding quality or publishable performance.

## Plume adaptation

Keep Plume-managed MLX-LM as the local-first happy path on Apple Silicon.
Keep Ollama supported and honestly labelled as compatibility, not the center.
Separate raw inference, resource use, model quality, and end-to-end product
performance in every benchmark record.

## Already shipped overlap

Plume ships local-model discovery, verified MLX folder detection, supervised
MLX-LM start/stop and diagnostics, streaming MLX chat, Ollama-compatible chat,
and deterministic benchmark harness/viewer infrastructure.

## Remaining gap

The full target-hardware benchmark matrix has not run. D130 remains blocked on
real 128 GB M5 Max evidence and sanitized committed records; its launch and
README claims must distinguish measured fact, inference, and marketing copy.

## Rejected or deferred

Do not claim MLX speed, model fit, or agent quality from architecture alone.
Do not make Ollama the default path, auto-install runtimes, download models
without an explicit product gate, or publish D130 claims before the hardware
evidence exists.
