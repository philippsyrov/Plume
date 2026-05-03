# Architecture

Plume is a Tauri 2 application. The UI is a React 19 + CodeMirror 6 frontend
served from Vite during development and bundled at release. The backend is a
Rust crate that owns every operation that touches the filesystem, processes,
the network, or git.

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
|  +- process    spawn/stop provider processes Plume owns    |
|  +- safety     path + command validation, approval ledger  |
|  +- patch      parse + validate + apply unified diffs      |
|  +- commands   approved shell command runner               |
|  +- settings   persisted app config (TOML / SQLite)        |
|                                                            |
+------------------------------------------------------------+
```

## Boundary rules

The frontend never touches the disk, never spawns processes, never opens a
network socket on its own. All side effects flow through Rust.

| Concern                | Owner       | Notes                                       |
| ---------------------- | ----------- | ------------------------------------------- |
| Read project files     | Rust `fs`   | Validates path is inside project root       |
| Write project files    | Rust `fs`   | Only via approved patch or explicit IPC     |
| Run shell commands     | Rust `cmd`  | Per-command approval; output streamed       |
| Talk to model runtime  | Rust `prov` | Streams tokens back as Tauri events         |
| Persist settings       | Rust `set`  | TOML in OS app data dir                     |
| Hold UI state          | React       | Lives in the window; never persisted via JS |

## IPC contract (planned)

Tauri commands are typed end-to-end. The Rust side defines a function with
`#[tauri::command]`; the TS side imports a generated invoke wrapper. The
contract is small on purpose:

```
# project
project.open(path: string)        -> ProjectMeta
project.refresh()                 -> ProjectMeta

# fs
fs.read(path: string)             -> { content, encoding }
fs.list(path: string)             -> FileEntry[]

# git
git.status()                      -> GitStatus
git.diff(path: string)            -> string
git.checkpoint(label: string)     -> { stash: string }

# providers
providers.list()                  -> ProviderInfo[]
providers.installed(id)           -> boolean
providers.startServer(id)         -> ServerHandle
providers.stopServer(handle)      -> void

# chat
chat.send(req: ChatRequest)       -> ChatStreamId
# events: 'chat.token' { id, token }
#         'chat.done'  { id, finish_reason }

# patches
patch.validate(diff: string)      -> { ok: true } | { ok: false, errors }
patch.apply(diff: string)         -> { applied: string[] }

# commands
commands.detect()                 -> CommandSuggestion[]
commands.run(cmd, approved)       -> RunHandle
# events: 'cmd.line' { handle, stream, line }
#         'cmd.exit' { handle, code }
```

Every IPC call goes through `safety::guard` first. The guard rejects
requests that violate sandbox rules and writes an entry to the session log.

## State and storage

- Per-project session lives in memory while the window is open.
- Persistent settings: TOML file inside the OS app data directory (e.g.
  `~/Library/Application Support/dev.plume.app/config.toml` on macOS).
- Optional SQLite for session transcripts and provider metadata cache,
  deferred until the app actually needs it.
- Plume-managed project files live under `<project>/.plume/` and are
  gitignored by default.

## Data flow for a scoped edit

1. User selects file(s) and types an instruction.
2. Frontend builds a `ChatRequest` referencing files by path.
3. Rust loads requested files through `fs.read`, applying secret redaction.
4. Rust forwards a model-specific prompt to the active provider.
5. Provider streams tokens back; backend forwards them as `chat.token`
   events.
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
- `lib/context/` context packet builder
- `lib/prompts/` model-specific prompt templates
- `lib/models/` registry helpers

Backend (`src-tauri/src/`):

- `main.rs`
- `commands/` IPC handlers, thin wrappers
- `project/`
- `fs/`
- `git/`
- `providers/{trait, mlx_lm, ollama, lmstudio, llamacpp}.rs`
- `process/`
- `safety/`
- `patch/`
- `settings/`

## Why Tauri, not Electron

A 16 GB Mac running a 4-7 B parameter local model has very little headroom
for a bundled Chromium. Tauri reuses the system WebView and keeps the Rust
binary small, which leaves memory available for the model the user actually
cares about. This is a stack decision, not a preference — see `AGENTS.md`
rule #1.
