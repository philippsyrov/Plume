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
|  |   +- Unified sidebar project and chat navigation        |
|  |   +- Top bar with model control and workspace views     |
|  |   +- One active Chat, Files, Browser, Library, or       |
|  |   |   Benchmarks workspace                               |
|  |   +- Settings categories, dialogs, and Details views    |
|  |                                                         |
|  | <-> Tauri IPC (typed commands + events)                 |
|  |                                                         |
|  Rust backend (single tauri::Builder)                      |
|  +- project    open folder; detect AGENTS.md/CLAUDE.md and |
|  |             package manager; git status only after the  |
|  |             user grants project trust                   |
|  +- fs         guarded reads inside project root            |
|  +- git        diff checkpoint and revert support           |
|  +- providers  fixed catalog + adapters (Apple, mlx_lm,    |
|  |             ollama, lmstudio, llamacpp, ...)            |
|  +- prompts    build final model prompts from ChatRequest  |
|  +- process    spawn/stop provider processes Plume owns    |
|  +- safety     path validation and approval boundaries      |
|  +- patch      parse + validate + apply unified diffs      |
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
| Run shell commands     | Not shipped | No production command executor exists        |
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
thing the frontend sees is the `chat/token` stream that comes back.
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

## Unified workspace shell

Trusted-project and projectless work now share one consumer shell. The left
sidebar owns New chat, Search, Library, project/session rows, Settings, and
Help. It can collapse without changing the active task. The main side owns one
quiet top bar — current surface title, selected-model control, project switch,
and Workspace views when relevant — above exactly one active surface.

The active surface is explicit rather than a permanently crowded three-column
dashboard:

| Surface | Current shape |
| --- | --- |
| Chat / Project | One conversation and composer. Local Chat has no project authority; Project can use trusted project context and reviewed patch actions. |
| Files | Project navigator beside the read-only inspector. The shared `useFileNavigator(projectRoot)` state keeps selection and line range exact. |
| Browser | The owning chat beside its native WebKit page, with a resizable split or expanded Browser canvas. |
| Library | Source tree, scoped index, and reading/detail canvas. Mutations remain in Settings. |
| Benchmarks | Trusted read-only benchmark evidence viewer. |

Providers, local-model controls, Library editing, and advanced project tools
live in Settings. Agent configuration and the single-step MLX proof live in
the **Advanced** Settings category; the scripted developer dry-run is not a
production Settings surface. Technical project facts and
prompt manifests remain available in their owning **Details** disclosures
instead of forming a permanent status strip.

Selected-model state is window-local React state shared by the top-bar picker,
Settings panels, Chat, and advanced single-step controls. It is owned by the
window, not by the project, so it survives `project.close` and carries into
projectless chat — a model chosen while a project was open stays chosen after
the project closes. There is no backend model-selection persistence yet, so it
resets on relaunch.

Propose-diff replies validate automatically. A valid diff can be written only
through the user's explicit **Apply** action, which re-validates, checkpoints,
and writes atomically. **Revert** drift-checks that checkpoint before restoring
it. No chat reply, Browser page, or agent event applies its own proposal.

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
- Approval ledger: `<project>/.plume/approvals.json` (gitignored; JSON
  with epoch-ms timestamps — see `docs/SAFETY.md § Approval ledger`).
- Chat sessions (D63A): SQLite, one schema in two physically separate
  databases — local chats in `<app data>/sessions/state.sqlite`,
  project chats in `<trusted project>/.plume/sessions/state.sqlite`.
  Access is Rust-only (`sessions/`); the frontend never receives a
  database path. Provider metadata caching remains deferred.
- App-private user memory: bounded redacted JSONL at
  `<app data>/memory/entries.jsonl`. The backend resolves this path once at
  startup; IPC callers cannot supply it, and it is physically separate from
  project `.plume/memory`. The backend/API floor is CRUD + text search plus
  explicit `userMemoryEntry` resolution; it never adds user memory to prompts
  automatically. Reloads validate
  every JSONL row and hard invariant before use; a process-local mutex plus a
  fail-closed Unix advisory lock serialize access across app instances. The
  opened lock inode is forced to mode `0600`, and `entries.jsonl` is rejected
  by metadata before a bounded 64-KiB read (with a cap-plus-one growth check).
  Prompt assembly receives this backend-owned directory separately from the
  optional trusted project root, so each context kind can read only its owner.
- Plume-managed project files live under `<project>/.plume/` and are
  gitignored by default.

## Data flow for streaming chat

1. User picks a running model from the top bar or Settings; the selection is
   carried in window-local React state.
2. The user may place an eligible file or exact selection on the visible
   context shelf with **Use current file in chat** or **Use selection in chat**.
   Library and Browser add their own typed opaque refs through the same shelf.
   Binary, oversize, blocked, stale, and wrong-scope sources cannot attach or
   send.
3. User types a prompt in the chat panel. Frontend **mints a fresh
   `ChatStreamId`** with `mintStreamId()` (`crypto.randomUUID()`,
   with a timestamp+random fallback), then subscribes to the
   `chat/token` / `chat/done` events filtered by that id. Client-
   minted ids are how D7.1 closes the subscribe-before-send race —
   Tauri events are not replayed.
4. After listeners are live the frontend builds a `ChatSendPayload` with the
   stream/provider/model ids, visible transcript, ordered `contextSources`,
   exact session owner, and an explicit local-versus-project context flag. The
   frontend sends references, not source bodies.
5. Backend validates the payload and resolves every context ref through its
   owning app-private or trusted-project store. Project files run through
   `prompts::read::read_for_prompt` (secret-filename block, prompt-read `.git/`
   whitelist, size cap, binary block, hardlink check) and
   `prompts::redact` (content-pattern redaction). Preview and send share the
   same bounded resolution path and send returns the exact accepted manifest.
   Errors here (`Blocked`, `NotFound`, `PathEscape`,
   `NeedsApproval`) reject synchronously before a stream id is
   registered. The backend then registers the client-minted id in
   `AppState::chat_streams` (rejecting a duplicate with
   `BadArgument`), spawns the blocking streaming task, and the
   IPC call returns `{ streamId, providerId, modelId }`
   immediately.
6. The task dispatches to the selected adapter: Ollama reads bounded NDJSON,
   MLX-LM/MLX-VLM reads bounded OpenAI-style SSE from a Plume-owned loopback
   server, and Apple reads bounded JSON lines from the per-generation bundled
   helper.
   Every route polls its cancellation boundary; Apple has no server handle or
   localhost port and never falls back to Qwen.
7. For each provider delta the task emits a `chat/token` event with
   the per-frame `delta` and a monotonic `seq`. The frontend's
   `useChat` listener enforces sequencing (drop duplicates, buffer
   out-of-order, mark corrupt on a gap) and appends each in-order
   delta to the in-progress assistant entry.
8. When the adapter reaches its terminal record, the cancel flag trips, or the
   transport closes early, the task emits exactly one
   `chat/done` event with the `finish` reason and removes its
   entry from `chat_streams`. Frontend flips the streaming entry
   to its terminal shape (finalised assistant message, cancelled
   marker, or error row).
9. `chat.cancel(streamId)` is the user's Stop button. It sets the cancel flag;
   the adapter stops forwarding tokens and closes or kills its owned transport.
   Cancellation is bounded and cooperative, so an already-buffered delta may
   arrive before the adapter observes it.

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
5. Provider streams tokens back; backend emits `chat/token` events with
   monotonic `seq`.
6. When the model emits a unified diff, frontend asks `patch.validate`.
7. On user approval, frontend calls `patch.apply`.
8. Backend writes files, refreshes git status, emits an event so the UI
   updates.

Steps 3–8 are shipped for the patch-only path. Every finalised propose-diff
reply is validated automatically. **Apply** stays unavailable until validation
passes, then the explicit click invokes `patch.apply`; **Revert** invokes
`patch.revert` against the recorded checkpoint. This does not graduate the
planned scoped-edit loop into arbitrary writes or command execution. D7's
non-streaming `chat::ollama::send_chat` remains `#[cfg(test)]`-only because the
streaming adapters are the production callers.

## Current module ownership

The detailed, maintained maps are
[`src/features/README.md`](../src/features/README.md) for frontend surfaces and
[`src-tauri/src/README.md`](../src-tauri/src/README.md) for Rust domains and IPC
seams. The short architecture view is:

- `src/App.tsx` owns window routing and shared consumer-shell state;
  `src/features/` owns Browser, Library, sessions, chat/explicit context,
  Files, Settings/appearance/help, providers, skills, and benchmark surfaces;
  `src/lib/api/` owns typed Tauri wrappers.
- `src-tauri/src/lib.rs` owns application construction and shared state;
  `app_commands.rs` owns the hand-maintained application-command allowlist
  source that `build.rs` consumes to generate the build-time manifest;
  `commands/` is the thin IPC edge.
- `project/` and `safety/` own trust and path boundaries. `prompts/` owns
  backend-only context resolution, redaction, and exact manifests; display
  reads in `fs/` cannot enter that path.
- `sessions/` owns the physically separate local/project SQLite stores and
  persisted Browser descriptors. `browser/` owns native runtime policy,
  restoration, and session-owned evidence; the `task_browser*` handlers bind
  live children to an exact session identity.
- `memory/` owns project and app-private stores without merging their
  authority. `patch/` owns the only shipped model-proposed write/revert path.
  `providers/` owns the fixed app-level catalog, verified Qwen Coder/Qwen2-VL
  installations, Apple helper availability, runtime discovery, and the
  Plume-managed MLX supervisor. Qwen Coder and Qwen2-VL weights live in app data; the
  Apple helper and MLX-LM/MLX-VLM runtime are generated release resources.
- `agent/` contains the patch-only single-step and guarded foundations, not a
  broad executor. `skills/` owns trusted project skills.

Tests are colocated or in named sibling `*_tests` modules and are linked from
the two domain maps. IPC wire truth remains in `docs/IPC_CONTRACT.md`.

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
