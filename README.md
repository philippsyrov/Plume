# Plume

A hand-drawn local AI coding editor for Apple Silicon Macs and other modest
hardware. Plume is a quiet black-and-white coding cafe that runs open models
through lightweight native tooling, respects laptop memory, and gives students
and indie hackers a private Codex-style workflow without pretending small
local models are magic.

## Status

Plume is an early local-first coding editor with persisted local/project chat,
Apple On-Device, Plume-managed MLX-LM, and Ollama chat, trusted project context, safe
diff/apply/revert, project memory and curated topics, session branching,
project skills, a scope-aware Library, a human-controlled per-chat Browser,
a reproducible benchmark evidence viewer, and a typed explicit context shelf
with exact prompt manifests. The bounded agent loop, semantic retrieval,
agent-driven Browser actions, computer-use emission, and broad tool execution
are not shipped.

For exact evidence, see [docs/FEATURE_INVENTORY.md](docs/FEATURE_INVENTORY.md).
For ordered work, see [docs/ROADMAP.md](docs/ROADMAP.md).

## OpenAI Build Week judge build

Plume's Developer Tools entry is a local-first AI workspace that makes agent
context, browser evidence, memory, and file changes visible, inspectable, and
reversible. The judge candidate is **Plume 0.1.0 for macOS on Apple Silicon**.

Judges can follow the concise [no-rebuild testing path](docs/build-week/judge-testing.md).
The [Build Week evidence index](docs/build-week/README.md) links the packaged
release proof, qualifying-window audit, UI evidence, and sub-three-minute demo
script. The current source tree also adds a first-run model chooser: supported
hosts can use Apple's on-device system model, while Qwen Coder 1.5B is an
explicit, verified weights download served by Plume's bundled MLX-LM runtime.
Apple host availability and the Qwen download remain honest runtime states;
neither is required for the no-model context and restoration path.

Codex with **GPT-5.6 Sol** was used as Plume's development and review agent
during the qualifying Build Week work; it is build provenance, not a runtime
GPT-5.6 integration. Codex accelerated implementation, testing, and packaged
app review while supporting key product, engineering, and design decisions:
opaque typed context references with exact manifests, human-controlled Browser
evidence, scope-aware Library and persistence, local-runtime lifecycle and
streaming hardening, and the judge-facing composer/context-shelf cleanup. The
[eligibility evidence](docs/build-week/eligibility-evidence.md) separates this
qualifying work from the editor foundation that predates the event.

The current candidate is ad-hoc signed rather than Apple Developer ID signed
and notarized, so macOS may require **Privacy & Security → Open Anyway**. The
repository does not currently contain a public binary download; the final DMG
must be uploaded before submission.

## Stack

- Desktop shell: **Tauri 2** (Rust)
- Frontend: **TypeScript + React 19**
- Editor: **CodeMirror 6**
- Local model runtimes and adapters: **Apple Foundation Models**, **MLX-LM**, **Ollama**, **LM
  Studio**, **llama.cpp**
- No Electron. No default cloud calls.

## Read this first

1. [Plume Handbook](docs/USER_GUIDE.md) — setup and everyday workflows in
   plain language.
2. [Documentation map](docs/README.md) — product, safety, implementation, and
   research entry points.
3. [Feature inventory](docs/FEATURE_INVENTORY.md) — current capability and evidence.
4. [Ordered roadmap](docs/ROADMAP.md) — commissioned sequence and dependencies.

Contributors and coding agents should then read [AGENTS.md](AGENTS.md), the
authoritative project workflow, followed by the
[frontend](src/features/README.md) or [Rust](src-tauri/src/README.md) domain map
for the area they will change.

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
checks required-document presence, structure, and guardrails unconditionally.
The TypeScript documentation correctness checks run when Node and
`node_modules` are available; unavailable Rust/frontend/doc tool checks are
skipped with a `WARN`.

```bash
./scripts/verify.sh
```

## Repo layout

```
plume/
  AGENTS.md           rules for every contributor and AI agent
  README.md           you are here
  package.json        frontend manifest
  src/                React + CodeMirror frontend
  src-tauri/          Tauri / Rust backend
  docs/               architecture, providers, UI, safety, dev, smoke
  scripts/
    verify.sh         single source of truth for local checks
    dev-env.sh        project-local cache wrapper for installs
    smoke-app.sh      build and launch an addressable Plume.app
  reference/visual/   inspiration images, not bundled
```

## License

Plume is available under the [MIT License](LICENSE).
