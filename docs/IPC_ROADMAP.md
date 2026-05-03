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

## Project trust

- `project.trust`
- `project.trustState`

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

## Provider health

- `providers.health`

## Hooks (internal-only first)

Hook events fire inside Rust before any project-level hook config
exists. Reserved event names so settings and tests written today don't
collide with later additions: `SessionStart`, `InstructionsLoaded`,
`UserPromptSubmit`, `PreToolUse`, `PostToolUse`, `PostToolUseFailure`,
`PostToolBatch`, `PermissionRequest`, `PreCompact`, `PostCompact`,
`FileChanged`, `Stop`. **No external hook surface in MVP.** A
`.plume/hooks.toml` would let a malicious repo run shell on first open;
that problem is not solved here.
