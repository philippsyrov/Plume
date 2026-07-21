# Agent Runtime

Plume is not trying to be another local model dashboard. It is a local
Claude Code / Codex style coding workspace: read project truth, gather
small relevant context, propose safe changes, apply approved diffs, run
verification, and explain the result plainly.

The local-model constraint changes the design. Cloud coding agents can
lean on frontier model reasoning and huge context. Plume must make the
runtime smarter around the model so smaller local models get a clean,
bounded job instead of a messy repo dump.

## Source Hygiene

Use public documentation, open-source implementations, and our own
experiments as design inputs. Do not copy proprietary leaked source,
internal prompts, private tool schemas, or implementation text into
Plume.

Useful public inputs:

- Anthropic Claude Code docs for hooks, subagents, slash commands,
  settings, permissions, and memory behavior.
- OpenAI Codex docs for approval modes, AGENTS.md discipline, sandboxing,
  background tasks, and verification expectations.
- Public Hermes Agent docs for persistent memory, SOUL.md/personality,
  skills, toolsets, LSP diagnostics, MCP, local-provider setup, and
  execution backends.
- Public local-agent projects such as `ultraworkers/claw-code` as
  competitive reference, not as code to vendor.
- Private local product-pattern notes, when explicitly provided by their owner,
  as design input rather than source material.

## Product Boundary

Plume's durable shape:

- Desktop shell: Tauri + Rust + React + CodeMirror.
- Default mode: local-first, no default cloud calls.
- Primary object: an opened project folder, not a model server.
- Primary loop: inspect project, plan, propose/apply diff, verify.
- Provider layer: MLX-LM, Ollama, LM Studio, llama.cpp, and later
  OpenAI-compatible local servers such as vLLM if they fit.

Do not let Plume drift into a vLLM Studio clone. vLLM Studio and similar
tools manage model servers, recipes, dashboards, GPUs, remote deployment,
and observability. Plume can call those servers, but Plume's center of
gravity is the editor and coding workflow.

Also do not let Plume drift into an Ollama frontend. Ollama remains a
useful connected-runtime fallback, but Plume's preferred Mac path is
Plume-managed model weights through an efficient local runtime, especially
MLX on Apple Silicon. If the best Plume experience requires the user to
install and run Ollama first, the runtime track has missed the point.

Short positioning:

> vLLM Studio manages local model servers; Plume is the local coding desk
> that uses model servers to safely edit real projects.

For the full north-star note, see `docs/LOCAL_AGENT_NORTH_STAR.md`.

## Bounded Research Notes (Stage A)

The implemented research-note controller is a narrow artifact workflow, not
the coding-agent loop below. It accepts 1–10 exact `browserTextEvidence`
records already attached to one owning chat, re-resolves them in Rust, and uses
Apple On-Device, fixed Qwen Coder, or fixed Qwen2-VL to produce an inert Markdown
note. Qwen2-VL may additionally inspect exact owner-shelf Browser screenshot PNGs,
but at least one text record remains required for citation provenance. The
frontend never supplies source bodies or export paths.

The model protocol exposes exactly two internal text-framed submit actions:
one for a source summary and one for a draft. They do not execute host tools.
Each run is bounded to 13 logical turns and 26 provider calls. A logical turn
gets one shared recovery allowance: either a malformed-framing re-ask with the
parse error or one context-overflow repack. Recovery calls count toward the 26
provider-call ceiling but do not create another logical turn. Apple reports a
4,096-token context size on macOS 26.0–26.3 and exposes token counting only
from 26.4, so conservative estimation is the expected older-host path.

Stage A does no search, URL fetch, Browser navigation, arbitrary file read,
shell call, or broad tool invocation. Its only network-shaped traffic is model
transport: MLX loopback HTTP; Apple uses the bounded local helper. Memory,
topics, links, and ambient project context never join the bundle. Sources are
packed through the research-owned per-source resolver and ceilings, not the
chat resolver's separate 16-source/256 KiB aggregate.

Citation verification proves only that every `[[S#]]` marker names a source in
the immutable bundle. It does not prove truth or relevance, so `needsReview`
is an ordinary terminal state rather than an exceptional failure. Preview and
export receive projected Markdown footnotes as plain inert text; no link,
image, or HTML behavior is activated. Artifacts remain in bounded
session-local/project-session storage until the user explicitly exports one
through the native save panel.

The normal packaged Apple/Qwen Coder, recovery, Stop, review-needed, and export
paths are recorded. Qwen2-VL has direct-runtime screenshot proof, but its
exact-head packaged research/export proof remains pending, so Stage A stays
partial. Stage B network access and Stage C search are documentation-gated
candidates requiring separate authority review; neither is shipped.

## Runtime Pillars

### 1. Project Truth First

When a folder opens, Rust gathers a small `ProjectMeta` packet:

- `AGENTS.md`, then README.
- package/build config summaries.
- verification command candidates.
- git branch, dirty count, and changed files.
- presence of `CLAUDE.md`, flagged for consolidation.

This packet feeds the trust prompt and the first context packet. The model
does not start with a blank chat box.

### 2. Context Engine Before Agent Mode

The context engine is more important than the chat UI.

Inputs:

- user instruction
- selected text
- open file snippets
- explicitly attached files
- grep/search snippets
- git diff
- verifier output
- project rules
- compact session summary
- recent tool results

Rules:

- Never dump the whole repo.
- Never include generated folders by default.
- Redact secrets before model context.
- Prefer exact snippets with path and line anchors.
- Track why each item was included.
- For small local models, shrink context more aggressively.

Rust owns final prompt assembly. The frontend can help the user choose
attachments and mode, but raw project file content flows through Rust
redaction before reaching a provider.

### 3. Compaction Is A Core Feature

Local models need a real compaction ladder:

1. Micro-compact: trim stale tool output and repeated logs without asking
   the model.
2. Auto-compact: summarize older turns when the session reaches a context
   threshold.
3. Manual compact: user asks Plume to compress the session around a focus.

Compaction output should preserve:

- current task
- files touched or read
- accepted plan
- rejected attempts
- verifier results
- open questions
- permissions granted
- model/provider state

If compaction fails repeatedly, disable auto-compaction for that session
and tell the user. Silent repeated compaction failures waste tokens and
destroy trust.

### 4. Permissioned Tool Loop

Every tool call is typed, logged, cancellable, and permission-aware.

Minimum tool lifecycle:

1. Model requests a tool.
2. Rust validates arguments.
3. Safety layer checks project root, command policy, and approval ledger.
4. User approves if needed.
5. Runtime executes with cancellation support.
6. Result is streamed and summarized.
7. Session log records input, result, approval, and failure class.

Fail closed. Unknown tools, unknown commands, unsafe paths, destructive
operations, or ambiguous approvals require explicit user confirmation.

### 5. Agent Modes

Plume should keep staged autonomy. The names below are the `agentMode`
values from `docs/SAFETY.md`; pair each with an `approvalPolicy` rather
than treating the mode itself as a permission level.

| `agentMode`    | What the model does                                      | Local-model default         |
| -------------- | -------------------------------------------------------- | --------------------------- |
| `chat`         | Chat about visible/attached code                         | tiny / fast models          |
| `propose-diff` | Propose unified diff; user applies                       | small useful models         |
| `scoped-edit`  | Edit approved files; run approved verifier               | stronger local coder models |
| `agent-loop`   | Multi-step read/edit/test/fix with budget and allowlists | only explicit approval      |

Do not make `agent-loop` the default. For local models, the best UX is
often `propose-diff` with excellent context and diff review. Whether the
runtime asks on every write is a separate axis (`approvalPolicy`); see
`docs/SAFETY.md` for the full two-axis model.

### 6. Hooks

Hooks give deterministic control where prompts are too soft.

Internal hook events to design for:

- `SessionStart`
- `InstructionsLoaded`
- `PreToolUse`
- `PostToolUse`
- `PostToolUseFailure`
- `PermissionRequest`
- `PreCompact`
- `PostCompact`
- `FileChanged`
- `Stop`

MVP can keep hooks internal. Later, project hooks can live under
`.plume/hooks.toml` with strict command approval. Hooks must not become a
secret way for repo instructions to auto-run commands.

### 7. Memory As Index, Not Junk Drawer

Use a tiny always-loaded project memory index plus larger files loaded on
demand.

Possible layout:

```text
.plume/
  memory/
    INDEX.md
    topics/
      architecture.md
      commands.md
      user-preferences.md
  sessions/
    2026-05-03T12-00-00Z.jsonl
```

Rules:

- `INDEX.md` is short and pointer-like.
- Session logs are append-only.
- Topic files contain durable facts only.
- Convert relative dates to absolute dates.
- Remove contradicted facts instead of layering confusion.
- Never store secrets in memory.

Memory writes should be visible and reversible.

### 7.1 Sass Lessons: Distillation, Not Persona

The useful Sass reference is the working memory pipeline, not the
tsundere/waifu voice. Sass improves over time because it stores facts,
searches semantically, distills raw memories into compact profiles,
deduplicates similar entries, prunes stale low-value memories, caps growth,
and tracks recent responses to avoid repetition.

Plume should adapt that machinery to coding work:

- project facts instead of Discord-user facts,
- repo/session profiles instead of social profiles,
- task outcomes and verifier results instead of roast topics,
- recent assistant outputs to avoid repeated patch proposals,
- a scheduled or manual distillation pass that keeps memory small.

The first Plume memory slice should stay local and visible: `.plume/`
session logs, a tiny index, manual remember/forget, and SQLite FTS search.
Embeddings can follow once Plume has a local embedding path; cloud
embeddings must not be required for the default memory feature.

### 7.2 Hermes Lessons: Agent Harness

Hermes is a strong reference for an agent that grows with use: public docs
describe bounded memory, searchable sessions, reusable skills, SOUL.md,
toolsets, MCP, local/self-hosted provider support, background process
management, and LSP diagnostics.

Hermes also already supports local models through OpenAI-compatible
endpoints and integrations such as Ollama and LM Studio. That is useful
proof that local models can drive an agent harness, but it is not Plume's
whole angle. Hermes points at local model servers; Plume should own the
editor cockpit, model library, MLX-first runtime path, diff/apply/revert
safety layer, and project memory UX.

See `docs/HERMES_AGENT_RESEARCH.md` for the clean-room source pass over
Hermes' stream events, tool search, memory providers, session database,
prompt layering, hooks, and gateway/TUI patterns.

### 8. Reviewer Loop

Plume should have a built-in review posture, not just a generator posture.

Review mode reads:

- git status
- unstaged diff
- staged diff
- recent commit if clean
- relevant docs/contracts
- verifier output

Output starts with findings by severity and file/line. This mirrors the
handoff-review workflow and keeps AI-generated code honest.

### 9. Agent-Operable UI

Plume's visible desktop UI is the primary automation surface. A computer-use
agent should be able to drive the same controls a human uses: open a
project, trust it, browse files, approve commands, review diffs, cancel
runs, and inspect errors.

Rules:

- No hidden automation-only path for normal product workflows.
- Approval gates stay visible and actionable through the UI.
- Interactive controls need stable accessible names, roles, keyboard access,
  and visible focus.
- Status, errors, progress, and cancellation must be visible on screen, not
  only available in logs.
- The command palette can help agents, but it must expose the same powers as
  the UI, not bypass safety.

See `docs/AGENT_OPERABILITY.md` for the product contract.

## Lessons From Claw Code

`ultraworkers/claw-code` is a public Rust CLI agent harness with a broad
Claude-Code-like surface: REPL, one-shot prompts, provider routing,
permission modes, file and bash tools, sessions, slash commands, hooks,
MCP, plugins, skills, subagent surfaces, and a mock parity harness.

What it means for Plume:

- It is a serious reference point for Rust agent-runtime shape.
- It does not obsolete Plume because it is CLI-first, not a Tauri editor.
- It still relies on cloud/provider APIs by default; Plume's differentiator
  is local-model-first UX plus CodeMirror project editing.
- Its broad surface is also a warning: copying every Claude Code surface
  too early creates a huge maintenance burden.
- Its best lesson is not "copy the tools"; it is "typed events, parity
  harnesses, machine-readable state, and recovery loops matter."

Risk posture:

- Do not vendor or copy Claw Code source.
- Do not adopt any leaked-source-derived implementation.
- Public docs and behavior-level ideas are fair design references.
- Treat its existence as competitive pressure to make Plume's editor and
  safety story sharper, not as a reason to stop.

## Harness Radar: Hermes / Codex Lessons (latest pass)

A rolling read of public Hermes and Codex harness behavior, distilled into
architecture lessons for Plume. Source hygiene from the top of this doc still
applies: these are product/behavior patterns observed from public docs and
PRs, not copied schemas, prompts, or implementation text. Each lesson is
tagged with the pillar it touches and whether it is shipped, partial, or
roadmap. The reserved IPC shapes live in `docs/IPC_ROADMAP.md § Harness
radar`; the safety reasoning lives in `docs/SAFETY.md`.

### 1. Scoped progressive tool disclosure

A large flat tool catalog poisons a small model's context. The lesson is to
disclose tools in tiers — a tiny always-visible core set, the rest reached by
search and *scoped in* only for the turn that needs them. Plume has the
read-only half already (`tools.list` / `tools.search`, D92;
`docs/TOOL_DISCLOSURE.md`). The radar addition is **scoping**: disclosure
should be bounded per turn and per `agentMode`, so a `propose-diff` turn never
even sees a mutating tool. Pillar §4. Partial.

### 2. Writes-only approval mode

Between "ask on every action" and "trust everything" sits a useful middle:
read-only tools run freely, anything that writes prompts once, destructive
operations always prompt. This is an `approvalPolicy` value, not an
`agentMode` — it pairs with the two-axis model in §5 and `docs/SAFETY.md`. The
internal tool-risk metadata plus a pure "writes mode" policy helper (no new
execution) is the next code slice (D106). Pillars §4/§5. Roadmap.

### 3. Namespaced tool ids

Tool identity should carry its origin: a `namespace/tool` id (core, patch,
search, an MCP server, an engine) instead of a bare verb. Namespacing keeps
collisions impossible as optional/MCP tools grow, lets policy and disclosure
match on a prefix, and makes the event log legible about *whose* tool ran.
Plume's current `tool` field on agent events is a bare string; the radar moves
it to a namespaced id. Pillars §4/§6. Roadmap.

### 4. Typed event stream expansion

The harness reads better when every runtime happening is a typed event, not a
log line: tool lifecycle, approval, pause/resume, compaction, telemetry, and
failure classes each get a variant. Plume already ships a typed `AgentEvent`
union (D85) driving `AgentEventLog`. The lesson is to keep *expanding the
union* (not stringly-typed payloads) as new capabilities land — caps reached,
auto-pause, telemetry frames — so the UI and any parity harness stay
machine-readable. Pillar §4. Partial.

### 5. Per-turn tool and subagent caps

An agent loop needs hard ceilings: a max number of tool calls and a max number
of spawned subagents per turn, enforced by the runtime, not by asking the
model nicely. Caps are a runaway-cost and runaway-blast-radius backstop set
far above normal use, and hitting one is a typed event (see §4), not a silent
truncation. Especially important for small local models that can loop. Pillars
§4/§5. Roadmap.

### 6. Transport-failure auto-pause

When the provider transport drops mid-turn (server died, socket closed,
stream stalled), the loop should **auto-pause** and surface a resumable state,
not spin retries or fail silently. This matches Plume's "fail closed, stay
visible" posture (§4, §9) and the compaction rule that repeated failures must
tell the user. The pause is a typed event carrying a failure class. Pillars
§4/§9. Roadmap.

### 7. Memory delimiter / schema hardening

Memory entries that get folded into prompts are an injection surface: a
remembered line containing fake delimiters or role markers can try to escape
its slot. The lesson is to harden the memory store's schema and delimiters —
structured fields, escaped/validated content, no raw model-controlled text
spliced straight into the prompt frame. Plume already redacts secrets and caps
size on `memory.*` (D37); delimiter/role-marker hardening is the next layer.
Pillar §7 and §7.1. Partial.

### 8. Structured tool / inference telemetry

Every tool call and inference should emit structured telemetry — duration,
token counts, tokens/sec, failure class — as typed data, not prose. Plume
already carries generation stats on `chat.done` (D9); the lesson is to extend
the same structured-telemetry discipline to *tool* calls and to the agent loop
so cost and latency are inspectable and honest (the resource-honesty rule).
Telemetry frames ride the typed event stream (§4). Pillar §4. Partial.

### 9. Remote gateway auth + backend-workspace routing

When execution can leave the local box (a remote sandbox, an SSH/container
backend, a shared gateway), two things become non-negotiable: authenticated
access to the gateway, and explicit routing of each turn to a specific backend
*workspace* so work can't cross-contaminate between projects or sessions.
Plume is local-first and this stays firmly post-MVP (it lives near the
`engines.*` track and the computer-use Phase B gate), but the design is noted
now so the engine/gateway surface is reserved with auth and per-workspace
routing built in, not bolted on. Pillars §4; `docs/IPC_ROADMAP.md § External
agent engines`. Roadmap (post-MVP).

> Radar discipline: a lesson appearing here is a *design input*, not a commit.
> It graduates only when it lands in `docs/IPC_CONTRACT.md` with a schema, an
> error model, and a test — the same bar every other Plume capability clears.

## MVP Runtime Slice

Before model provider work gets fancy, build this sequence:

1. Project open and trust prompt.
2. Rust project scanner returns `ProjectMeta`.
3. File tree and CodeMirror open/read path.
4. Agent-operability pass for the visible file browser/editor workflow.
5. Context packet builder for selected file/text and project rules.
6. Provider interface can send one chat request to an existing local
   OpenAI-compatible endpoint.
7. Proposed diff display only.
8. Patch validation and apply after explicit approval.
9. Verification command detection and approved run.

This proves the coding-agent loop without pretending a small local model
can safely run full autonomy.

## Non-Goals For Now

- Multi-agent teams.
- Remote deployment.
- Docker orchestration.
- Full MCP marketplace.
- Automatic model downloads.
- Full plugin system.
- Cloud boost mode.
- Always-on background agent.

These may be good later, but they are distractions until the single
project-local coding loop is excellent.

## Open Design Questions

- Should `.plume/` memory/session files be created only after user trust?
- Which local model should be the first "known good" coding target?
- Should `propose-diff` use raw unified diff, structured edit
  operations, or both?
- How much of review mode should run without a model?
- Should Plume support a headless CLI later, or keep the desktop app as
  the only first-class surface?
