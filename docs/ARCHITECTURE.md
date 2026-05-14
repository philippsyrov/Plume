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
|  +- project    open folder; detect AGENTS.md/CLAUDE.md and |
|  |             package manager; git status only after the  |
|  |             user grants project trust                   |
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
instruction. The Rust `prompts::assemble(ChatRequest) -> AssembledPrompt`
function reads file content via a Rust-private helper, runs the secret
redactor, and assembles the final prompt for the provider. Raw file
content never leaves the backend in either direction — it goes from
disk, through the redactor, into the provider's HTTP body, and the only
thing the frontend sees is the `chat.token` stream that comes back.
This is what keeps the secret redactor a single chokepoint.

The frontend `lib/prompts/` module is for **prompt UI helpers**
(message-template chooser, attachment picker, mode hints) — it does not
assemble model prompts.

#### Display reads vs prompt reads

The split between display and prompt paths is enforced in three places:

| Layer         | Display path                                          | Prompt path                                                                       |
| ------------- | ----------------------------------------------------- | --------------------------------------------------------------------------------- |
| IPC           | `fs.read(path)` returns display content               | No IPC verb. `chat.send`'s optional `attachment` is the only entry                |
| Rust function | `fs::read::read_file(root, target) -> FileContent`    | `prompts::read::read_for_prompt(root, target, relPath)` (private to `prompts::`)  |
| Type          | `FileContent` (`src-tauri/src/fs/read.rs`)            | `RedactedContent` (`src-tauri/src/prompts/read.rs`, shipped D8)                   |

`FileContent` and `RedactedContent` are distinct Rust types with no
`From`/`Into` between them. The redactor in `prompts::redact` is the
only producer of `RedactedContent`, and the reader's visibility is
`pub(in crate::prompts)` so no module outside `prompts::` can
construct one — the boundary is enforced at the type level.

`fs.read` (and `FileContent`) exist for the editor, the file tree, the
diff viewer, and similar display surfaces. Its return value cannot
be fed into the prompt pipeline; the compiler rejects it.

### CSP profiles

The dev `tauri.conf.json` allows `connect-src http://localhost:*
http://127.0.0.1:*` so Vite HMR can talk to the WebView. Production
builds must drop the localhost allowance so a prompt-injection-driven
fetch cannot exfiltrate to a local proxy. When the prod build pipeline
lands, this lives in `tauri.prod.conf.json` (Tauri 2 profile override),
not as a JSON comment in the shared config.

### Provider track vs engine track

Plume's model integration runs on two parallel tracks:

- **Provider track.** Plume drives an LLM endpoint directly through
  the `Provider` trait (MLX-LM, llama.cpp, Ollama, LM Studio). Plume
  owns prompt assembly, the tool-call loop, diff handling, and the
  agent surface on top.
- **Engine track (planned, not implemented).** Plume embeds an
  external agent runtime such as Codex CLI, Claude Code, or
  OpenCode. The engine owns its agent loop; Plume is the cockpit —
  editor, file tree, safety gates, approval UI, project context.

The boundary above does not change between tracks. Whether the work
is driven by Plume's own provider stack or delegated to an embedded
engine, every disk read, file write, command run, and patch apply
still flows through Rust and through `safety::guard`. The engine
track is described in `docs/MODEL_PROVIDERS.md § External agent
engines`; no engine code, IPC, or trait shape is committed yet.

## Trusted-project workspace shell

When a project has been trusted, the React shell renders a three-zone
workspace below the project status strip. The split is intentional:
each zone has a stable role even before the agent loop lands.

| Zone   | Width                                | Contents                                                              |
| ------ | ------------------------------------ | --------------------------------------------------------------------- |
| Left   | 260 px (D30 resizable)               | `FileNavigator` + `ProvidersPanel` (reachability) + `LocalModelsPanel`; D32 each toggleable from the column chip strip |
| Center | flexible (`minmax(0, 1fr)`)          | `AgentWorkspace` — placeholder for chat / propose-diff / scoped-edit / agent-loop |
| Right  | 340 px (D30 resizable)               | `FileInspector` (header + read-only CodeMirror or empty placeholder); D32 toggleable from the column chip strip |

The navigator and inspector share state through a single
`useFileNavigator(projectRoot)` hook so a click in the navigator is
reflected in the inspector without prop drilling.

The center zone hosts a "Selected model" banner — D6's window-local
model picker (see `features/model-picker/useSelectedModel.ts`) — and
below it the read-only chat surface that landed across D7–D16. The
four mode cards (`chat`, `propose-diff`, `scoped-edit`, `agent-loop`)
still sit under the chat panel as a map of the safety modes from
`docs/SAFETY.md`. As of D16 the `chat` card is labeled
"shipped (read-only)", the `propose-diff` card is labeled
"preview only — apply not yet" (D15 renders model-emitted diffs,
D16 layered a read-only `patch.validate` IPC that shows a
"valid diff · N files · M hunks" or "invalid diff: <reason>" pill
under the rendered diff — but the Apply button stays disabled, no
on-disk writes), and `scoped-edit` plus `agent-loop` stay labelled
"not yet implemented". Selected-model state is owned by
`TrustedView`, set by the Select button on each model row in
`ProvidersPanel`, and read by `AgentWorkspace`. Closing the project
drops the selection; there is no backend persistence yet. Future
slices grow real controls (`patch.apply` / approval surfaces /
agent-loop progress) under the same accessible names rather than new
hidden surfaces.

The shell collapses gracefully at the configured 900 px window minimum
(see `src-tauri/tauri.conf.json`). A user-resizable split lands in a
later slice.

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

## Data flow for read-only chat (D7.1 + D8, shipping)

1. User picks a model in the provider panel; the selection is
   carried in window-local React state (D6).
2. (Optional D8) User picks a file in the inspector and clicks
   "Attach current file" in the chat panel. A visible chip records
   the project-relative path; binary / oversize / blocked
   selections cannot attach.
3. User types a prompt in the chat panel. Frontend **mints a fresh
   `ChatStreamId`** with `mintStreamId()` (`crypto.randomUUID()`,
   with a timestamp+random fallback), then subscribes to the
   `chat.token` / `chat.done` events filtered by that id. Client-
   minted ids are how D7.1 closes the subscribe-before-send race —
   Tauri events are not replayed.
4. After listeners are live the frontend builds a
   `ChatSendPayload` with `{ streamId, providerId, modelId,
   messages: [...], attachment? }` where `messages` is the full
   visible transcript and `attachment`, when present, references
   the file by project-relative path only — no bytes cross IPC.
5. Backend validates the payload. If `attachment` is set it also
   requires a trusted open project, runs
   `prompts::assemble`, which calls
   `prompts::read::read_for_prompt` (secret-filename block,
   prompt-read `.git/` whitelist, size cap, binary block,
   hardlink check) and `prompts::redact` (content-pattern
   redaction), and folds the result into the last user message.
   Errors here (`Blocked`, `NotFound`, `PathEscape`,
   `NeedsApproval`) reject synchronously before a stream id is
   registered. The backend then registers the client-minted id in
   `AppState::chat_streams` (rejecting a duplicate with
   `BadArgument`), spawns the blocking streaming task, and the
   IPC call returns `{ streamId, providerId, modelId }`
   immediately.
6. The task runs `chat::ollama::stream_chat`, which POSTs
   `/api/chat` with `stream: true` to localhost Ollama and reads
   the NDJSON body line by line. Between line reads it polls the
   cancel flag (~200 ms cadence).
7. For each NDJSON frame the task emits a `chat.token` event with
   the per-frame `delta` and a monotonic `seq`. The frontend's
   `useChat` listener enforces sequencing (drop duplicates, buffer
   out-of-order, mark corrupt on a gap) and appends each in-order
   delta to the in-progress assistant entry.
8. When the runtime emits a `done: true` frame, or the cancel flag
   trips, or the socket closes early, the task emits exactly one
   `chat.done` event with the `finish` reason and removes its
   entry from `chat_streams`. Frontend flips the streaming entry
   to its terminal shape (finalised assistant message, cancelled
   marker, or error row).
9. `chat.cancel(streamId)` is the user's Stop button. It sets the
   cancel flag; the streaming task notices on its next poll and
   exits cleanly. Cancellation is best-effort — one more buffered
   NDJSON frame may still appear before the loop notices the flag.

## Data flow for a scoped edit (planned, not implemented)

1. User selects file(s) and types an instruction.
2. Frontend builds a `ChatRequest` referencing files by path.
3. Backend `prompts::assemble` loads requested files through the
   Rust-private `prompts::read::read_for_prompt` path — the redactor
   is the only producer of `RedactedContent` — and builds the final
   model prompt. IPC `fs.read` is not used here; that verb is for
   the editor and other display surfaces only.
4. Backend forwards the prompt to the active provider with a
   `CancellationToken`.
5. Provider streams tokens back; backend emits `chat.token` events with
   monotonic `seq`.
6. When the model emits a unified diff, frontend asks `patch.validate`.
7. On user approval, frontend calls `patch.apply`.
8. Backend writes files, refreshes git status, emits an event so the UI
   updates.

Steps 3 (prompt assembly), 5 (token streaming) shipped across
D7.1–D11. Step 6 (`patch.validate`) shipped in D16: the chat panel
runs every finalised propose-diff reply through the validator and
renders a `valid diff · N files · M hunks` or `invalid diff:
<reason>` pill under the rendered diff — but the Apply button
stays disabled, so steps 7 (`patch.apply`) and 8 (write +
refresh) are still roadmap. D7's `chat::ollama::send_chat`
covers step 4 in its non-streaming form (retained as
`#[cfg(test)]`-only since D7.1 made the streaming path the only
production caller).

## Module list (planned)

Frontend (`src/`):

- `app/` shell layout, providers, theme
- `app/ink/` `InkButton`, `InkPanel`, `InkBadge`, ... visual primitives
- `features/editor/` CodeMirror integration
- `features/file-tree/` `useFileNavigator` hook + `FileNavigator` and
  `FileInspector` zone renderers
- `features/agent/` `AgentWorkspace` — header, selected-model banner
  (D6), `ChatPanel` (D7), and the mode-card grid; grows real prompt
  / mode / streaming controls in later slices
- `features/chat/` `ChatPanel` + `useChat` hook — the D7 read-only
  chat surface and its window-local transcript
- `features/providers/` provider registry + reachability panel + the
  per-model Select button (D6)
- `features/model-picker/` `useSelectedModel` hook +
  `SelectedModelBanner` — window-local selection state today; the
  typed/persisted version lands with `session.setSelectedModel`
- `features/system/` `SystemChips` + `useSystemSnapshot` polling hook
- `features/diffs/`
- `features/terminal/`
- `features/settings/`
- `lib/api/` typed wrappers around Tauri invoke
- `lib/context/` UI helpers for picking attachments/scope
- `lib/prompts/` UI helpers for message templates and mode hints
- `lib/models/` registry helpers

Backend (`src-tauri/src/`):

- `main.rs`
- `commands/` IPC handlers, thin wrappers (`chat`, `fs`, `project`,
  `providers`, `system`)
- `project/`
- `fs/`
- `git/`
- `chat/{mod, ollama, stream}.rs` — D7.1 streaming chat transport.
  Today's scope is Ollama via `/api/chat` with `stream:true` and a
  cooperative cancel flag (`ChatStreamRegistry` in `stream.rs`).
  Additional adapters (LM Studio, llama.cpp) sit behind the same
  IPC verbs when they land. The non-streaming `send_chat` adapter
  is retained `#[cfg(test)]`-only as a reference implementation.
- `prompts/{mod, assemble, read, redact}.rs` — D8 prompt assembly.
  `assemble` is the public surface called from `commands::chat`;
  `read` is the Rust-private prompt-read path that produces
  `RedactedContent`; `redact` carries the secret content patterns.
  No IPC verb is exposed here.
- `providers/{registry, health, http, ollama, openai_compat, fit}.rs`
  + future `{trait, mlx_lm}.rs`
- `system/` — host machine introspection (RAM, swap, load average,
  machine labels) for the fit estimator and the trusted-project
  status strip. macOS reader shells out to `sysctl` / `vm_stat`;
  other platforms return `None`s.
- `prompts/` — final prompt assembly, secret redaction integration
- `process/`
- `safety/`
- `patch/`
- `settings/`

## Reserved for later

If sub-agents land, write isolation should use git worktrees: each
sub-agent gets a temporary worktree at the same commit, edits there,
and the parent merges or discards. No IPC fields, no schemas, no
provider trait changes are committed today — this paragraph reserves
the concept so we don't paint ourselves into a corner.

## Why Tauri, not Electron

A 16 GB Mac running a 4-7 B parameter local model has very little
headroom for a bundled Chromium. Tauri reuses the system WebView and
keeps the Rust binary small, which leaves memory available for the
model the user actually cares about. Stack decision, not preference —
see `AGENTS.md` rule #1.
