# Dependency Isolation

Short answer: yes, but not as one magic `venv`.

Plume has three dependency worlds:

1. **Node** frontend dependencies live in `node_modules/`.
2. **Rust** crates and build output live in Cargo caches and `target/`.
3. **Python/model** tooling for MLX-LM lives in `.venv/` plus model caches.

Python has `venv`; Rust and Node do not use the same model. Plume's practical
answer is `scripts/dev-env.sh`, a small wrapper that makes future installs and
downloads prefer project-local ignored folders.

## What stays global

Some toolchains are still machine-level tools:

- Xcode Command Line Tools on macOS.
- Node.js itself.
- Rust/rustup itself.
- GUI apps such as Ollama or LM Studio, if you choose to use them.

Those are like installing Python itself. The project can avoid global package
pollution, but it still needs compilers/runtimes to exist somewhere.

## What stays local

Run dependency, fetch, build, and model-download commands through:

```bash
./scripts/dev-env.sh <command>
```

The wrapper sets:

| Env var | Local path | Used for |
| ------- | ---------- | -------- |
| `NPM_CONFIG_CACHE` | `.cache/npm/` | npm tarball cache |
| `NPM_CONFIG_PREFIX` | `.local/npm-global/` | accidental npm global prefix |
| `COREPACK_HOME` | `.cache/corepack/` | Corepack metadata |
| `PNPM_HOME` | `.local/pnpm/` | pnpm home if adopted later |
| `YARN_CACHE_FOLDER` | `.cache/yarn/` | Yarn cache if adopted later |
| `CARGO_HOME` | `.cargo-home/` | Cargo registry/git cache |
| `CARGO_TARGET_DIR` | `src-tauri/target/` | Rust build output |
| `PIP_CACHE_DIR` | `.cache/pip/` | pip cache |
| `PIP_REQUIRE_VIRTUALENV` | `true` | blocks accidental global pip installs |
| `HF_HOME` | `.cache/huggingface/` | Hugging Face / MLX model cache |
| `XDG_CACHE_HOME` | `.cache/xdg/` | libraries that respect XDG cache |
| `PLUME_MODEL_DIR` | `plume-models/` | Plume-managed model files scanned by `providers.localModels` |

These folders are ignored by git.

## First install commands

Only run these after the user explicitly approves dependency installation:

```bash
cd "/Users/philippsyrov/Desktop/CS Projects/Plume"

# Frontend packages stay in node_modules/ and npm cache stays in .cache/npm/.
./scripts/dev-env.sh npm install

# Rust crates stay in .cargo-home/ and build output stays in src-tauri/target/.
./scripts/dev-env.sh bash -lc 'cd src-tauri && cargo fetch'

# Optional MLX-LM Python env.
python3 -m venv .venv
./scripts/dev-env.sh bash -lc '. .venv/bin/activate && python -m pip install --upgrade pip mlx-lm'
```

## Day-to-day commands

```bash
cd "/Users/philippsyrov/Desktop/CS Projects/Plume"

./scripts/dev-env.sh npm run tauri dev
./scripts/dev-env.sh npm run typecheck
./scripts/verify.sh
```

## Cleanup

To wipe project-local dependencies and downloaded caches without touching the
rest of the laptop:

```bash
cd "/Users/philippsyrov/Desktop/CS Projects/Plume"
rm -rf node_modules .cargo-home src-tauri/target .venv .cache .local plume-models
```

Do not run that cleanup unless the user explicitly asks for it.

## Limits

This is dependency isolation, not a hostile-code sandbox. A malicious package
could still run arbitrary install scripts if you install it. For stronger
isolation later, consider a Nix/devbox setup, but that is heavier and usually
annoying for a Tauri GUI app on macOS.
