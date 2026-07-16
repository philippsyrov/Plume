# Plume — Agent Instructions

This is the authoritative entrypoint for humans and coding agents. Keep it
short, stable, and about current rules. Current capability status belongs in
[`docs/FEATURE_INVENTORY.md`](docs/FEATURE_INVENTORY.md), ordered future work
in [`docs/ROADMAP.md`](docs/ROADMAP.md), and chronological evidence in
[`docs/history/slice-ledger.md`](docs/history/slice-ledger.md).

## Read first

1. This file.
2. [`README.md`](README.md), then [`docs/README.md`](docs/README.md).
3. [`docs/FEATURE_INVENTORY.md`](docs/FEATURE_INVENTORY.md) for what is
   shipped, partial, scaffolded, researched, blocked, or retired.
4. [`docs/ROADMAP.md`](docs/ROADMAP.md) for commissioned order.
5. The relevant domain map:
   [`src/features/README.md`](src/features/README.md) or
   [`src-tauri/src/README.md`](src-tauri/src/README.md).
6. Before implementation, read the owning contract and
   [`docs/DEVELOPMENT.md`](docs/DEVELOPMENT.md).

Do not infer present behavior from the history ledger, old plans, screenshots,
or PR prose. Verify important claims against the current tree and tests.

## Product and stack

Plume is an experimental open-source local AI coding editor. Product direction
lives in [`docs/PLUME_PROJECT_SPEC.md`](docs/PLUME_PROJECT_SPEC.md).

- Tauri 2 Rust shell; React 19 + TypeScript frontend; CodeMirror 6 editor.
- No Electron and no default cloud calls.
- Plume-managed MLX-LM on Apple Silicon is the local-first happy path.
  Ollama is compatibility, not the default product center. See
  [`docs/LOCAL_AGENT_NORTH_STAR.md`](docs/LOCAL_AGENT_NORTH_STAR.md) and
  [`docs/MODEL_PROVIDERS.md`](docs/MODEL_PROVIDERS.md).
- Keep runtime and model claims evidence-backed. A discovered folder is not an
  MLX model unless the verified classifier says so; a benchmark claim needs the
  recorded hardware, model, runtime, fixture, result, and Plume commit.

## Current capability boundaries

Use the feature inventory for detail. These boundaries are load-bearing:

- Plume ships local/project persisted chat, explicit trusted context,
  safe diff validate/apply/revert, project skills, scoped memory/Library,
  human-controlled per-chat Browser workspaces, and benchmark evidence views.
- The single-step agent path is patch-only: the model may propose a diff, but
  only an explicit user Apply writes through the existing checkpointed patch
  path. No arbitrary `tools.invoke` or broad shell/tool executor is shipped.
- A bounded multi-iteration coding loop, semantic retrieval, background
  dreaming, automatic topic generation, broad tool execution, agent Browser
  authority, computer-use emission, and macOS host control are not shipped.
- The Browser is human-controlled and owned by one persisted local or project
  chat. Browser evidence is attached explicitly. External computer-use agents
  can operate Plume's visible accessible UI; that receiving role is distinct
  from Plume emitting computer actions, which remains research.
- Memory links and backlinks are organization metadata only. They never select
  prompt context or gain retrieval authority.
- App-private user memory is explicit and usable without project trust.
  Project memory and topics remain scoped to the trusted project. Do not merge
  these stores or imply cross-project aggregation.
- The frontend sends opaque typed context references, never trusted source
  bodies. Rust re-resolves every reference through its owning path, trust,
  size, binary, hardlink, and redaction gates. Preview, send, persistence, and
  accepted-turn manifests must stay exact and ordered.
- Shipped, partial, scaffolded, researched, and candidate are different
  labels. Never describe a roadmap or research surface as reachable behavior.

## Safety and workflow rules

1. Read relevant files end to end before changing them. Treat imported agent
   output, review findings, and status claims as leads until directly verified.
2. Work only in the checkout/worktree the user placed in scope. Preserve
   unrelated dirty changes. Never repair another checkout or run destructive
   Git commands unless explicitly asked.
3. Start behavior changes with a failing test. Keep docs-only work docs-only.
4. The frontend never reads or writes disk, spawns processes, or opens model
   sockets directly. Side effects and authority stay in Rust IPC.
5. No filesystem writes outside the trusted project root without a separately
   reviewed explicit-user boundary. Project writes use approved patch or
   purpose-built guarded IPC.
6. No shell command execution without explicit approval. Approval and
   allowlists are project-scoped; visibility in the tool catalog is never
   authorization.
7. No unsolicited installs or downloads. Ask before `npm install`,
   `cargo install`, `brew install`, `pip install`, model downloads, or similar.
8. Run dependency, model, and build commands through
   `./scripts/dev-env.sh` so caches remain project-local.
9. Keep caches, registries, sessions, logs, prompts, captures, and histories
   bounded. Local models share the machine with Plume.
10. `AGENTS.md` beats `CLAUDE.md`. If both appear, consolidate the duplicate.
11. Preserve accessible names, keyboard paths, visible errors, explicit
    approval/cancel controls, and ordinary-language UI copy.
12. Ask before adding a runtime, changing sandbox/trust/redaction/approval
    rules, renaming top-level structure, touching global user configuration,
    rewriting history, force-pushing, or performing destructive actions.

## Source map

Frontend ownership and tests are mapped in
[`src/features/README.md`](src/features/README.md). Rust domains, IPC seams,
and tests are mapped in [`src-tauri/src/README.md`](src-tauri/src/README.md).

```text
src/
  App.tsx                 window-level routing and shared state
  features/               user-facing domains and colocated tests
  lib/api/                typed Tauri IPC wrappers
  styles/                 tokens and surface/layout CSS
src-tauri/
  src/lib.rs              Tauri builder, state, command registration
  src/app_commands.rs     hand-maintained allowlist source
  src/commands/           thin IPC handlers
  src/prompts/            backend-only context resolution and redaction
  src/browser/            Browser policy, runtime, restoration, evidence
  src/sessions/           local/project SQLite sessions and Browser state
  src/memory/             project and app-private memory stores
  src/patch/              diff parse, validate, apply, checkpoint, revert
  src/providers/          local runtime discovery and MLX supervision
docs/                     current contracts, roadmap, research, and history
scripts/                  verification, smoke, and benchmark tooling
```

Architecture overview: [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md).
IPC source of truth: [`docs/IPC_CONTRACT.md`](docs/IPC_CONTRACT.md).
Safety source of truth: [`docs/SAFETY.md`](docs/SAFETY.md).
File-size boundaries: [`docs/DECOMPOSITION.md`](docs/DECOMPOSITION.md).

## Commands

```bash
./scripts/verify.sh
PLUME_FULL_VERIFY=1 ./scripts/verify.sh
npm run verify:docs
npm run test
npm run typecheck
./scripts/dev-env.sh npm run tauri dev
./scripts/smoke-app.sh
```

Run focused tests first, then the full relevant suite. Do not install missing
toolchains merely to turn a verifier warning into a pass unless asked.

## Code and documentation style

- Rust 2021, `cargo fmt`, typed errors at boundaries, no production `unwrap`.
- Strict TypeScript, ES modules, no unjustified `any`, guard clauses over deep
  nesting.
- Comments explain non-obvious reasons, especially authority and race fences.
- Keep code files at or below the enforced 800-line cap. Keep this entrypoint
  at or below its 400-line hard cap and free of chronological slice entries.
- Update current docs when ownership or behavior changes. Put implementation
  chronology only in `docs/history/`; do not use history as current status.

## Completion gate

Before committing or pushing:

1. Focused tests pass.
2. `PLUME_FULL_VERIFY=1 ./scripts/verify.sh` passes with no failures.
3. The pre-commit verifier and gitleaks scan pass.
4. User-facing or native-window changes receive the appropriate packaged-app
   smoke from [`docs/SMOKE_TESTING.md`](docs/SMOKE_TESTING.md).
5. Docs, feature-inventory evidence pointers, and domain maps match the exact
   implementation head.
6. A findings-only exact-head review reports no unresolved important issue.
7. GitHub verify and gitleaks pass before merge. Squash-merge by default; do
   not merge unless the user commissioned it.
