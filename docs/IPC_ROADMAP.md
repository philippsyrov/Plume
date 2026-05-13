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

Until these land the v1 session is locked to `approvalPolicy:
'ask-each'` with empty allowlists, regardless of `agentMode`. D6
shipped the model picker as window-local React state in
`features/model-picker/useSelectedModel.ts` — selection is hoisted
in `TrustedView` and dropped when the project closes. A typed,
persisted version goes through `session.setSelectedModel` once the
session module lands.

## Project memory

- `memory.read`
- `memory.write`
- `memory.list`
- `memory.delete`

## Chat streaming

D7.1 shipped: `chat.send` returns a `ChatStreamId` immediately and
emits `chat.token` (per delta) plus a terminal `chat.done` event.
`chat.cancel(streamId)` flips a cooperative cancel flag. The full
shape is in `docs/IPC_CONTRACT.md § chat`.

D8 shipped: optional `attachment: { kind: 'projectFile', relPath }`
on `chat.send`. Backend resolves through the Rust-private
`prompts::assemble` path (secret-filename block, size cap, binary
block, content redactor) before folding the file into the last
user message. No IPC verb returns prompt-ready content.

D9 shipped: provider-neutral generation telemetry on `chat.done`.
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

Still roadmap on top of the streaming surface:

- `chat.tool { id, seq, name, args }` — tool-call frames for an
  agent-loop mode. Reserved in the streaming shape but not emitted
  today (the backend rejects payloads with `role: 'tool'`).
- Multi-file attachments. D8 carries at most one file per send.
  When multi-file lands the shape will likely become
  `attachments: ChatAttachment[]` with a per-array cap; `attachment`
  (singular) stays valid for one-file sends.
- Additional attachment kinds — recent terminal output, a
  clipboard snippet. The `kind` tag on `ChatAttachment` is the
  extension point. (D10's line range is now part of `projectFile`,
  not a separate kind.)
- Richer project-instructions surface — `README.md` auto-context,
  per-directory overlays, `.plume/instructions/` files. D11 keeps
  the v1 scope to root `AGENTS.md` only.
- Live mid-stream tok/s in `chat.token`. D9 ships the final
  per-call breakdown (`outputTokens`, `evalMs`, `tokensPerSecond`,
  `promptTokens`, `promptMs`) inside the terminal `chat.done`
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

The "actually apply" half is what's still roadmap:

- `patch.checkpoint` — record working-tree state before a write
  so the user can revert atomically.
- `patch.revert` — undo the last applied patch using the
  checkpoint.
- `patch.apply` — the IPC verb that takes a unified diff (or a
  structured patch) and writes it through a safety gate. Until
  this lands, the propose-diff Apply button stays disabled even
  when `patch.validate` returns `ok: true`.

## Tools

Beyond the model's text channel, future slices give the model
**tool-use surfaces** — typed IPC verbs the chat loop can route
through. Each tool family lives behind its own approval gate;
none are wired today. The umbrella verb:

- `tools.list` — enumerate tool families available in the current
  session, gated by `agentMode` and the per-tool capability flags
  the project's session policy carries.

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
                         the same way `chat.token` does today

The engine never reaches disk on its own. Reads, writes, command
runs, and patch applies still flow through Plume's existing `fs.*`,
`commands.*`, and `patch.*` IPC and through `safety::guard`. An
engine that expects direct disk access is not a fit for this track.

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
