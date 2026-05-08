# Plume — Agent Instructions

Plume is an experimental open-source local AI coding editor. The product brief
is `docs/PLUME_PROJECT_SPEC.md`. Treat it as the source of truth for product
direction.

## Stack

Tauri 2 (Rust) shell + TypeScript / React 19 frontend with CodeMirror 6 as the
editor surface. Local model runtimes (MLX-LM, Ollama, LM Studio, llama.cpp)
reach the app through a `Provider` trait in `src-tauri/src/providers/`. No
Electron. No default cloud calls.

## Status

Early foundation. Slice A landed the IPC and safety contracts.
Slice B added project open + persisted trust + `ProjectMeta`.
Slice C added trusted display reads, a read-only file browser,
CodeMirror viewing, blocked secret-file reads, and a packaged-app
smoke harness. Slice D0 documented the provider track vs engine
track split. Slice D1 added the provider registry plus
reachability UI — `providers.list`, `providers.health`, and a
small panel showing each runtime's category and current state.
Slice D1.5 reshaped the trusted-project view into a three-zone
workspace shell — left navigation (file tree + provider strip),
center agent placeholder, right file inspector — without
committing to a chat backend yet. The Rust backend compiles, the TS
frontend typechecks, and `./scripts/verify.sh` passes. Model
loading, chat, agent loop, file writes, and the patch flow are not
implemented yet. See `docs/DEVELOPMENT.md` for working with the
current slice and `docs/IPC_ROADMAP.md` for what's reserved.

## Key documents

- `docs/PLUME_PROJECT_SPEC.md` — product brief
- `docs/ARCHITECTURE.md` — process model, modules, IPC contract
- `docs/AGENT_OPERABILITY.md` — visible UI contract for human/agent control
- `docs/MODEL_PROVIDERS.md` — provider trait and per-runtime notes
- `docs/UI_STYLE.md` — hand-drawn cafe visual system
- `docs/SAFETY.md` — file/command sandbox + agent staging
- `docs/DEVELOPMENT.md` — dev setup, run, verify, test
- `docs/SMOKE_TESTING.md` — packaged app smoke checklist
- `docs/DEPENDENCY_ISOLATION.md` — local caches, venv, and no-global-install rules
- `docs/BOOTSTRAP.md` — implemented `~/scripts/setup-tauri-project.sh` contract

## Commands

- Verify (always available): `./scripts/verify.sh`
- Run a command with project-local caches: `./scripts/dev-env.sh <command>`
- Verify with clippy: `PLUME_FULL_VERIFY=1 ./scripts/verify.sh`
- Frontend dev (after `npm install`): `npm run dev`
- Tauri dev (after Rust + Node deps installed): `npm run tauri dev`
- Rust lint: `cargo fmt --check && cargo clippy --all-targets -- -D warnings`
- TS type check: `npm run typecheck`

## Project structure

```
plume/
  AGENTS.md
  README.md
  package.json
  tsconfig.json
  vite.config.ts
  index.html
  src/                          React + CodeMirror frontend
    main.tsx
    App.tsx
    features/
      agent/AgentWorkspace.tsx       center-zone placeholder (D1.5)
      editor/ReadOnlyEditor.tsx      CodeMirror display surface
      file-tree/FileBrowser.tsx      useFileNavigator + Navigator + Inspector
      providers/ProviderPanel.tsx    provider registry + reachability
    lib/api/                    typed Tauri-invoke wrappers
    styles/                     tokens.css, ink.css, layout.css
  src-tauri/                    Rust backend (Tauri)
    Cargo.toml
    tauri.conf.json
    capabilities/default.json   narrowed to core:event:default
    src/
      main.rs
      commands/                 IPC handlers
      project/                  project open + persisted trust
      fs/                       sandboxed display reads
      providers/                static registry + TCP reachability
      safety/                   path validation
      error.rs                  IpcRequest envelope + IpcError
  docs/
  scripts/
    verify.sh                   structural + guardrail + tool-aware
    dev-env.sh                  project-local cache wrapper
    smoke-app.sh                build + launch real Plume.app for agents
  reference/visual/             inspiration images, not bundled
```

## Hard rules

1. **No Electron.** This project exists partly to avoid Electron's memory
   cost on local-model laptops.
2. **No default cloud model calls.** Cloud providers must be opt-in and
   visibly labeled in the status strip.
3. **No filesystem writes outside the open project root** without explicit
   user approval. The Rust backend enforces this; the frontend never touches
   the disk directly.
4. **No shell command execution without user approval.** Verification
   commands detected from project files require an explicit approval prompt
   the first time, and that approval is scoped per-project.
5. **AGENTS.md beats CLAUDE.md.** If a `CLAUDE.md` ever appears, consolidate
   into AGENTS.md and remove the duplicate.
6. **Resource honesty in the UI.** Models too large for the user's machine
   must be flagged before load, not silently attempted.
7. **No unsolicited installs.** Never run `npm install`, `cargo install`,
   `brew install`, `pip install`, `npx create-*`, or any other dependency
   command without an explicit ask. Listing a dep in a manifest is fine;
   running an installer is not.
8. **Use the project env wrapper for dependency commands.** Run dependency,
   model-download, and build commands through `./scripts/dev-env.sh` so caches
   stay under the project instead of spreading into global user directories.

## Code style

- **Rust:** `cargo fmt`, idiomatic Rust 2021. Errors with `thiserror` /
  `anyhow` once adopted; never `unwrap` in production paths. Prefer typed
  errors over stringly-typed ones at module boundaries.
- **TypeScript:** strict mode, no `any` without a one-line comment justifying
  it, ES modules only, `camelCase` / `PascalCase` / `UPPER_SNAKE_CASE`.
- **Bounded collections only.** Caches, model registries, session histories
  must have a size cap or eviction policy. Memory leaks here directly hurt
  the model running alongside Plume.
- **Guard clauses over deep nesting** in both languages.
- **Comments only when WHY is non-obvious.** Don't restate the code.

## Before declaring a task done

1. `./scripts/verify.sh` passes.
2. Any new Rust module has at least one happy-path test and one failure-mode
   test (especially path-safety and command-validation paths).
3. Any new TS module has a smoke test or is exercised by the running app.
4. New user-facing strings respect the visual identity (no emoji, no glossy
   SaaS language, no purple-blue AI vibes).
5. `docs/` is updated when behavior or structure changes — doc-first prevents
   UI drift.

## Things to ask first

- Installing new crates or npm packages.
- Adding a new local model runtime, or removing an existing one.
- Anything that changes file/command sandbox rules.
- Renaming the project or top-level directories.
- Touching the user's global `~/scripts/` or `~/.claude/` directories.
- Initializing git, force-pushing, rewriting history, or anything destructive.
