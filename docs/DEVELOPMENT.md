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

## Optional: a local model runtime

Plume runs against any provider it has an adapter for. Pick at least one:

- **MLX-LM** (Apple Silicon, best perf): install inside `.venv` through
  `./scripts/dev-env.sh`.
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

# 3. Sanity check
./scripts/verify.sh
```

`./scripts/verify.sh` works even before `npm install` and `cargo fetch`. It
just skips the checks whose tools are not yet available, with a clear
`[WARN]`.

## Day-to-day

```bash
./scripts/dev-env.sh npm run tauri dev       # launches the desktop window
./scripts/dev-env.sh npm run typecheck       # tsc --noEmit
./scripts/verify.sh                          # pre-commit-grade checks
PLUME_FULL_VERIFY=1 ./scripts/verify.sh      # adds cargo clippy
```

## Layout

```
plume/
  AGENTS.md        rules for any contributor or AI agent
  README.md        product overview
  scripts/
    verify.sh      single source of truth for local checks
    dev-env.sh     project-local dependency/cache wrapper
  src/             frontend (TypeScript + React + CodeMirror)
  src-tauri/       backend (Rust + Tauri)
  docs/            architecture, providers, UI, safety, this file
  reference/       inspiration material, not bundled
```

## Adding a feature

1. Open the relevant doc under `docs/` and update it first if the change is
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
| Frontend    | (Vitest / Playwright once added)      | component states, IPC mocks             |
| Integration | mocked provider HTTP servers          | adapter parsing                         |
| Manual      | local model on a real Mac             | latency, memory, agent loop             |

Minimum bar for a new module: one happy-path test and one obvious
failure-mode test.

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
- `cargo fmt` complains about a file you did not touch — run
  `cd src-tauri && cargo fmt`. Don't disable `rustfmt` to silence it.
- `npm run tauri dev` fails with a WebView error — re-check the Tauri 2
  prerequisites for your OS.
