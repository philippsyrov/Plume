# Third-Party Runtime Notice

Plume's generated macOS model-runtime payload includes standalone CPython and
the Python packages pinned in `scripts/mlx-runtime-requirements.lock`.

- CPython is distributed under the Python Software Foundation License.
- MLX, MLX-LM, and MLX Metal are distributed under their upstream open-source
  licenses. Package license metadata is preserved inside the generated Python
  installation.
- The Apple Foundation Models helper is Plume code. It links at runtime to the
  Apple FoundationModels framework supplied by macOS; Plume does not
  redistribute that framework or an Apple model.

Model weights are not application resources. The optional fixed Qwen model is
downloaded only after an explicit user action and retains its Apache-2.0
license and pinned-source metadata in the catalog/install receipt.
