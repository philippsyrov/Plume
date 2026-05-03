# Architecture

Plume is a Tauri 2 application. The UI is a React 19 + CodeMirror 6
frontend served from Vite during development and bundled at release. The
backend is a Rust crate that owns every operation that touches the
filesystem, processes, the network, or git.

## Process model

```
+----------------------- Plume window -----------------------+
|                                                            |
|  WebView (system, not Chromium)                            |
|  +- React tree                                             |
|  |   +- EditorPane     CodeMirror 6                        |
|  |   +- FileTree       project files + AI-read marker      |
|  |   +- AIPanel        chat / propose-diff / agent control |
|  |   +- DiffViewer     unified diff render                 |
|  |   +- TerminalPane   approved command output             |
|  |   +- StatusStrip    provider, model, memory, git state  |
|  |   +- Settings, Modal, Tooltip primitives                |
|  |                                                         |
|  | <-> Tauri IPC (typed commands + events)                 |
|  |                                                         |
|  Rust backend (single tauri::Builder)                      |
|  +- project    open folder; read AGENTS.md, README;        |
|  |             detect package manager; git status          |
|  +- fs         sandboxed reads/writes inside project root  |
|  +- git        status, diff, checkpoint, branch info       |
|  +- providers  trait + adapters (mlx_lm, ollama,           |
|  |             lmstudio, llamacpp, ...)                    |
|  +- prompts    build final model prompts from ChatRequest  |
|  +- process    spawn/stop provider processes Plume owns    |
|  +- safety     path + command validation, approval ledger  |
|  +- patch      parse + validate + apply unified diffs      |
|  +- commands   approved shell command runner               |
|  +- settings   persisted app config (TOML / SQLite)        |
|                                                            |
+------------------------------------------------------------+
```

## Boundary rules

The frontend never touches the disk, never spawns processes, never opens
a network socket on its own (in production builds; see CSP note below).
All side effects flow through Rust.

| Concern                | Owner       | Notes                                       |
| ---------------------- | ----------- | ------------------------------------------- |
| Read project files     | Rust `fs`   | Validates path is inside project root       |
| Write project files    | Rust `fs`   | Only via approved patch or explicit IPC     |
| Run shell commands     | Rust `cmd`  | Per-command approval; output streamed       |
| Talk to model runtime  | Rust `prov` | Streams tokens back as Tauri events         |
| Build model prompts    | Rust `prompts` | Frontend sends a `ChatRequest`; never raw file content |
| Persist settings       | Rust `set`  | TOML in OS app data dir                     |
| Hold UI state          | React       | Lives in the window; never persisted via JS |

### Prompt assembly is backend-only

The frontend builds a `ChatRequest` with file paths and a free-form
instruction. The Rust `prompts` module reads file content via `fs.read`,
runs the secret redactor, and assembles the final prompt for the
provider. Raw file content never leaves the backend in either direction
— it goes from disk, through the redactor, into the provider's HTTP
body, and the only thing the frontend sees is the `chat.token` stream
that comes back. This is what keeps the secret redactor a single
chokepoint.

The frontend `lib/prompts/` module is for **prompt UI helpers**
(message-template chooser, attachment picker, mode hints) — it does not
assemble model prompts.

### CSP profiles

The dev `tauri.conf.json` allows `connect-src http://localhost:*
http://127.0.0.1:*` so Vite HMR can talk to the WebView. Production
builds must drop the localhost allowance so a prompt-injection-driven
fetch cannot exfiltrate to a local proxy. When the prod build pipeline
lands, this lives in `tauri.prod.conf.json` (Tauri 2 profile override),
not as a JSON comment in the shared config.

## IPC contract

The full typed surface — error model, IDs, cancellation, event
sequencing, and per-command shapes — lives in `docs/IPC_CONTRACT.md`.
This document is the architecture overview; that document is the
contract.

Every IPC handler goes through `safety::guard` first. The guard rejects
requests that violate sandbox rules and writes an entry to the session
log.

## State and storage

- Per-project session lives in memory while the window is open.
- Persistent settings: TOML file inside the OS app data directory (e.g.
  `~/Library/Application Support/dev.plume.app/config.toml` on macOS).
- Approval ledger: `<project>/.plume/approvals.toml` (gitignored).
- Optional SQLite for session transcripts and provider metadata cache,
  deferred until the app actually needs it.
- Plume-managed project files live under `<project>/.plume/` and are
  gitignored by default.

## Data flow for a scoped edit

1. User selects file(s) and types an instruction.
2. Frontend builds a `ChatRequest` referencing files by path.
3. Backend `prompts` module loads requested files through `fs.read`,
   applies secret redaction, assembles the final model prompt.
4. Backend forwards the prompt to the active provider with a
   `CancellationToken`.
5. Provider streams tokens back; backend emits `chat.token` events with
   monotonic `seq`.
6. When the model emits a unified diff, frontend asks `patch.validate`.
7. On user approval, frontend calls `patch.apply`.
8. Backend writes files, refreshes git status, emits an event so the UI
   updates.

## Module list (planned)

Frontend (`src/`):

- `app/` shell layout, providers, theme
- `app/ink/` `InkButton`, `InkPanel`, `InkBadge`, ... visual primitives
- `features/editor/` CodeMirror integration
- `features/file-tree/`
- `features/ai-panel/`
- `features/diffs/`
- `features/terminal/`
- `features/model-picker/`
- `features/settings/`
- `lib/api/` typed wrappers around Tauri invoke
- `lib/context/` UI helpers for picking attachments/scope
- `lib/prompts/` UI helpers for message templates and mode hints
- `lib/models/` registry helpers

Backend (`src-tauri/src/`):

- `main.rs`
- `commands/` IPC handlers, thin wrappers
- `project/`
- `fs/`
- `git/`
- `providers/{trait, mlx_lm, ollama, lmstudio, llamacpp}.rs`
- `prompts/` — final prompt assembly, secret redaction integration
- `process/`
- `safety/`
- `patch/`
- `settings/`

## Why Tauri, not Electron

A 16 GB Mac running a 4-7 B parameter local model has very little
headroom for a bundled Chromium. Tauri reuses the system WebView and
keeps the Rust binary small, which leaves memory available for the
model the user actually cares about. Stack decision, not preference —
see `AGENTS.md` rule #1.
