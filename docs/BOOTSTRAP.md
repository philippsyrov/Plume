# Bootstrap contract - `setup-tauri-project.sh`

Philipp's `~/scripts/` directory now has three project bootstraps:

- `setup-project.sh` - JS/TS/Node projects.
- `setup-python-project.sh` - Python/ML projects.
- `setup-tauri-project.sh` - Rust/Tauri desktop projects.

`~/scripts/setup-tauri-project.sh` is implemented. It creates the shared
agent, verification, hook, CI, and hygiene layer for Tauri projects. It does
not create the actual app shell and it does not install dependencies.

Plume was scaffolded manually before the global Tauri script existed. After
`git init`, the global script was run once in Plume to add the shared hook, CI,
Claude project rule, `.agents` placeholder, and hygiene files. This document
is both the contract for future Tauri projects and the drift checklist for
keeping Plume aligned with that global bootstrap.

## Scope

Run the script from the root of a new or existing Tauri desktop project:

```bash
cd "/path/to/project"
~/scripts/setup-tauri-project.sh
```

The script is idempotent. If a file already exists, it skips that file instead
of overwriting it.

## What the script writes

| Path                              | Purpose                                      |
| --------------------------------- | -------------------------------------------- |
| `AGENTS.md`                       | Cross-agent project instructions             |
| `README.md`                       | Minimal project overview if missing          |
| `.claude/rules/tauri-rust-desktop.md` | Claude project-local Tauri/Rust rules   |
| `.agents/skills/README.md`        | Shared project-local references placeholder  |
| `scripts/verify.sh`               | Single verifier script                       |
| `.editorconfig`                   | Editor indentation/EOL/charset defaults      |
| `.gitignore`                      | Node, Rust, Tauri, env, and macOS ignores    |
| `.gitattributes`                  | Placeholder for generated-file metadata      |
| `.github/workflows/verify.yml`    | CI verify and secret-scan workflow           |
| `.git/hooks/pre-commit`           | Local verify + staged gitleaks hook if `.git` exists |

> Note on `.claude/rules/tauri-rust-desktop.md`: Plume diverges from this
> row — the file was removed because newer Claude builds try to open
> `.claude/rules` as a single file and crash on the directory shape. See
> the "Plume drift checklist" below. A future global-script revision will
> probably want to drop this row entirely.

## What the script does not do

- It does not run `npm install`, `cargo fetch`, `cargo install`,
  `brew install`, `pip install`, `npx create-*`, or any other installer.
- It does not fetch anything from the network.
- It does not generate `package.json`, `Cargo.toml`, `tauri.conf.json`, or
  application source files.
- It does not touch global Claude/Codex config such as `~/.claude` or
  `~/.codex`.
- It does not force git initialization.

That split is intentional: the script creates the project discipline layer;
the human still chooses the app stack details and when to install toolchains.

## Verifier contract

The global verifier created by `setup-tauri-project.sh` runs checks that are
meaningful for the files already present:

1. Required docs: `AGENTS.md` and `README.md`.
2. Frontend scripts if `package.json` exists: `typecheck`, `lint`, `test`,
   and `build` only when each script is defined.
3. Tauri config JSON parse check if `src-tauri/tauri.conf.json` exists.
4. Rust checks if `src-tauri/Cargo.toml` exists: `cargo fmt`, `cargo clippy`,
   and `cargo test`.

If the Rust toolchain is missing while Rust files exist, the global verifier
fails and tells the user to install Rust. Plume's current local verifier is
more forgiving during the very first scaffold stage: it reports missing
toolchains as `WARN` so the docs and guardrails can still be checked before
dependencies are installed.

## Pre-commit contract

If `.git/` already exists, the bootstrap installs `.git/hooks/pre-commit`:

```bash
#!/bin/bash
set -e

./scripts/verify.sh

if command -v gitleaks >/dev/null 2>&1; then
  gitleaks git --pre-commit --staged --redact --no-banner
else
  echo "gitleaks not installed - skipping local secret scan."
  echo "Install with: brew install gitleaks"
  echo "CI should still run a full secret scan."
fi
```

If `.git/` does not exist yet, the script skips the hook. Run the bootstrap
again after `git init`, or copy the hook manually from the script.

## CI contract

The global bootstrap writes `.github/workflows/verify.yml` with two jobs:

1. `verify` on Ubuntu:
   - checks out the repo,
   - sets up Node 20 when `package.json` exists,
   - installs dependencies from an existing lockfile only,
   - installs Rust components when `src-tauri/Cargo.toml` exists,
   - runs `./scripts/verify.sh`.
2. `secrets` on Ubuntu:
   - checks out full history,
   - runs `gitleaks/gitleaks-action`.

The workflow intentionally does not invent a package lockfile or run a project
generator. If dependencies are not committed yet, CI reaches the verifier and
lets the verifier explain what is missing.

## Plume drift checklist

Because Plume started as a manual scaffold and then received the shared
bootstrap layer, keep these differences visible:

- Plume has custom product docs beyond the generic bootstrap:
  `MODEL_PROVIDERS.md`, `UI_STYLE.md`, `SAFETY.md`, and
  `PLUME_PROJECT_SPEC.md`.
- Plume has app skeleton files: `package.json`, `src/`, `src-tauri/`, and
  `index.html`. The global bootstrap would not create these.
- Plume now has `.github/workflows/verify.yml`, `.gitattributes`,
  `.agents/skills/README.md`, and `.git/hooks/pre-commit` from the global
  bootstrap.
- Plume's verifier currently allows missing toolchains as warnings; the global
  verifier treats missing Rust as a real failure once Rust files exist.
- **Plume removed `.claude/rules/tauri-rust-desktop.md`** that the global
  bootstrap writes. Newer Claude builds were trying to open `.claude/rules`
  as a single file and crashing on the directory shape, and the file's three
  sections (dependency discipline, Tauri boundaries, verifier discipline)
  were already covered by `AGENTS.md`'s Hard Rules + "Before declaring a
  task done" checklist. `AGENTS.md` is the authoritative source per its own
  Hard Rule #5. If you re-run the global bootstrap on Plume, delete the file
  again — or update the global script to stop writing it once that's the
  agreed direction.

When the global bootstrap changes in the future, rerun this comparison before
committing drift back into Plume:

```bash
~/scripts/setup-tauri-project.sh
./scripts/verify.sh
```

Then review any newly created or changed bootstrap files before committing
them.
