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
committing to a chat backend yet. Slice D2 layered the first
adapter-specific HTTP probe on top: when Ollama answers TCP, Plume
also fetches `/api/tags` and shows the installed model count plus
names in the provider panel. Slice D3 turned "Ollama has models" into
"which ones fit this machine" — a lazy per-model `POST /api/show`
reads family / parameter count / quantization / context length, a
cautious fit estimator weighs them against `sysctl hw.memsize`, and
the panel expands each model in place with a green / amber / red
verdict plus a `GGUF / Metal (Ollama)` runtime-path label. Slice D4
extended the model-list probes to LM Studio and llama.cpp via their
OpenAI-style `/v1/models` endpoints; those expose
runtime-reported / server-visible model ids only, not full
downloaded catalogs (LM Studio's richer `/api/v1/models` is
roadmap). llama.cpp moved off the "not configured" list onto the
TCP probe set at the documented 8080 default, with the shared
parser in `src-tauri/src/providers/openai_compat.rs`. Slice D5
added a lightweight host-machine status strip: `system.snapshot`
shells out to `sysctl` / `vm_stat` / `uname` / `sw_vers` and feeds
memory pressure + RAM-used + swap chips polled at ~7s, with CPU /
GPU live usage explicitly on the roadmap. Slice D6 added a
window-local model picker shell: each model row in the provider
panel grew a Select button (disabled when reachability is not
`available`), the agent workspace gained a "Selected model"
banner above the mode cards, and selection state lives in React
in `TrustedView` via `useSelectedModel`. Slice D7 added the first
real read-only chat against Ollama. Slice D7.1 turned that into a
streaming surface: `chat.send` now returns a `ChatStreamId`
immediately and the assistant reply arrives as `chat.token`
events; a new `chat.cancel(streamId)` verb flips a cooperative
cancel flag the streaming loop polls between NDJSON line reads.
The `ChatPanel` renders the streaming reply in place with a
blinking cursor and shows a Stop button while a stream is in
flight; cancellation keeps the partial reply in the transcript
with a "stopped by you" marker. Slice D8 added read-only file
context for chat: an "Attach current file" control on the chat
panel hands a project-relative path to the backend, which uses a
Rust-private prompt-read path (`prompts::assemble` +
`prompts::read::read_for_prompt` + `prompts::redact`) to fold the
file content into the last user message. Raw bytes never cross
IPC; the secret redactor (`AKIA…`, `ghp_…`, `sk-…`, JWTs,
`Bearer …`) is the only producer of `RedactedContent`, and a 256
KiB cap plus binary / secret-filename / `.git/`-whitelist blocks
sit in front of it. The chip on the chat panel is the source of
truth for what got attached; clearing it removes the context
from the next send. Slice D9 layered provider-neutral generation
telemetry onto `chat.done`: the terminal event now carries a
`stats` object with `outputTokens`, `evalMs`, `tokensPerSecond`,
`promptTokens`, and `promptMs` parsed from Ollama's final-frame
`eval_count` / `eval_duration` / `prompt_eval_count` /
`prompt_eval_duration`. The chat panel renders a tiny footer
under completed assistant messages — `<n> tokens · <r> tok/s` —
with the prompt-eval breakdown carried in the hover title;
cancelled / truncated / errored streams suppress the footer
because they have no authoritative metrics. Slice D10 narrowed
the D8 attachment to an optional line range: the read-only
inspector's editor tracks the user's text selection and reports
1-based line numbers; the chat panel's attach button flips to
"Attach selection" when a non-empty selection exists and emits
`startLine` / `endLine` on the `chat.send` payload. The backend
slices the redacted content to that range AFTER redaction (so
secrets outside the range never appear and the range can't be
used to dodge redaction inside the slice). Slice D11 layered
project-instructions auto-context onto chat: when the trusted
project has a root `AGENTS.md`, the backend reads it through the
same Rust-private `prompts::read::read_for_prompt` path (size
cap, binary detection, hardlink check, redactor) and prepends it
as a `system` message on every send. A broken `AGENTS.md`
(oversize / binary / hardlink) skips silently; a new
`instructionsIncluded: boolean` on the synchronous
`chat.send` response confirms what landed. The chat header
renders a small `¶ AGENTS.md` badge when the project has one.
Re-read on every send picks up edits to `AGENTS.md` mid-session.
Slice D12 added a read-only `chat.context` IPC alongside
`chat.send`: same trust gate, same prompt-read pipeline, but no
model call — it reports back what AGENTS.md and the optional
attachment would contribute (`originalBytes`, `redactionCount`,
line range when applicable). Attachment rejections that the real
send would surface as a typed `IpcError` come back here as
`attachment.status === 'blocked'` with a stable `reason` code,
so the chat panel can render a small "Context preview" area
area listing both AGENTS.md and the attachment without a blocked
attachment hiding the AGENTS.md side. The two paths share
`prompts::preview_context` so the preview's numbers always match
what the actual send would log.
Slice D13 is CSS/layout polish only — no IPC, no Rust changes.
Once a project is trusted, the global `Plume` hero is hidden so
the compact status strip is the top-of-window identity (the open
form still keeps the hero). The whole window is a fixed canvas
now: `overflow: hidden` on `html`, `body`, `#root`, and
`.plume-shell` prevents page-level scrolling — only the file
listing, the chat transcript, and the inspector editor scroll
internally. The CodeMirror gutter paints `var(--paper)` and
holds a `min-width: 40px` so horizontally-scrolled file content
no longer slides under the line numbers. A new `--radius-small`
token (4px) replaces the hardcoded `3px` corners on small chips
and rows; `--radius-soft` (6px) stays for full panels. UI_STYLE
documents the workspace shell rule for future slices.
The non-streaming `send_chat` adapter is retained
`#[cfg(test)]`-only as a reference implementation. No multi-file
attachments, no `README.md` auto-context, no per-directory
overlays, no patching, no command running, no `ollama serve`
auto-start, no tool calls. The Rust backend compiles with 227
cargo tests passing, the TS frontend typechecks, and
`./scripts/verify.sh` passes with clippy clean. The agent loop,
file writes, and the patch flow are not implemented yet. See
`docs/DEVELOPMENT.md` for working with the current slice and
`docs/IPC_ROADMAP.md` for what's reserved.

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
      agent/AgentWorkspace.tsx       center-zone shell — banner (D6) + ChatPanel (D7) + mode cards
      chat/                          ChatPanel + useChat (D7 read-only chat)
      editor/ReadOnlyEditor.tsx      CodeMirror display surface
      file-tree/FileBrowser.tsx      useFileNavigator + Navigator + Inspector
      model-picker/                  useSelectedModel + SelectedModelBanner (D6)
      providers/ProviderPanel.tsx    provider registry + reachability + Select button (D6)
      system/                        SystemChips + useSystemSnapshot (D5)
    lib/api/                    typed Tauri-invoke wrappers
    styles/                     tokens.css, ink.css, layout.css
  src-tauri/                    Rust backend (Tauri)
    Cargo.toml
    tauri.conf.json
    capabilities/default.json   narrowed to core:event:default
    src/
      main.rs
      chat/                     D7 + D7.1 read-only chat transport (Ollama only, streaming + cancel)
      prompts/                  D8 Rust-private prompt-read + redactor + assemble (no IPC verb)
      commands/                 IPC handlers
      project/                  project open + persisted trust
      fs/                       sandboxed display reads
      providers/                static registry + TCP reachability
      system/                   host machine introspection (D5)
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
