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

Still roadmap on top of the streaming surface:

- `chat.tool { id, seq, name, args }` — tool-call frames for an
  agent-loop mode. Reserved in the streaming shape but not emitted
  today (the backend rejects payloads with `role: 'tool'`).
- Multi-file attachments. D8 carries at most one file per send.
  When multi-file lands the shape will likely become
  `attachments: ChatAttachment[]` with a per-array cap; `attachment`
  (singular) stays valid for one-file sends.
- Additional attachment kinds — recent terminal output, a
  selection-range snippet, a clipboard paste. The `kind` tag on
  `ChatAttachment` is the extension point.
- Per-token throughput / latency telemetry in `chat.done` so the
  UI can render "served by X at N tok/s". Today only `durationMs`
  is carried.
- Forcible cancellation. D7.1's cancel is cooperative — between
  NDJSON line reads, the loop polls a flag. Hard-aborting the
  underlying TCP read would close the socket and shorten the
  worst-case latency from ~200 ms to ~0 ms but adds complexity for
  marginal benefit on localhost. Revisit if a future adapter has
  a longer per-frame wait.

## Context inventory

- `chat.context`
- `chat.compact`

## Diagnostics

- `doctor.run`

## Patch checkpoint / revert

- `patch.checkpoint`
- `patch.revert`

## Tools

- `tools.list`

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
