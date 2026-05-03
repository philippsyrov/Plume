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
- Public local-agent projects such as `ultraworkers/claw-code` as
  competitive reference, not as code to vendor.
- Local notes such as
  `/Users/philippsyrov/Desktop/CS Projects/Sass/.claude/reference-claude-code-patterns.md`
  as product-pattern notes, not as source material.

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

Short positioning:

> vLLM Studio manages local model servers; Plume is the local coding desk
> that uses model servers to safely edit real projects.

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

## MVP Runtime Slice

Before model provider work gets fancy, build this sequence:

1. Project open and trust prompt.
2. Rust project scanner returns `ProjectMeta`.
3. File tree and CodeMirror open/read path.
4. Context packet builder for selected file/text and project rules.
5. Provider interface can send one chat request to an existing local
   OpenAI-compatible endpoint.
6. Proposed diff display only.
7. Patch validation and apply after explicit approval.
8. Verification command detection and approved run.

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
