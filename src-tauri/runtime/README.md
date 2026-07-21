# Generated Model Runtime Resources

This directory owns build inputs and documentation for Plume's bundled model
runtimes. `generated/` is deliberately gitignored: release and smoke builds
create it with `npm run prepare:model-runtime`.

Fresh-clone Rust compilation creates empty `generated/mlx-runtime/` and
`generated/apple-model/` roots so Tauri can read its bundle manifest without a
package-runtime download. Those empty roots are not a usable bundled runtime;
the explicit preparation command below is still required for packaged builds.

The generated payload contains:

- a standalone arm64 CPython 3.12.13 runtime with the hash-locked MLX-LM and
  MLX-VLM stack;
- the thin arm64 `plume-apple-model` helper executable; and
- `runtime-identity.json`, which records the Python/package versions and exact
  `scripts/mlx-runtime-requirements.lock` SHA-256.

It never contains Qwen Coder, Qwen2-VL, or other model weights. Each fixed checkpoint is
an explicit user download into the app's Application Support directory. Tauri
maps the generated directories to resource-root `mlx-runtime/` and
`apple-model/`, matching the Rust-only resolvers.
