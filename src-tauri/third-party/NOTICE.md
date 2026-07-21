# Third-Party Runtime Notice

Plume's generated macOS model-runtime payload includes standalone CPython and
the Python packages pinned in `scripts/mlx-runtime-requirements.lock`.

- CPython is distributed under the Python Software Foundation License.
- MLX, MLX-LM, MLX-VLM, and MLX Metal are distributed under their upstream
  open-source licenses. Package license metadata is preserved inside the
  generated Python installation.
- The Apple Foundation Models helper is Plume code. It links at runtime to the
  Apple FoundationModels framework supplied by macOS; Plume does not
  redistribute that framework or an Apple model.

Model weights are not application resources. The optional fixed Qwen and Gemma
models are downloaded only after an explicit user action and retain their
license and pinned-source metadata in the catalog/install receipt.

## Gemma model notice

Gemma is provided under and subject to the Gemma Terms of Use found at
<https://ai.google.dev/gemma/terms>.

Those terms incorporate the Gemma Prohibited Use Policy at
<https://ai.google.dev/gemma/prohibited_use_policy>. They govern the optional
Gemma model downloaded directly from its pinned Hugging Face source; they do
not change Plume's own source-code license. By downloading or using Gemma
through Plume, the user agrees to follow those terms and restrictions.
