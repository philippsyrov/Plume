# Generated Model Runtime Resources

This directory owns build inputs and documentation for Plume's bundled model
runtimes. `generated/` is deliberately gitignored: release and smoke builds
create it with `npm run prepare:model-runtime`.

The generated payload contains:

- a standalone arm64 CPython 3.12.13 runtime with the hash-locked MLX-LM stack;
- the thin arm64 `plume-apple-model` helper executable; and
- `runtime-identity.json`, which records the Python/package versions and exact
  `scripts/mlx-runtime-requirements.lock` SHA-256.

It never contains Qwen or other model weights. The fixed Qwen checkpoint is an
explicit user download into the app's Application Support directory. Tauri
maps the generated directories to resource-root `mlx-runtime/` and
`apple-model/`, matching the Rust-only resolvers.
