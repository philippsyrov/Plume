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

Until these land the v1 session is locked to `approvalPolicy:
'ask-each'` with empty allowlists, regardless of `agentMode`.

## Project memory

- `memory.read`
- `memory.write`
- `memory.list`
- `memory.delete`

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

What's still roadmap is the *richer* per-provider state that an
adapter-specific probe can return:

- current loaded model and how long it has been resident in RAM,
- recent errors with timestamps,
- token-stream throughput estimates,
- per-provider feature flags (e.g. tool-call mode the daemon negotiated),
- per-model metadata beyond `id` and `sizeBytes` (quantization,
  parameter size, family) — likely as additive optional fields on
  `ProviderModel`.

These are additive — they extend `ProviderHealth` and `ProviderModel`
without breaking the v1 fields. Each adapter contributes its own
probe; the D1 TCP connect is the floor, not the ceiling.

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
