# IPC Roadmap

Planned IPC names only. **Not part of the v1 contract.** Names and
shapes will change. Do not type the frontend wrapper against anything
in this file — the contract lives in `docs/IPC_CONTRACT.md`.

This file exists so we don't quietly drift between the runtime ideas in
`docs/AGENT_RUNTIME.md` / `docs/CLAUDE_CODE_REFERENCE_NOTES.md` and the
shipping IPC. When a name moves out of this file into `IPC_CONTRACT.md`
it acquires a stable schema, an error model, and a place in the typed
frontend wrapper. Until then it is a placeholder for a conversation,
not a commitment.

## Permission ledger

- `permissions.list`
- `permissions.grant`
- `permissions.revoke`
- `permissions.check`

## Session mode and policy

- `session.setMode`           — flip `agentMode` (see `docs/SAFETY.md`)
- `session.setApprovalPolicy` — flip `approvalPolicy`
- `session.setAllowlist`      — replace `fileAllowlist` / `commandAllowlist`
- `session.state`             — read current mode, policy, allowlists
- `session.setSelectedModel`  — pin `{ providerId, modelId }` for the session
- `session.clearSelectedModel`
- `session.setUiMode`         — set `'simple' | 'developer'` for the
                                trusted-project shell render
- `session.uiMode`            — read current ui mode

Until these land the v1 session is locked to `approvalPolicy:
'ask-each'` with empty allowlists, regardless of `agentMode`. D6
shipped the model picker as window-local React state in
`features/model-picker/useSelectedModel.ts` — selection is hoisted
in `TrustedView` and dropped when the project closes. A typed,
persisted version goes through `session.setSelectedModel` once the
session module lands.

### UI mode (Simple vs Developer)

The `uiMode` axis is described in `docs/PLUME_PROJECT_SPEC.md §
7.7`, the visual rules in `docs/UI_STYLE.md § Simple Mode vs
Developer Mode`, and the accessibility contract in
`docs/AGENT_OPERABILITY.md § Mode toggle`. The IPC surface is
deliberately small because Simple and Developer render the
**same** chat, attachment, propose-diff, validate, and context
IPC; they differ only in what the React tree renders.

The first implementation slice ships `uiMode` as window-local
React state in `TrustedView`, the same pattern D6 used for the
selected model. Default is `'simple'` on every project open; the
mode lives in a hook in `features/ui-mode/` (name TBD) and is
hoisted alongside `useSelectedModel`. Frontend-only state means
the slice does not touch the IPC contract, the trust ledger, or
the project schema — it is purely a renderer choice.

The state graduates to IPC when persistence lands:

- `session.setUiMode({ mode: 'simple' | 'developer' })` —
  records the mode against the active project. The storage
  surface is `<project>/.plume/` (per `docs/ARCHITECTURE.md`'s
  Plume-managed project files convention), not the OS app-data
  trust store, because the mode is a project-scoped UX
  preference rather than a security state.
- `session.uiMode` — reads the persisted value at project
  open. Returns `'simple'` if no record exists (the default
  remains Simple for first-time users).

The verbs are reserved here so the eventual graduation doesn't
collide with another `session.*` name. Neither is implemented
today.

Out of scope:

- `session.uiMode` does NOT take a project id as input — it
  reads the active session. Cross-project mode introspection is
  not a goal; each project gets its own record.
- The verb does NOT push a `uiMode.changed` event. The
  frontend already knows when the user flipped the toggle; the
  graduation only adds durability, not a new event surface.
- The mode does NOT affect any other IPC. Chat, patch,
  providers, system, project, fs all behave identically in both
  renders. A future slice that tried to gate an IPC behind UI
  mode would be violating the "two renders, one IPC" rule that
  this section pins.

## Chat sessions

### Landed in D63A

Durable chat sessions behind the `sessions.*` family — `sessions.list`
/ `create` / `load` / `rename` / `archive` / `delete` /
`saveTranscript`. See `docs/IPC_CONTRACT.md § sessions` for the wire
shapes. One SQLite schema in two physically separate databases:
app-data for local chats, `<trusted project>/.plume/sessions` for
project chats. D63B wired the sidebar to it: persisted rows with
rename/archive/delete dialogs, transcript restore on relaunch, and
boundary-only saves (accepted turn + terminal outcome, never per
token).

D66 landed session search: schema v2 (FTS5 over titles + message
content, trigger-maintained, atomic v1→v2 migration) behind
`sessions.search`, wired to a compact overlay (sidebar `Search chats`
/ Cmd+K) with per-scope result sections.

Reserved follow-ups that build on this spine (deliberately not D63/
D66): transcript compaction, a structured event log superseding
snapshot saves, and memory distillation from sessions.

## Project memory

Project memory is not a hidden second instruction channel. It is local,
visible state that helps future Plume sessions orient faster.

### Landed in D37

- `memory.index` — read the current entry list + caps + on-disk
  size. See `docs/IPC_CONTRACT.md § memory`.
- `memory.remember` — add a redacted text entry. Every text passes
  through the same `prompts::redact` redactor the prompt-read
  pipeline uses; secrets surface as `[REDACTED:<kind>]` markers.
- `memory.forget` — remove one entry by opaque id; idempotent.

D37 stores entries as JSONL at
`<project>/.plume/memory/entries.jsonl`. Hard caps: 100 entries,
1 KiB per entry, 64 KiB total. The store is gated by the same
trusted-project check the patch verbs use; `.plume/` symlinks are
refused before any write or delete; every read/write takes a
process-wide memory mutex so concurrent remembers/forgets don't
lose updates.

### Reserved follow-ups

- `memory.search` — query project/session memory with local text
  search (likely SQLite FTS per `docs/LOCAL_AGENT_NORTH_STAR.md`).
- `memory.distill` — manual consolidation pass over recent session
  logs and memory files.
- Topic files (`USER.md`, `SOUL.md`, `topics/`) as separate
  always-loaded slots alongside the flat entry list.
- Session log replay + append-only logs at
  `.plume/memory/sessions/`.

### Future-slice storage target

```text
.plume/
  memory/
    entries.jsonl       # landed in D37
    INDEX.md            # follow-up
    USER.md             # follow-up
    SOUL.md             # follow-up
    topics/             # follow-up
  sessions/
    state.sqlite        # follow-up
    logs/               # follow-up
```

### Rules pinned now

- `INDEX.md`, `USER.md`, and `SOUL.md` are small and capped.
- Session logs are append-only.
- Memory writes are visible and reversible.
- Secrets are redacted before storage (D37 uses `prompts::redact`).
- Memory never grants file or command permission.
- SQLite FTS is the first search backend; embeddings are a follow-up only
  when Plume has a local embedding path.
- Distillation borrows the Sass pattern: dedupe, prune, cap, compress, and
  write a visible summary of what changed.

## Chat streaming

D7.1 shipped: `chat.send` returns a `ChatStreamId` immediately and
emits `chat/token` (per delta) plus a terminal `chat/done` event.
`chat.cancel(streamId)` flips a cooperative cancel flag. The full
shape is in `docs/IPC_CONTRACT.md § chat`.

D8 shipped: optional `attachment: { kind: 'projectFile', relPath }`
on `chat.send`. Backend resolves through the Rust-private
`prompts::assemble` path (secret-filename block, size cap, binary
block, content redactor) before folding the file into the last
user message. No IPC verb returns prompt-ready content.

D9 shipped: provider-neutral generation telemetry on `chat/done`.
The new `stats` field carries `outputTokens`, `evalMs`,
`tokensPerSecond`, `promptTokens`, `promptMs` — populated from
Ollama's final-frame `eval_count` / `eval_duration` /
`prompt_eval_count` / `prompt_eval_duration` on `finish === 'stop'`.
Additive; old listeners ignore it.

D10 shipped: optional `startLine` / `endLine` on `ChatAttachment`.
Frontend's read-only inspector tracks the user's text selection;
the chat panel's attach control flips between "Attach current
file" and "Attach selection (lines X–Y)". Backend slices the
redacted content to the requested 1-based inclusive range AFTER
the redactor runs, so secrets outside the range never appear.

D11 shipped: `AGENTS.md` auto-context. When a trusted project has
a root `AGENTS.md`, the chat handler prepends it as a `system`
message on every send, read through the Rust-private
`prompts::assemble` path with the same secret-filename / size /
binary / redactor gates as file attachments. A new
`instructionsIncluded: boolean` field on `ChatSendStartedResponse`
confirms per-send whether the file landed. A broken AGENTS.md
(oversize / binary / hardlink) skips silently.

The typed explicit-context shelf is shipped on top of that compatibility
field. `contextSources[]` carries at most 16 ordered opaque refs for project
files/selections, exact memory ids, canonical topic files, and immutable Browser
text/screenshot-evidence ids. Preview reports
independent ready/blocked outcomes; send re-resolves all sources and accepts
only when the whole bounded set fits, then returns the exact manifest. Project
sessions persist the current shelf and accepted per-turn manifests. The old
singular `attachment` remains wire-compatible but cannot be combined with the
typed array.

Visible drag/drop placement is shipped as a frontend gesture over that exact
contract. Eligible Knowledge memory/topic actions and the current Files
inspector file/selection carry one opaque `ContextSourceRef` under a private
MIME type. A temporary **Drop into project chat** target calls the same
`addContextSource` handoff as **Use in chat**, then reveals the canonical shelf.
No content bytes cross the gesture, and backend preview/send resolution remains
the authority.

Still roadmap on top of the streaming surface:

- `chat.tool { id, seq, name, args }` — tool-call frames for an
  agent-loop mode. Reserved in the streaming shape but not emitted
  today (the backend rejects payloads with `role: 'tool'`).
- Additional typed source kinds — recent terminal output or a clipboard
  snippet — only after each owning bounded resolver and
  provenance manifest exists. D10's line range remains part of
  `projectFile`, not a separate kind.
- Richer project-instructions surface — `README.md` auto-context,
  per-directory overlays, `.plume/instructions/` files. D11 keeps
  the v1 scope to root `AGENTS.md` only.
- Live mid-stream tok/s in `chat/token`. D9 ships the final
  per-call breakdown (`outputTokens`, `evalMs`, `tokensPerSecond`,
  `promptTokens`, `promptMs`) inside the terminal `chat/done`
  event; a per-token rolling throughput would need a window over
  recent deltas and is deferred.
- Forcible cancellation. D7.1's cancel is cooperative — between
  NDJSON line reads, the loop polls a flag. Hard-aborting the
  underlying TCP read would close the socket and shorten the
  worst-case latency from ~200 ms to ~0 ms but adds complexity for
  marginal benefit on localhost. Revisit if a future adapter has
  a longer per-frame wait.

## Context inventory

D12 shipped: `chat.context` — read-only preflight that reports what
would ride along on the next `chat.send` (AGENTS.md probe +
attachment resolution + line-range validation) without invoking a
model. Shape lives in `docs/IPC_CONTRACT.md § chat`. Reuses
`prompts::preview_context` so the preview's numbers always match
the actual send. Attachment rejections surface in-band as
`attachment.status === 'blocked'` with a stable `reason` code so
a blocked attachment doesn't hide the AGENTS.md preview alongside
it. UI lands in the chat panel as a small "Context preview" area
between the attach bar and the textarea (label is neutral so a
blocked attachment, which would NOT ride along, still belongs in
the same section).

- `chat.compact` — token-budget-aware transcript compaction is the
  next step in this surface. Out of scope for D12.

## Diagnostics

- `doctor.run`

## Patch validate / apply / checkpoint / revert

D15 shipped the model side: `mode: 'proposeDiff'` on `chat.send`
pins the response to a unified-diff PREVIEW that the chat panel
renders with per-line coloring. Plume does NOT apply the diff:
the visible Apply button is disabled with a tooltip naming the
boundary, and no IPC verb writes to disk on behalf of a diff. The
D14 Copy button on the assistant turn covers "grab this diff and
apply by hand." See `docs/IPC_CONTRACT.md § chat` for the wire
shape.

D16 shipped the read-only validator on top of that: a new
`patch.validate(payload: { diff })` verb that parses the
assistant's reply, enforces project-root path safety on every
diff-side path (no `..`, no absolute paths, no symlinks pointing
out), and returns `{ ok: true; touches; hunks }` or
`{ ok: false; errors[] }`. The chat panel renders the validator's
verdict as a small pill under the diff body (`valid diff · 2
files · 4 hunks` / `invalid diff: <reason>`). The Apply button
stays disabled — validation passing today only means "the shape
is sane and stays inside the project," not "Plume will apply
this." See `docs/IPC_CONTRACT.md § patch` for the wire shape.

D31 shipped the writing half — `patch.apply(payload: { diff })`.
It re-runs validation server-side, verifies every hunk's pre-image
against disk, takes a filesystem-backed checkpoint at
`.plume/checkpoints/<id>/`, then writes each touched file via
sibling-tempfile + atomic rename. Apply is all-or-nothing: a
pre-image mismatch on any hunk rejects the whole patch before any
write; a mid-apply write failure rolls back everything applied so
far via the checkpoint. Supported change types: modify, create,
delete. Rename apply also lands as of D33. The chat panel's
Apply button is now wired: enabled when validation is green,
disables while in flight, flips to terminal `Applied` on
success with the checkpoint id visible in the pill. See
`docs/IPC_CONTRACT.md § patch` for the wire shape.

D32 was a frontend-only slice: per-column inner-panel toggles
on top of D30's outer columns. No IPC changes.

D33 shipped the inverse verb and rename apply.
`patch.revert({ checkpoint })` reads the manifest, drift-detects
every touched file against the stored post-apply image, rejects
in-band on disagreement (`reason: 'drift'`), and otherwise
applies the inverse of each manifest entry all-or-nothing. The
checkpoint manifest grew a `version: 2` stamp and a parallel
`post/` subtree of post-image bytes; D31-vintage checkpoints
reject with `unsupportedCheckpoint` since they have no
post-image signature to drift-detect against. Rename apply
ships as `fs::rename` plus an optional atomic body write for
rename-with-edits (the destination must not exist; pure
rename-no-edits diffs are now parseable). The chat panel grew
a Revert button next to the now-terminal Apply on a
successfully applied turn.

Still roadmap:

- Force-revert / `override: 'discardLocalEdits'` — D33's revert
  rejects on any drift. A follow-up slice can layer an explicit
  force flag, but it needs its own approval prompt: a revert
  that nukes user changes is exactly what the approval gate
  exists for.
- Durable redo checkpoint. D33 captures pre-revert state in
  memory for rollback only; persisting it to disk so the user
  can revert the revert is a future refinement.
- `patch.checkpoint` as a standalone verb — deferred indefinitely;
  the empty-payload shape is not implementable as a useful
  primitive. See `PATCH_APPLY_DESIGN.md § deferred` for the
  reasoning. Whole-working-tree snapshots will likely tier off
  the agent-loop slice instead.
- Three-way merge / soft drift recovery on apply.

## Tools

Beyond the model's text channel, future slices give the model
**tool-use surfaces** — typed IPC verbs the chat loop can route
through. Each tool family lives behind its own approval gate.

**Status (D92/D93/D96): catalog + event protocol are scaffolds; the
first executing step (`agent.singleStep`) runs ONE safe action
(read-only validate) and gates everything else.** No tool that mutates
state or runs a command executes yet; that lands only behind an explicit
approval / allowlist gate (`docs/SAFETY.md`). What exists today:

- `tools.list` / `tools.search` (D92, shipped) — a **read-only** view of
  the agent tool catalog (`docs/TOOL_DISCLOSURE.md`): core tools are
  always listed, optional tools are reached by search. Listing or finding
  a tool grants *visibility*, never permission to run it. No execution,
  no MCP. See `docs/IPC_CONTRACT.md § tools`.
- `agent.dryRun` (D93, shipped) — a deterministic, **dev-only** stream of
  the typed agent events (`docs/IPC_CONTRACT.md § agent`) that proves the
  event protocol drives the UI's `AgentEventLog`. Nothing real runs.
- `agent.singleStep` (D96, shipped) — the first **executing** step. Sends
  one propose-diff prompt to the selected, running local MLX model,
  classifies the reply, runs read-only `patch.validate` (the one safe
  action — writes nothing), and surfaces *applying* behind the D83
  approval gate (a write always prompts, so the run emits
  `approvalRequired` + `paused`). It never applies, runs a command, or
  recurses; an unsupported tool request becomes a blocked `toolFailed`.
  This is where the catalog/approval/event scaffolds first carry a real
  model turn. See `docs/IPC_CONTRACT.md § agent`.
- A future `tools.invoke` (not implemented) is where *mutating* execution
  lands — apply, run-command, search — gated by `agentMode` + the per-tool
  capability flags the session policy carries, and surfaced through the
  same typed event stream. `agent.singleStep` is the seam it grows from.

### Computer use (post-MVP)

Plume's "computer use" track is about an EMITTING role: Plume
drives a target environment on the user's behalf (clicks, types,
scrolls, captures screenshots, optionally reads an accessibility
tree). This is the inverse of `docs/AGENT_OPERABILITY.md`, which
is about EXTERNAL agents driving *Plume*'s UI through ordinary
accessibility APIs. The two surfaces are independent and share
no IPC — the operability work uses platform accessibility, the
computer-use work is a Plume-mediated tool family the model gets
to call. See `docs/AGENT_OPERABILITY.md § Plume as a computer-use
HOST` for the boundary.

The track lands in two phases:

1. **Phase A — In-app / browser sandbox.** Plume opens a sandboxed
   webview inside its own window, hands the model a reference to
   that session, and routes input synthesis + screenshots through
   it. The "computer" is fully Plume's territory: no host
   accessibility APIs, no host screen capture, no host input
   synthesis. The sandbox enforces a strict CSP, blocks disk
   access, and blocks arbitrary URL navigation unless the user
   has put the host in the session's `targetAllowlist`. Phase A
   is the *only* track shipped initially because the blast radius
   is bounded to a Plume-controlled webview.
2. **Phase B — Host desktop.** Plume drives the user's actual
   macOS desktop via macOS accessibility APIs + `CGEvent` input
   synthesis + `CGWindowList` screen capture. **Off by default,
   per-session opt-in, per-target allowlist.** A session that
   wants host access must show Plume's own per-session approval
   dialog every time it starts; nothing about that dialog is
   persisted across sessions. The target allowlist names
   specific application bundle IDs or window titles — no "all
   of macOS" mode. Phase B is gated behind the same
   project-trust check as everything else, AND requires the
   macOS-level Accessibility + Screen Recording permissions.
   Those macOS permissions are **app-level persistent grants**
   managed in System Settings → Privacy & Security: macOS
   prompts the user once when Plume first attempts each, then
   remembers the choice across launches and sessions until the
   user revokes it. Plume's per-session approval dialog (which
   does NOT persist) sits ON TOP OF the persistent OS grant —
   the OS grant alone does not authorize a session, and
   revoking the OS grant disables Phase B regardless of any
   prior session-level approval. See `docs/SAFETY.md §
   Computer-use sandbox` for the three-layer gate.

#### Shipped human Browser foundation (not computer use)

Eight v1 application commands now own one separately labelled remote-content
window:

```text
browser.sandboxOpen({ url, approvedLoopbackOrigin? }) -> BrowserSandboxState
browser.sandboxClose({})                            -> BrowserSandboxState
browser.sandboxState({})                            -> BrowserSandboxState
browser.sandboxFocus({})                            -> BrowserSandboxState
browser.sandboxBack({})                             -> BrowserSandboxState
browser.sandboxForward({})                          -> BrowserSandboxState
browser.sandboxReload({})                           -> BrowserSandboxState
browser.sandboxCaptureText({ captureKind })         -> BrowserEvidenceSummary
```

They are callable only from webview `main`, and Tauri's production capability
grants no application or core permission to `browser-sandbox`. The URL policy
accepts absolute HTTP(S), classifies loopback without DNS, and blocks
credentials and every other top-level scheme. Popups and downloads are denied;
Rust-owned callbacks may update visible URL/loading state. A global human
workspace now exposes visible fixed navigation controls. Loopback top-level
navigation requires exact-origin confirmation once per sandbox-window session;
public hosts and ordinary subresources are not represented as a full allowlist. See
`docs/IPC_CONTRACT.md § browser` for the exact v1 wire shape.

In a trusted project, the main webview can also request one of two fixed-purpose
text captures: the user's current selection or visible page text. No caller-
supplied script/selector crosses IPC. The backend binds the callback to the
current page generation and project, redacts and caps the result, stores an
immutable project record, and returns its opaque id for the typed context shelf.
This is explicit evidence placement, not agent observation or retrieval.

This is a human Browser, not a `computer.*` executor. It has no agent session
approval, general target allowlist, screenshot, arbitrary DOM
observation, input synthesis, trace, automatic retrieval, or host control. The
packaged smoke covers human navigation, localhost confirmation, and explicit
text capture; hostile-page
authority remains covered by runtime capability tests and packaged observation.

Reserved verbs (post-MVP, none implemented today):

```
computer.session.start(payload: { target, targetAllowlist? })
  -> { sessionId; targetKind: 'sandbox' | 'host'; viewportPx }

computer.session.end(payload: { sessionId })
  -> void

computer.capture(payload: { sessionId })
  -> { sessionId; frameId; widthPx; heightPx; image }
  // `image` is a typed payload (PNG bytes + content-type) routed
  // through IPC, not a file path. The frontend renders it inline
  // in the trace area. Image safety for the *model-bound* copy is
  // scaling / cropping + the session's `targetAllowlist` — the
  // existing text-regex prompt-read redactor does NOT rewrite
  // image bytes (it cannot un-paint a secret in a PNG). Any text
  // Plume extracts from the capture (OCR, AX tree, DOM strings)
  // DOES pass through the existing redactor. See
  // `docs/SAFETY.md § Redaction before model sees frames`.

computer.click(payload: { sessionId, x, y, button?, modifierKeys? })
  -> { actionId }

computer.type(payload: { sessionId, text, modifierKeys? })
  -> { actionId }

computer.scroll(payload: { sessionId, x, y, dx, dy })
  -> { actionId }

computer.drag(payload: { sessionId, from: { x, y }, to: { x, y }, button? })
  -> { actionId }

computer.observe(payload: { sessionId })
  -> { sessionId; tree: AxNode[] }
  // Read-only accessibility / DOM snapshot. Phase A serves the
  // webview's DOM (Plume can introspect this directly); Phase B
  // serves a filtered AXUIElement walk on macOS. The tree is
  // already a structured representation, so the model doesn't
  // need to OCR a screenshot for basic interaction.

computer.trace(payload: { sessionId })
  -> { sessionId; actions: ActionTraceEntry[] }
  // Read-only audit log of every action this session has
  // executed (or had rejected). Used by the chat panel's
  // computer-use trace area; also surfaced to the user as the
  // before-approval review surface when a session ends.
```

Events:

```
computer.action          { sessionId, actionId, kind, status, ... }
computer.frame           { sessionId, frameId, widthPx, heightPx, image }
computer.session.end     { sessionId, reason }
```

Approval shape:

- Every session start is a foreground approval prompt. Approving
  one session does NOT pre-approve future sessions; there is no
  "always allow" toggle for computer-use sessions (mirroring the
  pattern `docs/SAFETY.md § Approval ledger` calls out as the
  reason `agent-loop` requires explicit per-task approval).
- Each individual action goes into a visible trace; the user can
  pause / stop the session from the trace area. Phase A's
  within-session policy is `auto-execute`-style (the sandbox is
  bounded — actions inside the session run without per-action
  re-prompt). Phase B's within-session policy is `ask-each` for
  per-action gating until the user explicitly relaxes it
  **within that same approved session**; relaxation does NOT
  carry into the next session. The next `computer.session.start`
  starts fresh at `ask-each` regardless of how the previous
  session ended. There is no persistent computer-use approval
  setting at any layer.
- The `targetAllowlist` is a per-session list of allowed
  bundleIds / URLs / window titles. Actions that target anything
  outside the list reject with `Blocked`. There is no
  "wildcard" target — `*` is not an allowed entry.

Open contract questions (to settle before the slice lands):

- Whether `computer.capture` returns raw PNG bytes via IPC or a
  one-shot URL the webview/UI can load directly. The trade-off
  is IPC payload size vs render simplicity; this depends on what
  Tauri's IPC layer prefers for binary blobs.
- Whether `computer.observe` lives behind its own approval
  (it can leak text content from the target — Phase B against
  the user's email window is sensitive). Probably yes — listed
  separately in the session's capability flags.
- Whether `trycua` / `cua-driver` (the upstream computer-use
  reference, https://github.com/trycua) supplies the Phase B
  backend or only inspires the shape. No commitments today: no
  installs, no dependency added, no code. The mention here is
  a placeholder so the shape we ship leaves room for
  integration if the trade-off lands favourably.

What does NOT ship in this track:

- A "click whatever you think is needed" autopilot. Every action
  is announced and traceable.
- Hidden host access. Phase B is opt-in per session AND per
  target — there is no codepath that grants host access from a
  Phase A approval.
- Persistent computer-use approvals across sessions. The
  approval ledger does not store computer-use entries today and
  is not the place for them in the future — they belong to the
  session, not the project.
- Network-targeted automation. Plume is a local-first editor;
  the computer-use track inherits that. Even Phase A's webview
  defaults to offline (no network) unless the session explicitly
  whitelists hosts.

See `docs/SAFETY.md § Computer-use sandbox` for the safety
contract and `docs/AGENT_OPERABILITY.md § Plume as a
computer-use HOST` for the UI contract.

## Provider health (future fields)

The verb `providers.health` itself shipped in D1 and is part of the
v1 contract — see `docs/IPC_CONTRACT.md § providers` for the shape
that lands today.

D2 added the per-adapter `models` field, populated by the Ollama
adapter (`GET /api/tags`). Other adapters carry `null` until their
HTTP probes land — LM Studio's `/v1/models` and llama.cpp's
`/v1/models` are the next two.

D3 added `providers.modelDetails` (lazy, per-model `/api/show` for
Ollama) and the cautious fit estimator. Model truth (family,
parameter count, quantization, context length) plus the green / amber
/ red working-set verdict now lands in the contract.

D4 added LM Studio and llama.cpp model-list probes — both serve
OpenAI-style `/v1/models` and share a parser. llama.cpp moved out of
"not configured" and onto the TCP probe set (default port 8080); its
registry category stays `PlumeManaged` until Plume actually
supervises `llama-server`.

What's still roadmap is the *richer* per-provider state that an
adapter-specific probe can return:

- current loaded model and how long it has been resident in RAM,
- recent errors with timestamps,
- token-stream throughput estimates,
- per-provider feature flags (e.g. tool-call mode the daemon negotiated).

Also still roadmap, post-D4:

- `providers.modelDetails` for non-Ollama adapters (LM Studio's
  WebSocket-only model metadata, llama.cpp's `/props` endpoint, an
  MLX-LM model registry).
- User-configurable probe ports for llama-server (`--port` overrides)
  and Ollama (`OLLAMA_HOST`).
- Process supervision: spawning `ollama serve`, `llama-server`,
  `mlx_lm.server`. Moves the relevant providers to `PlumeManaged` in
  practice, not just on paper.
- KV-cache math that uses real per-architecture head/layer counts
  instead of the 15% rule-of-thumb the D3 estimator uses today.
- Live memory-pressure signal so the UI can flip from "fit guess" to
  "fit observed" once a model is actually loaded.

These are additive — they extend `ProviderHealth`, `ProviderModel`,
and `ProviderModelDetails` without breaking v1 fields. Each adapter
contributes its own probe; the D1 TCP connect is the floor, not the
ceiling.

## Local model library

D27 shipped the first verb in the local-model-library track:
`providers.localModels` returns a read-only inventory of the model
weights Plume can see on disk under `PLUME_MODEL_DIR` (default
`<project>/plume-models`). The scanner surfaces `.gguf` files,
`.safetensors` files, and HuggingFace-style transformer folders
(`config.json` + a `tokenizer*` file + a weight file); symlinks are
not followed. `PLUME_MODEL_DIR` is trusted operator input — see
`docs/MODEL_PROVIDERS.md § Local model library` and
`docs/IPC_CONTRACT.md § providers` for the wire shape and the
trust-model note.

D27 is **inventory only**. The verb does not download, copy, import,
launch, validate, or select models — those are deliberately separate
concerns the track will pick up across later slices. Reserved
roadmap:

- `providers.localModels.import(payload)` — copy or hard-link a file
  or folder into `PLUME_MODEL_DIR` with SHA verification, free-disk
  pre-flight, and no auto-extract. Companion `.remove` purges a
  library entry.
- `providers.localModels.diskUsage()` — recursive size summary for
  the library, for the UI's eventual capacity strip.
- `providers.localModels.download.start / .cancel / .status` —
  HTTP-only model download with progress events, per-host allowlist,
  and no auto-execution. Safety contract pinned before the first
  slice lands.
- A stricter `mlx-folder` kind that downgrades the current
  `transformer-folder` heuristic once a slice parses `config.json`
  or inspects weight files for MLX-specific markers (`*.npz` shards,
  MLX quantization keys). The `mlx-folder` name is reserved on the
  wire today for exactly this purpose.

These extensions are additive — the v1 `LocalModel` shape stays
fixed, and downloads/imports/launches land as new verb names rather
than overloads of `providers.localModels`.

## Host status

D5 shipped `system.snapshot` returning memory / swap / load average
/ machine labels from cheap macOS tools (`sysctl`, `vm_stat`,
`uname`, `sw_vers`). The contract shape is locked; what's still
roadmap is the *extra signals* we did not ship there:

- live CPU usage (vs the 1/5/15-minute load average we surface
  today). The `iostat` and `top` paths are heavier than the rest of
  the snapshot, so this lands when the cost is justified — likely
  alongside chat where the user actually cares about percent-busy
  during a generation.
- GPU usage / power. No cheap general-purpose macOS API for this;
  requires Metal / IOReport / `powermetrics`, which needs sudo.
  Reserved until a slice has a real reason to spend the complexity.
- non-macOS platforms (Linux `/proc/meminfo` + `/proc/loadavg`,
  Windows `GlobalMemoryStatusEx`). Plume's primary target is mac
  per `docs/PLUME_PROJECT_SPEC.md § 5`; other platforms get a
  null-everywhere snapshot until they are first-class.
- kernel memory-pressure level (the value Activity Monitor renders
  as the green/yellow/red graph). The sysctl that exposes it
  requires elevated privileges on most macOS versions, so D5
  derives a heuristic; we revisit once supervision or sandboxing
  unlocks the read.

## External agent engines

Reserved for the engine track sketched in `docs/ARCHITECTURE.md` and
`docs/MODEL_PROVIDERS.md § External agent engines`. Names and shapes
will change. Nothing here is part of the v1 contract.

- `engines.list`       — installed engines, version, runtime category
- `engines.start`      — open an engine session in the current project,
                         returns an engine session id
- `engines.stop`       — terminate an engine session
- `engines.send`       — forward a user instruction; tokens stream back
                         the same way `chat/token` does today

The engine never reaches disk on its own. Reads, writes, command
runs, and patch applies still flow through Plume's existing `fs.*`,
`commands.*`, and `patch.*` IPC and through `safety::guard`. An
engine that expects direct disk access is not a fit for this track.

## Harness radar (reserved surfaces)

Reserved shapes for the latest Hermes/Codex harness lessons distilled in
`docs/AGENT_RUNTIME.md § Harness Radar`. Names and shapes will change; nothing
here is part of the v1 contract. These mostly *extend existing sections* (Tools,
Session mode and policy, Project memory, External agent engines) rather than
add new families — they are gathered here so the radar has one home.

### Tool-risk metadata (D106, internal-only first)

An internal descriptor per agent tool, carried in the catalog
(`docs/TOOL_DISCLOSURE.md`) and read by policy. **No execution** — D106 ships
the data model and a pure policy helper, nothing that runs a tool.

```
ToolRisk {
  id: string            // namespaced, e.g. "patch/apply" (see below)
  namespace: string     // "core" | "patch" | "search" | "mcp:<server>" | "engine:<id>"
  title: string         // human label for disclosure + the event log
  readOnly: bool
  mutating: bool
  destructive: bool
  openWorld: bool       // reaches network / outside the project root
  requiresApproval: bool // hard floor — true means "always prompt", policy can't lower it
}
```

### Writes-only approval mode (D106 policy helper)

`approvalPolicy` gains a `'writes'` value alongside the v1 `'ask-each'`:

- read-only tools run without a prompt,
- mutating tools prompt once (then follow the ledger),
- destructive tools always prompt, regardless of policy or ledger.

```
session.setApprovalPolicy({ policy: 'ask-each' | 'writes' | 'allowlist' })
```

D106 implements the *decision function* over `ToolRisk` as a pure helper with
tests only — no IPC, no execution wiring. The verb above stays reserved until
the policy module lands (see `session.setApprovalPolicy` above).

### Namespaced tool ids

Tool identity becomes `namespace/tool` (matching `ToolRisk.id`) instead of a
bare verb. The `tool` field on the `AgentEvent` variants
(`docs/IPC_CONTRACT.md § agent`) carries the namespaced id so the event log and
policy can match on a prefix. A migration note, not a new verb.

### Typed event stream expansion

New `AgentEvent` variants reserved (additive to the D85 union; names will
change): `capReached` (a per-turn ceiling hit), `autoPaused` (transport failure,
resumable), `telemetry` (structured per-tool / per-loop stats). The rule from
`docs/AGENT_RUNTIME.md § 4` holds: expand the typed union, never stuff new
payloads into a stringly-typed field.

### Per-turn tool / subagent caps

Reserved fields on the session policy (read via `session.state`):
`maxToolCalls` and `maxSubagents` per turn — runtime-enforced ceilings, far
above normal use, surfaced as a `capReached` event when hit (never a silent
truncation).

### Transport-failure auto-pause

When the provider transport drops mid-turn the loop emits `autoPaused` with a
failure class and a resumable handle, instead of retry-spinning or failing
silent. Pairs with the existing `paused` event; resume reuses the chat-stream
resume path rather than a new verb.

### Memory delimiter / schema hardening

Not a new verb — a schema rule on `memory.*` (Project memory, above). Remembered
text is already secret-redacted and size-capped (D37); the radar adds
delimiter/role-marker escaping and a structured entry schema so a remembered
line can never forge a prompt delimiter or escape its slot when folded into
context.

### Structured tool / inference telemetry

A `stats` payload (mirroring the `chat.done` generation stats from D9) extended
to *tool* calls and the agent loop, delivered as the `telemetry` event above:
duration, token counts, tokens/sec, failure class. Inspectable cost, per the
resource-honesty rule.

### Remote gateway auth + backend-workspace routing

Post-MVP, lives next to `External agent engines`. If execution can leave the
local box, the gateway surface reserves authenticated access plus explicit
per-workspace routing so sessions/projects can't cross-contaminate:

```
engines.start({ ..., backendWorkspaceId })   // route this session to one backend workspace
gateway.connect({ endpoint, authToken })     // reserved; authenticated gateway access
```

Local-first remains the default; this is reserved so auth + routing are
designed in, not retrofitted. See `docs/AGENT_RUNTIME.md § 9` and the
computer-use Phase B gate for the safety posture.

## Agent operability / smoke harness

No IPC names reserved yet. The first step is UI contract, not hidden
automation API: see `docs/AGENT_OPERABILITY.md`.

Possible future surfaces:

- `app.smokeState` — read app/window state useful for smoke tests, only in
  dev/smoke builds.
- `app.focusRegion` — focus a named visible UI region, same behavior as a
  keyboard shortcut.

Do not add an automation-only bypass for trust, command approval, patch
approval, or file safety.

## Hooks (internal-only first)

Hook events fire inside Rust before any project-level hook config
exists. Reserved event names so settings and tests written today don't
collide with later additions: `SessionStart`, `InstructionsLoaded`,
`UserPromptSubmit`, `PreToolUse`, `PostToolUse`, `PostToolUseFailure`,
`PostToolBatch`, `PermissionRequest`, `PreCompact`, `PostCompact`,
`FileChanged`, `Stop`. **No external hook surface in MVP.** A
`.plume/hooks.toml` would let a malicious repo run shell on first open;
that problem is not solved here.
