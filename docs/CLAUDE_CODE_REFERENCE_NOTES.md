# Claude Code Reference Notes

This file captures public, behavior-level lessons for Plume from:

- Kuber Mehta's write-up:
  `https://kuber.studio/blog/AI/Claude-Code's-Entire-Source-Code-Got-Leaked-via-a-Sourcemap-in-npm,-Let's-Talk-About-it`
- `codeaashu/claude-code`:
  `https://github.com/codeaashu/claude-code`
- Plume's existing local note:
  `/Users/philippsyrov/Desktop/CS Projects/Sass/.claude/reference-claude-code-patterns.md`

Do not copy leaked source code, internal prompts, private schemas, or
proprietary implementation text into Plume. Treat these sources as a map
of product patterns to re-design cleanly for a local-first Tauri/Rust app.

## Legal / Source Hygiene

The `codeaashu/claude-code` repository says the `src/` directory is leaked
Anthropic source and its license says it is not open source and not
licensed for redistribution. Do not:

- clone it into Plume
- vendor any file from it
- install its MCP explorer
- copy source snippets, internal prompts, or private constants
- ask an agent to reproduce implementation details from source files

Allowed use:

- read public write-ups and documentation summaries
- extract behavior-level architecture ideas
- compare product surfaces
- write original Plume designs in our own words

This is the clean-room line. Plume should be inspired by the workflow, not
derived from leaked code.

## Sourcemap Leak Lesson For Plume

The blog explains the leak mechanism: a production package shipped a
source map whose `sourcesContent` embedded original TypeScript source.

Plume should add a release-artifact audit before any public package:

- no source maps in production desktop bundles unless intentionally
  stripped of source content
- no `sourcesContent` in distributable artifacts
- no internal docs, prompts, logs, or test fixtures in release bundles
- no `.env`, local config, app data, session logs, or model cache files
- no debug prompt dumps
- no private feature flags or unreleased provider names in public builds

Future verifier idea:

```text
scripts/verify-release.sh
  build app
  scan dist/, src-tauri/target/release/bundle/
  fail on *.map with sourcesContent
  fail on known secret/config/session patterns
```

## Product Shape Takeaway

Claude Code is not just "chat in a terminal." The public summaries describe
a full coding-agent runtime:

- query engine for streaming and tool loops
- self-contained tool modules
- command registry for slash commands
- permission checks on every tool invocation
- context compaction
- memory
- background tasks
- diagnostics
- IDE bridge
- MCP integration
- skills/plugins
- multi-agent orchestration
- feature gates

Plume should not copy this whole surface at once. The useful first target
is the smallest local coding-agent loop:

```text
Project truth -> context packet -> model reply -> proposed diff ->
path-safe validation -> user approval -> apply -> verify -> review summary
```

Everything else should orbit that loop.

## Tool System Pattern

Public docs describe each tool as a self-contained unit with:

- name and aliases
- input schema
- permission model
- execution logic
- read-only/destructive classification
- concurrency-safety classification
- progress reporting
- UI rendering for invocation and result
- optional prompt contribution

Clean Plume version:

```text
ToolDescriptor
  id
  display_name
  input_schema
  risk_level
  read_only
  concurrency
  required_approval
  run(input, ToolContext, CancellationToken) -> ToolResult
```

Important for local models:

- Tool schemas must be short and token-efficient.
- Small models should see only the tools available in the current stage.
- Read-only tools can run sooner; write/command tools require stronger
  permission gates.
- Tools should return compact structured results plus optional full logs
  stored outside prompt context.

Recommended MVP tool set:

- `project.open`
- `fs.list`
- `fs.read`
- `search.grep`
- `git.status`
- `git.diff`
- `patch.validate`
- `patch.apply`
- `commands.detect`
- `commands.run`
- `chat.cancel`
- `commands.cancel`

Defer:

- web fetch/search
- notebook editing
- MCP tool execution
- LSP
- tasks/background agents
- cron/scheduled work
- remote triggers

## Command System Pattern

The public docs separate tools from user-facing slash commands. Commands
are UX shortcuts; tools are model-callable capabilities.

Useful command categories for Plume:

- `/review` inspect current diff and report findings
- `/compact` compress session context
- `/context` show what is in the model context and why
- `/diff` show current changes
- `/doctor` check local dependencies, providers, verifier, git
- `/model` select provider/model
- `/permissions` inspect/revoke approvals
- `/memory` inspect/edit project memory
- `/status` show project/provider/runtime truth
- `/clear` reset conversation

Plume should avoid command sprawl until the core loop works. MVP should
start with visible UI actions and later expose slash commands as a power
user surface.

## Query Engine / Agent Loop Pattern

The central runtime should own:

- streaming provider calls
- tool-call loop
- retry/backoff for transient provider errors
- cancellation
- token/context accounting
- compaction triggers
- permission interrupts
- event sequencing
- tool result summarization
- final turn summary

For Plume, this belongs in Rust, not React. React displays state and asks
for actions; Rust owns model calls, filesystem, process execution, and
tool execution.

Clean Plume loop:

1. Build `ChatRequest` from UI state.
2. Rust builds the final prompt from safe project context.
3. Provider streams tokens.
4. If tool call appears, Rust validates against current mode and
   permissions.
5. Tool result is summarized and fed back.
6. Loop continues until stop, cancellation, error, or iteration budget.
7. Session log records every step.

## Prompt Architecture Pattern

The blog describes a modular prompt architecture with static and dynamic
sections. Plume should copy the idea, not the text.

Clean Plume prompt sections:

- static Plume role and safety rules
- current agent stage
- project rules from `AGENTS.md`
- selected task
- allowed files and allowed commands
- context packet
- tool schemas for the current stage only
- output contract
- cancellation/stop conditions

Rules:

- Static sections should be stable.
- Dynamic sections should be small and explicit.
- Volatile reminders should be isolated so they do not churn the whole
  prompt.
- For local models, prefer fewer instructions with stronger structure.
- Never include hidden proprietary prompt text from outside sources.

## Context / Compaction Pattern

Useful behavior:

- `/context` explains what the model can currently see.
- `/compact` reduces old conversation state.
- auto-compaction runs before hard context failure.
- compaction preserves current task, files, decisions, verifier output,
  permissions, and open questions.

Plume-specific design:

- Micro-compact repeated terminal output locally without a model.
- Auto-compact older turns using the active local model only if the model
  is strong enough; otherwise use deterministic extraction.
- Manual compact lets the user say what to preserve.
- Context cards in the UI show source, size, reason, and redaction state.

For small local models, context quality is more important than context
quantity. A 4B model with a clean 2k-token packet may beat a 14B model
fed a messy repo dump.

## Permission System Pattern

Public docs describe permission modes, wildcard rules, protected files,
path traversal defenses, and risk explanations.

Plume version:

- default: ask for destructive tools
- plan: show a full plan and ask for scoped approval
- read-only: only read/search/status tools
- scoped-edit: only approved files and approved verifier commands
- never include a "bypass everything" user-facing mode in MVP

Approval records should include:

- project id/root
- normalized tool id
- normalized argv or file scope
- risk level
- granted time
- expiration/revocation state
- human-readable explanation shown to the user

Protected targets:

- `.git/**` writes denied by default
- `.env*`, keys, tokens, credentials redacted by default
- shell profiles and global config outside project denied
- package manager install/fetch commands require explicit approval
- network commands require explicit approval

Do not use an LLM as the only permission checker. Cheap deterministic
checks run first; an LLM may explain risk, but Rust enforces policy.

## Risk Classification

Every tool action should get a risk label:

- Low: read-only file list, git status, grep, context inspect
- Medium: read file content, run verifier, inspect diff, provider health
- High: write file, apply patch, run shell command, edit config
- Blocked: path escape, destructive shell, global install, credential
  access, `.git` mutation without explicit flow

The UI should show risk before approval. The model should not be allowed
to self-certify that a high-risk action is safe.

## Protected Files And Path Safety

Public summaries call out path traversal handling and protected files.
Plume already has safety docs; add these implementation reminders:

- normalize Unicode before policy checks
- reject URL-encoded traversal
- handle macOS case-insensitive paths carefully
- defend symlink escape
- document hard-link behavior
- avoid check-then-open races where possible
- treat `.git/**` as a special protected namespace
- redaction must be content-based, not filename-only

## Feature Gates

The blog highlights compile-time and runtime feature gating. Plume should
use simpler gates:

- cargo features for optional provider adapters
- config flags for experimental UI surfaces
- kill switches for risky runtime behavior
- explicit "experimental" labels in the UI

Do not build hidden internal modes. This is an open/local product; feature
gates are for staged rollout and safety, not secrecy.

Good early gates:

- `provider.mlx_lm`
- `provider.ollama`
- `agent.stage4`
- `memory.project`
- `hooks.project`
- `mcp.client`

## Diagnostics / Doctor

Public docs show a `/doctor` command that checks environment and runtime
health. Plume needs this early.

Doctor should check:

- app version
- OS and architecture
- Rust availability
- Node availability
- Tauri prerequisites where detectable
- `node_modules` present
- Cargo cache/toolchain availability
- provider availability: MLX-LM, Ollama, LM Studio, llama.cpp
- git repo state
- verifier command and last result
- project trust state
- approval ledger location
- release-artifact audit status eventually

Doctor output should be local and honest: no fake "build passes" if
dependencies are missing.

## Memory Pattern

Public summaries describe project memory, user memory, extracted memory,
and team memory. Plume should start smaller:

- project memory only
- stored under `.plume/memory/`
- created only after trust
- visible in UI
- append-only session logs
- short index plus topic files
- no secret storage

Memory should help future sessions orient; it should not become a hidden
second instruction file that can grant permissions.

## Task / Background Work Pattern

Public summaries mention background tasks, local shell tasks, agent tasks,
remote tasks, and dream tasks.

Plume MVP should only support:

- one foreground agent run
- cancellable verifier command
- cancellable provider stream

Later:

- background verifier
- background repo scan
- memory consolidation
- review worker

Never let background work write files without a visible active session and
approval record.

## Multi-Agent / Coordinator Pattern

Multi-agent orchestration is impressive but not MVP.

For local models, use it sparingly:

- read-only explorer worker
- reviewer worker
- verifier worker

Do not build agent teams, remote workers, or worktree swarms until the
single-agent editing loop is good.

If subagents happen later:

- isolate write scopes
- use git worktrees for parallel edits
- require coordinator to read worker findings, not blindly relay them
- verify worker claims with direct commands
- log every worker action

## IDE Bridge Pattern

The leaked-source docs describe an IDE bridge. Plume is already the UI, so
it does not need a VS Code bridge first.

Useful concept:

- permission prompts can be routed through a UI surface
- bridge protocols need typed messages and trust
- session handoff is a separate product surface

For Plume:

- Tauri event protocol is the first bridge.
- Future headless CLI can connect to the same Rust core.
- Do not build VS Code/JetBrains integration before the desktop app works.

## MCP / Plugin / Skill Pattern

The repo exposes an MCP explorer for the leaked source. Do not install it.
But the design idea matters: external capabilities can be discovered and
mounted as tools.

Plume should defer MCP until after:

- core tool permissions
- command approval ledger
- context packet UI
- basic provider chat
- patch validation

When MCP lands:

- MCP tools are untrusted by default
- every MCP tool gets a local risk classification
- resources are read-only unless explicitly approved
- auth lives in OS keychain/app data, not project files
- project instructions cannot auto-enable MCP servers

## LSP Pattern

An LSP tool is useful for code intelligence:

- go to definition
- find references
- symbols
- diagnostics
- rename preview

But MVP can use grep + file reads. LSP should come after the editor/file
tree is stable.

## Output Modes

Public docs mention brief/fast modes and output styling. For Plume:

- default: direct, short, reviewable
- brief: status-strip and tool-log friendly
- review: findings first
- teaching: more explanation for student mode

Do not let output modes alter safety behavior. They only change wording and
verbosity.

## What To Ask Claude To Re-Read

When Claude has a larger context window, ask it to read:

1. `docs/AGENT_RUNTIME.md`
2. this file
3. `docs/IPC_CONTRACT.md`
4. `docs/SAFETY.md`
5. `docs/MODEL_PROVIDERS.md`
6. Kuber's blog post
7. the public docs in `codeaashu/claude-code/docs/`
8. the repo README, contributing note, and license

Ask Claude specifically:

- Which patterns are missing from Plume's docs?
- Which patterns are too large for MVP?
- Which source-hygiene warnings should become verifier checks?
- What should the first local-model coding loop implement?
- What should be impossible for project instructions to grant?

Do not ask Claude to inspect or copy leaked source files.

## Plume Backlog Items From This Research

Near term:

- Add `docs/AGENT_RUNTIME.md` to the key-docs list.
- Add this reference note to the key-docs or research list.
- Add release sourcemap/source-content audit plan.
- Add `ToolDescriptor` / risk / concurrency shape to IPC or runtime docs.
- Add `/doctor`, `/context`, `/compact`, `/review` as planned UX surfaces.
- Add protected file/path policy to safety tests once code starts.

Medium term:

- Internal hook events.
- Context inspector UI.
- Permission ledger UI.
- Project memory index.
- Provider health doctor.
- Token/context budget display.
- Local-model prompt template tiers.

Defer:

- MCP client.
- LSP tool.
- plugin system.
- background tasks.
- subagents.
- worktree orchestration.
- remote sessions.
- cloud boost mode.

## Bottom Line

The leaked-source ecosystem confirms the target: a serious coding-agent
runtime is a bundle of context management, tools, permissions, memory,
diagnostics, and review loops. Plume can compete by building those ideas
cleanly around a local-first Tauri editor instead of cloning a terminal
product or a leaked codebase.
