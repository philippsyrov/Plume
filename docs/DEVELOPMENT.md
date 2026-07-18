# Development

> This project does not bundle its toolchain. Install only what you need.
> Nothing in this file is run by automation — every command is a human
> decision.

## Required tools (when you are ready to actually run the app)

- **Rust** >= 1.78 with the stable toolchain. <https://rustup.rs>
- **Node.js** >= 20.
- **Tauri 2 prerequisites** for your OS:
  - macOS: Xcode Command Line Tools (`xcode-select --install`).
  - Linux: see <https://tauri.app/start/prerequisites/>.
  - Windows: WebView2 + MSVC build tools.

## Dependency isolation

There is no single Rust + Node + Python equivalent of Python's `venv`, so
Plume uses a small wrapper script instead:

```bash
./scripts/dev-env.sh <command>
```

The wrapper keeps npm cache/prefix, Cargo registry cache, Python pip cache,
Hugging Face model cache, and Plume model downloads inside ignored project
folders. Read `docs/DEPENDENCY_ISOLATION.md` before installing or fetching
anything.

## Local model runtimes

Plume runs against any provider it has an adapter for. Pick at least one:

- **MLX-LM** (Apple Silicon, local-first path): packaged builds carry the
  pinned runtime. A `.venv` is only a debug-development override.
- **Ollama**: <https://ollama.com>.
- **LM Studio**: <https://lmstudio.ai>.
- **llama.cpp**: build `llama-server` from source.

## First-time install

```bash
# 1. Frontend deps (lists what the manifest declares; safe to skip until
#    you actually want to run the app)
./scripts/dev-env.sh npm install

# 2. Cargo will fetch on first build
./scripts/dev-env.sh bash -lc 'cd src-tauri && cargo fetch'

# 3. Package-only prerequisite (Apple Silicon release/smoke builds)
#    Downloads runtime dependencies, never model weights.
npm run prepare:model-runtime

# 4. Sanity check
./scripts/verify.sh
```

`./scripts/verify.sh` works even before `npm install` and `cargo fetch`. It
just skips the checks whose tools are not yet available, with a clear
`[WARN]`.

## Day-to-day

```bash
./scripts/dev-env.sh npm run tauri dev       # launches the desktop window (raw dev binary)
./scripts/smoke-app.sh                       # builds + launches an addressable Plume.app (macOS)
npm run prepare:model-runtime                # stages pinned MLX + Apple helper resources
./scripts/install-dev-alias.sh               # optional: ~/Desktop/Plume (dev).app for one-click launches
./scripts/dev-env.sh npm run typecheck       # tsc --noEmit
./scripts/dev-env.sh npm run test            # Vitest component/unit tests
./scripts/verify.sh                          # pre-commit-grade checks
PLUME_FULL_VERIFY=1 ./scripts/verify.sh      # adds cargo clippy
```

Use `tauri dev` for fast UI iteration with hot reload. Use
`smoke-app.sh` when you (or an agent) need a real `.app` bundle macOS
LaunchServices can allowlist — see `docs/AGENT_OPERABILITY.md` § Smoke
Harness and `docs/SMOKE_TESTING.md`.

`prepare:model-runtime` requires Apple Silicon macOS, `uv`, Swift, and network
access when the project-local caches are cold. Generated payloads are ignored
and reproducible from the checked-in hash lock. Tauri maps them to
`mlx-runtime/` and `apple-model/` at the resource root. The application bundle
contains no Qwen/model weights.

Ordinary Rust compilation does not require those payloads or their toolchains.
`build.rs` creates the two empty ignored resource roots before Tauri reads the
bundle configuration; `prepare:model-runtime` populates them only for a release
or packaged smoke build.

For manual end-to-end testing, see `docs/MANUAL_TESTING.md`. It
includes a manual smoke checklist that exercises trust, file
browser, chat, propose-diff / apply / revert, memory, and the
local-model panel.

## Benchmarks (D128 contract; D129 harness)

`docs/MODEL_BENCHMARKS.md` is the binding evidence contract (D128);
`docs/BENCHMARK_HARNESS.md` documents the D129 implementation:
deterministic fixtures under `benchmarks/fixtures/`, the scripted fake
runtime, schema-v1 record validation with contradiction rules, and the
three reserved commands. Quick smoke against the fake runtime (no
model, no network):

```bash
scripts/benchmark-suite.sh benchmarks/plans/fake-smoke-plan.json
npx --no-install vite-node scripts/summarize-benchmarks.ts -- \
  benchmark-artifacts/fake-smoke.jsonl
```

The summarizer banners fake-runtime output as harness test data.

D129A adds the real MLX-LM adapter. With mlx-lm importable (see
`docs/MLX_RUNTIME.md`) and a checkpoint under `plume-models/`:

```bash
scripts/benchmark-mlx-smoke.sh
```

runs a tiny verified warm+cold matrix against the local checkpoint —
mechanics validation on one machine, never a performance claim.
Nothing in the harness downloads models or talks to anything but
127.0.0.1.

D129C adds the paired Plume-overhead smoke (builds the `plume_bench`
sidecar first, then measures the same checkpoint directly and through
Plume's real orchestration modules, printing per-pair
`extraOverheadMs`):

```bash
scripts/benchmark-plume-smoke.sh
```

D131 adds select-and-run presets from the model catalog
(`benchmarks/catalog/`): `scripts/benchmark-preset.sh` lists them,
`scripts/benchmark-preset.sh pong-paired-smoke` runs one — the
catalog's pinned artifact identity is re-verified against the live
checkpoint before anything runs.

D132 adds the in-app results viewer: open this repo as a trusted
project in Plume and pick **Benchmarks** from the **Workspace views** drawer to see
the recorded runs, group medians, raw-vs-Plume pairs, failures,
resource probes, and evidence files — read-only, validated by the
same reader the CLI summarizer uses. Runs still start here in the
terminal, never from the app.

## Layout

```
plume/
  AGENTS.md         rules for any contributor or AI agent
  README.md         product overview
  scripts/
    verify.sh       single source of truth for local checks
    dev-env.sh      project-local dependency/cache wrapper
    smoke-app.sh    builds and launches an addressable Plume.app for
                    accessibility / computer-use agents (macOS only;
                    debug profile; bundle at
                    src-tauri/target/debug/bundle/macos/Plume.app)
  src/              frontend (TypeScript + React + CodeMirror)
    features/README.md  frontend owners, tests, API seams, contracts
  src-tauri/        backend (Rust + Tauri)
    src/README.md    Rust owners, IPC seams, tests, contracts
  docs/             architecture, providers, UI, safety, this file
  reference/        inspiration material, not bundled
```

## Adding a feature

1. Use the relevant frontend or Rust domain map to identify the owner, tests,
   IPC seam, and contract. Open that contract under `docs/` and update it first
   if the change is
   user-visible. Doc-first prevents UI drift.
2. Implement the smallest backend change needed; add a unit test that
   proves the path-safety / command-safety contract still holds.
3. Implement the frontend; reuse `Ink*` primitives.
4. Run `./scripts/verify.sh` and `PLUME_FULL_VERIFY=1 ./scripts/verify.sh`.
5. Open a PR — the CI workflow (when enabled) runs the same verify
   script.

## Adding a model provider

See `docs/MODEL_PROVIDERS.md` § Adding a new provider. Short version:

1. New module under `src-tauri/src/providers/`.
2. Implement the `Provider` trait.
3. Register in the provider registry.
4. Add a card in the model picker.
5. Document tested models in `docs/MODEL_PROVIDERS.md`.

## Testing

| Layer       | Tool                                  | Covers                                  |
| ----------- | ------------------------------------- | --------------------------------------- |
| Rust unit   | `cargo test`                          | path safety, patch parser, command deny |
| Frontend    | `npm run test` (Vitest + happy-dom)   | component states, IPC mocks, UI guards  |
| Integration | mocked provider HTTP servers          | adapter parsing                         |
| Manual      | local model on a real Mac             | latency, memory, agent loop             |
| Agent smoke | bundled local app + computer use      | visible UI can be driven like a human   |

Minimum bar for a new module: one happy-path test and one obvious
failure-mode test.

### Frontend tests

Frontend tests live next to the code they pin as `*.test.ts` /
`*.test.tsx`. Vitest runs them in `happy-dom` with Testing Library
matchers loaded from `src/test/setup.ts`.

Use these for UI state machines and IPC-mockable component behavior:
disabled buttons, visible hints, copy text, selector guards, and
small pure helpers. Do not start Tauri, `mlx-lm`, or provider daemons
from Vitest; those stay in `docs/MANUAL_TESTING.md` / smoke scripts.

`./scripts/verify.sh` runs `npm run test` whenever `node_modules/`
is present, so a frontend regression should fail the same local gate
as TypeScript.

For UI slices, also do a quick agent-operability pass: keyboard path,
accessible names, visible errors, and visible approval/cancel controls.
The contract lives in `docs/AGENT_OPERABILITY.md`; the repeatable packaged
app checklist lives in `docs/SMOKE_TESTING.md`.

## Pre-commit hook (after `git init`)

The global Tauri bootstrap has installed `.git/hooks/pre-commit` for this
repo. It runs `./scripts/verify.sh` and then runs a staged gitleaks scan when
`gitleaks` is available.

If the hook is missing in a fresh clone, recreate it with:

```bash
mkdir -p .git/hooks
cat > .git/hooks/pre-commit <<'EOF'
#!/bin/bash
set -e
./scripts/verify.sh
EOF
chmod +x .git/hooks/pre-commit
```

## CI (after the repo is on GitHub)

The global Tauri bootstrap has added `.github/workflows/verify.yml`. It runs
`./scripts/verify.sh` and a gitleaks secret scan on push and PR.

## Release verification

Until the build pipeline can produce a real desktop installer, this
section is the contract. The implementation of `scripts/verify-release.sh`
follows when there is something to scan; until then, treat the lists
below as the human checklist for any tagged release.

The motivation lives in `docs/CLAUDE_CODE_REFERENCE_NOTES.md` §
Sourcemap Leak Lesson — the Claude Code source-map leak shipped because
a build default emitted source maps and an `.npmignore` didn't filter
them. No one would have caught it in PR review; the defense has to be
at the artifact-inspection layer.

### Bundle audit

The release script must fail on any of:

- `dist/**/*.map` exists in the bundled output.
- Any `.map` under `src-tauri/target/release/bundle/**` contains a
  `"sourcesContent"` JSON key.
- Any release artifact contains `.env`, `.plume/`, `node_modules`, the
  contents of `reference/`, or files matching the secret-pattern list
  used by `safety::secrets`.

### Build config audit

- `tauri.conf.json` (prod profile) has
  `dangerous_disable_asset_csp_modification: false`.
- `productName` and `identifier` are stable — no debug names in release.
- Vite `build.sourcemap` is set explicitly. If maps ship at all, they
  ship as *separate `.map` artifacts uploaded to a crash service*, never
  bundled in the desktop installer.
- No `VITE_*` env var contains a secret — Vite inlines them into the
  bundle.
- `Cargo.toml` `[profile.release]` has `strip = true`, `debug = false`.

### Source hygiene

- No file under `vendor/`, `third_party/`, or similar is derived from
  any leaked-source repo (see `docs/CLAUDE_CODE_REFERENCE_NOTES.md` §
  Legal / Source Hygiene). Failing this means failing the release.
- No prompt template under `src-tauri/src/prompts/` carries text from
  outside sources without an audit comment naming the public origin.

### Repo hygiene (continuous, not just at release)

- `.gitignore` covers `.plume/`, `*.log`, `*.local.json`, `.env*`,
  `target/`, `dist/`, `node_modules/`.
- `gitleaks` runs in pre-commit and CI (already configured by the
  bootstrap script).

## Troubleshooting

- `verify.sh` prints `[WARN] node not installed` — that's fine; install
  Node 20+ and rerun.
- `verify.sh` prints `[WARN] node_modules missing` — run `npm install`
  once. The TS check needs the toolchain, not just the manifest.
- `verify.sh` prints `[WARN] cargo not installed` even though you ran
  rustup — `verify.sh` now auto-prepends `$HOME/.cargo/bin` to `PATH`,
  so this should only happen if rustup installed Cargo somewhere else.
  Source `~/.cargo/env` (or whatever your install put it in) before
  rerunning.
- `cargo fmt` complains about a file you did not touch — run
  `cd src-tauri && cargo fmt`. Don't disable `rustfmt` to silence it.
- `npm run tauri dev` fails with a WebView error — re-check the Tauri 2
  prerequisites for your OS.
