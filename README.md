# Plume

A hand-drawn local AI coding editor for Apple Silicon Macs and other modest
hardware. Plume is a quiet black-and-white coding cafe that runs open models
through lightweight native tooling, respects laptop memory, and gives students
and indie hackers a private Codex-style workflow without pretending small
local models are magic.

## Status

Early foundation. The product brief, architecture, and visual system are
written. Slice A landed the IPC and safety contracts; Slice B implemented
project open + persisted trust + `ProjectMeta` with a minimal Rust backend,
typed TS wrapper, and an open-and-trust UI. The Rust backend compiles, the
TS frontend typechecks, and `./scripts/verify.sh` (with
`PLUME_FULL_VERIFY=1` for clippy) passes. Provider integration, file
reads/writes, and the patch flow are not implemented yet — see
`docs/DEVELOPMENT.md` and `docs/IPC_ROADMAP.md` for what comes next.

## Stack

- Desktop shell: **Tauri 2** (Rust)
- Frontend: **TypeScript + React 19**
- Editor: **CodeMirror 6**
- Local model runtimes (provider plugins): **MLX-LM**, **Ollama**, **LM
  Studio**, **llama.cpp**
- No Electron. No default cloud calls.

## Read this first

1. `docs/PLUME_PROJECT_SPEC.md` — long product brief and motivation.
2. `AGENTS.md` — rules every contributor and AI agent must follow.
3. `docs/ARCHITECTURE.md` — how the pieces fit.
4. `docs/MODEL_PROVIDERS.md` — how local model runtimes plug in.
5. `docs/UI_STYLE.md` — visual system.
6. `docs/SAFETY.md` — file/command sandbox.
7. `docs/DEVELOPMENT.md` — dev setup and commands.
8. `docs/DEPENDENCY_ISOLATION.md` — keep installs and caches inside the project.
9. `docs/BOOTSTRAP.md` — implemented `setup-tauri-project.sh` contract.

## Quick start (after toolchains are installed)

> The commands below are intentionally **not** run by automation. Install only
> what you actually need.

```bash
# 1. Toolchains (one-time, manual)
xcode-select --install                                    # macOS
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
# Node 20+ via nvm or Homebrew

# 2. Project deps (routed through dev-env so caches stay project-local)
./scripts/dev-env.sh npm install
./scripts/dev-env.sh bash -lc 'cd src-tauri && cargo fetch'

# 3. Run dev shell
./scripts/dev-env.sh npm run tauri dev

# 4. Verify before committing
./scripts/verify.sh
```

## Verifying right now (without any toolchain installed)

`./scripts/verify.sh` works even before `npm install` and `cargo fetch`. It
checks docs/structure/guardrails unconditionally and skips Rust/frontend
checks with a `WARN` when their tools aren't installed.

```bash
./scripts/verify.sh
```

## Repo layout

```
plume/
  AGENTS.md           rules for every contributor and AI agent
  README.md           you are here
  package.json        frontend manifest (deps not installed yet)
  src/                React + CodeMirror frontend (skeleton)
  src-tauri/          Tauri / Rust backend (skeleton)
  docs/               architecture, providers, UI, safety, dev
  scripts/
    verify.sh         single source of truth for local checks
    dev-env.sh        project-local cache wrapper for installs
  reference/visual/   inspiration images, not bundled
```

## License

To be decided once the project is past prototype. Until then, treat the
source as "all rights reserved" by default.
