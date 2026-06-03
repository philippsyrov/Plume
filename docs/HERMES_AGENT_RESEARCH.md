# Hermes Agent Clean-Room Research

This note records a clean-room research pass over the public
`NousResearch/hermes-agent` repository, public Hermes docs, selected
Teknium-authored PR/issue writeups, and the user's local Hermes backup
shape.

It is a design input for Plume, not an implementation source. Do not copy
Hermes source code into Plume. The useful output is behavior, architecture,
tests, and product judgment.

## Scope

Read in this pass:

- `gateway/stream_events.py`
- `gateway/stream_dispatch.py`
- `gateway/stream_consumer.py` at a high level
- `tools/tool_search.py`
- `tests/tools/test_tool_search.py`
- `hermes_state.py`
- `agent/memory_manager.py`
- `agent/memory_provider.py`
- `agent/system_prompt.py`
- `gateway/hooks.py`
- `tui_gateway/server.py`
- `tui_gateway/ws.py`
- `apps/shared/src/json-rpc-gateway.ts`
- selected desktop store files under `apps/desktop/src/store/`
- selected public docs under `website/docs/user-guide/features/`
- selected PR/issue writeups:
  - PR #34493, tool search
  - PR #37250, structured stream-event protocol
  - PR #37405, desktop WebSocket origin guard
  - PR #38350, remote desktop connect needs `--tui`
  - PR #38352, at-rest scroll jump-up during code-block highlight
  - PR #38232, observer telemetry hooks
  - issue #625, structured temporal memory
  - issue #523, local model setup skill

Not read in this pass:

- every file in the Hermes repo,
- every plugin,
- every platform adapter,
- private secrets or auth files from the user's Hermes backup,
- Twitter/X posts except where the user supplied screenshots.

## Top-Level Takeaway

Hermes is not just a chat wrapper. It is a persistent agent runtime with:

- typed presentation events,
- a tool registry and progressive tool disclosure,
- SQLite-backed session history and search,
- memory-provider lifecycle hooks,
- prompt layers designed around cache stability,
- platform adapters that render or suppress agent events,
- gateway/TUI surfaces,
- hooks and observability,
- skills and curation,
- many regression tests around weird real-world failures.

Plume should not try to clone that whole product. Plume's bet is still
different: local model ownership, MLX-first Mac runtime, project editor,
safe diff/apply/revert, visible approvals, and coding-focused memory.

But Hermes shows the runtime spine Plume will need once local models become
good enough to drive real tools.

## Patterns To Adapt

| Hermes Pattern | What It Does | Plume Adaptation | Priority |
| --- | --- | --- | --- |
| Structured stream events | Agent emits typed presentation events; gateway/platform decides rendering. | Add a typed `agent.event` vocabulary for chat, tool calls, notices, diagnostics, and future computer-use traces. | High |
| Progressive tool disclosure | Core tools stay direct; deferrable plugin/MCP tools hide behind `tool_search`, `tool_describe`, `tool_call` when schemas become too large. | Keep Plume core tools always visible; defer model-library/plugin/browser tools when schema cost crosses a context budget. | High |
| SQLite session spine | Sessions/messages live in SQLite with FTS, token/cost/tool metadata, parent session lineage. | Move from window-only transcript to `.plume/sessions/state.sqlite` with FTS and compaction lineage. | High |
| Memory provider lifecycle | Memory has initialize, prefetch, sync, session switch/end, pre-compress, write mirror, delegation hooks. | Evolve Plume memory from JSONL CRUD into a local provider lifecycle with distillation and provenance. | High |
| Prompt cache tiers | Stable, context, and volatile prompt layers are explicit. | Keep Rust prompt assembly layered so local models get small, stable prefixes and tiny volatile tails. | High |
| Adapter render/eat model | Platform adapters can render or intentionally drop events they cannot show. | Desktop UI, future CLI, computer-use trace, and headless tests should render the same event stream differently. | Medium |
| Observer hooks | Lifecycle hooks can emit rich traces when listeners exist, while keeping no-listener overhead low. | Add optional local trace hooks for model calls, runtime starts, patch apply/revert, verifier runs, approvals, and memory writes. | Medium |
| Remote readiness contracts | Health can lie if required channels are not actually enabled. | Any Plume remote/runtime readiness check must prove the actual required route/socket/tool channel, not only `/health`. | Medium |
| Desktop WebSocket origin guard | Auth first, then explicit desktop-origin policy; browser/DNS-rebind stays strict. | If Plume ships a remote desktop bridge, design origin/auth policy before opening sockets. | Medium |
| UI browser-regression tests | Layout bugs need real rendered tests, not only unit tests. | Add screenshot/browser/Tauri smoke checks for the workspace shell, chat input, local-model rows, and overflow states. | High for UI |
| Preview caps vs full logs | Visible trails are bounded separately from full logs. | Keep chat/tool/runtime previews small; store full logs separately in bounded files or SQLite. | Medium |
| Rate-limit/resource states | Background workers distinguish rate-limited/resource-blocked from crashed. | Future Plume agent queues should have `blocked_on_resource`, not only success/failure. | Medium |

## Structured Stream Events

Hermes' `gateway/stream_events.py` defines frozen dataclasses for the
presentation stream:

- `MessageChunk`
- `MessageStop`
- `Commentary`
- `ToolCallChunk`
- `ToolCallFinished`
- `LongToolHint`
- `GatewayNotice`

`gateway/stream_dispatch.py` routes those events through a platform adapter
and a delivery sink. The key design move is separation:

- agent history remains the source of truth,
- stream events describe presentation,
- platform adapters decide how to render,
- adapters can return nothing to intentionally eat tool chrome,
- presentation failures do not crash the agent loop.

Plume already has `chat.token` and `chat.done`, but that is too narrow for a
real agent. The next version should be a typed event stream, for example:

```text
agent.message.delta
agent.message.stop
agent.tool.started
agent.tool.finished
agent.approval.requested
agent.runtime.notice
agent.patch.validated
agent.patch.applied
agent.verifier.started
agent.verifier.finished
agent.computer.observe
agent.computer.action
```

That gives every surface the same facts:

- desktop chat,
- diagnostics panel,
- future CLI/headless runner,
- computer-use trace,
- tests,
- session recorder.

Plain English: the model should not paint the UI with text. The runtime
should emit Lego bricks, and each surface builds the display it can actually
support.

## Progressive Tool Disclosure

Hermes' tool search design is directly relevant to Plume because local
models cannot afford huge schema lists.

Core findings from `tools/tool_search.py` and
`website/docs/user-guide/features/tool-search.md`:

- core tools never defer,
- bridge names are reserved,
- config supports `auto`, `on`, and `off`,
- auto mode activates when deferrable schemas exceed a context percentage,
- token estimate uses a cheap char-count heuristic,
- catalog rebuilds from live tool definitions every assembly,
- retrieval uses BM25 over name, description, and parameter names,
- substring fallback handles exact-ish names,
- `tool_call` unwraps to the real tool so hooks, approvals, guardrails, and
  display all see the underlying tool.

The tests are as important as the feature. `tests/tools/test_tool_search.py`
pins:

- invalid config fallback,
- core tools never defer,
- unknown tools stay visible rather than silently disappearing,
- threshold behavior,
- retrieval ranking,
- substring fallback,
- bridge tool idempotency,
- scope/visibility regressions.

Plume adaptation:

- Always visible:
  - file read,
  - file search,
  - patch validate/apply/revert,
  - memory,
  - verifier,
  - stop/cancel,
  - model/runtime diagnostics.
- Deferrable:
  - plugin tools,
  - MCP tools,
  - model-library download/import actions,
  - rarely used browser/computer-use tools,
  - optional GitHub/Hugging Face connectors.
- Hard rule:
  - search visibility is permission;
  - if a scoped edit session cannot call a tool, it must not search or
    describe that tool either.

This should become a Plume design doc before code:
`docs/TOOL_DISCLOSURE.md`.

## Session Database And Search

`hermes_state.py` is a serious persistent state layer:

- SQLite `state.db`,
- WAL mode with fallback to DELETE on filesystems where WAL breaks,
- schema versioning,
- `sessions` and `messages` tables,
- FTS5 search,
- trigram FTS for substring/CJK-style search,
- parent session lineage,
- token counts,
- tool-call count,
- cost fields,
- session source,
- archive/rewind/compression metadata,
- pruning helpers,
- migration/rebuild logic.

Plume's transcript still mostly behaves like window state. That is fine for
early slices, but not enough for an agent that gets better over time.

Plume adaptation:

```text
.plume/
  sessions/
    state.sqlite
      sessions
      messages
      message_fts
      tool_calls
      approvals
      verifier_runs
      model_events
```

Start with local project sessions only. Global profile memory can come later.

## Memory Provider Lifecycle

Hermes' memory system is not just a file. `agent/memory_provider.py` defines
provider lifecycle hooks:

- `initialize`
- `system_prompt_block`
- `prefetch`
- `queue_prefetch`
- `sync_turn`
- `get_tool_schemas`
- `handle_tool_call`
- `shutdown`
- `on_turn_start`
- `on_session_end`
- `on_session_switch`
- `on_pre_compress`
- `on_memory_write`
- `on_delegation`

`agent/memory_manager.py` orchestrates providers and includes a streaming
scrubber for `<memory-context>` so recalled memory context cannot leak into
visible assistant text across chunk boundaries.

Important Plume lessons:

- Memory writes and memory context are different things.
- Prefetch should be fast and ideally one turn ahead.
- Memory provider failures should not block the whole agent.
- External provider count should be controlled to avoid tool/schema bloat.
- Memory context needs explicit fencing and output scrubbers.
- Compression is a memory boundary, not only a context-size trick.
- Subagent results and delegation summaries are memory events.

Plume's current JSONL memory MVP is a good floor. The future shape should be
a local memory provider with:

- provenance,
- confidence,
- timestamps,
- source session/message ids,
- delete/revert,
- distillation preview/apply,
- token-aware injection,
- local search,
- optional local embedding path.

Issue #625's temporal memory idea maps well:

```text
topOfMind
workContext
recentSessions
recentMonths
earlierContext
longTermBackground
facts(confidence, source, category, createdAt)
```

## Prompt Layers

`agent/system_prompt.py` makes the cache contract explicit:

- stable layer:
  - identity,
  - tool guidance,
  - skills,
  - model-family guidance,
  - environment hints.
- context layer:
  - caller system message,
  - project/context files.
- volatile layer:
  - memory snapshot,
  - profile,
  - external memory provider block,
  - timestamp/session/model/provider line.

Plume should keep this in Rust prompt assembly. Local models benefit from
clean stable prefixes and small volatile tails. This also makes prompt
debugging easier because each packet can be previewed independently.

Candidate Plume shape:

```text
PromptPacket {
  stable: StablePrompt,
  project: ProjectPrompt,
  memory: MemoryPrompt,
  task: TaskPrompt,
  tools: ToolPrompt,
  volatile: RuntimePrompt,
}
```

## Hooks And Observer Telemetry

Hermes has `gateway/hooks.py` for user hooks and PR #38232 adds an observer
style surface for session/turn/API/tool/approval/subagent lifecycle events.

The important pattern is not "add plugins everywhere." It is:

- hooks are typed lifecycle events,
- hook failures do not block the main pipeline,
- rich payload construction is gated so the no-listener path stays cheap,
- events carry correlation ids so a trace can connect a model request, tool
  call, approval, result, and final answer.

Plume adaptation:

- Add local trace hooks around:
  - model request,
  - runtime start/stop,
  - provider health probe,
  - tool call,
  - approval,
  - patch validate/apply/revert,
  - verifier run,
  - memory write/distill.
- Store traces locally.
- Keep no-observer overhead near zero.
- Make traces visible in the UI later, not just logs.

## Gateway, Desktop, And Remote Lessons

The TUI gateway source and Teknium PR writeups surface three practical
lessons:

1. A status endpoint can lie.
   PR #38350 fixed documentation where the backend could say "ready" while
   `/api/ws` and `/api/pty` were unavailable because the embedded TUI surface
   was not enabled. Plume should never treat "process up" as "agent channel
   ready."

2. WebSocket origin policy is subtle.
   PR #37405 handles desktop `file://` / `null` origins differently from
   browser origins, while keeping auth and DNS-rebind protections strict.
   If Plume ships a remote bridge, this must be designed before code.

3. Long handlers need async dispatch and cancellation.
   `tui_gateway/server.py` routes slow handlers through a small thread pool so
   interrupt/approval requests do not sit unread behind a blocking command.
   Plume's future tool loop should keep approvals, cancel, and stop responsive
   even while a verifier or model call is running.

## UI Reliability Lessons

Hermes' desktop UI issues are very relevant because Plume already had
overlaps, hidden chat input, and row wrapping bugs.

Two rules:

- Unit tests catch logic. Browser/screenshot tests catch layout.
- Visible previews must have independent caps from full logs.

Plume should add rendered UI checks for:

- trusted project shell at narrow/normal/wide sizes,
- chat input visible with selected local model,
- local-model row with selected/running/source/kind badges,
- diagnostics disclosure open,
- inspector hidden/shown,
- no-project chat mode,
- long assistant/tool output.

`happy-dom` is useful, but it cannot prove flex layout. We need a real
browser/Tauri smoke step for the UI shell.

## Skills And Curator

Hermes treats skills as reusable procedures and has a curator concept that
can review, archive, consolidate, and back up skills.

Plume should not jump straight into agent-authored skills. The safer order:

1. Record repeated workflows in session history.
2. Let the user manually promote a workflow to a project skill.
3. Store it under `.plume/skills/`.
4. Show a preview and require approval before edits.
5. Snapshot before curator changes.
6. Let future sessions retrieve skills by search.

Skills should be project-local first. Global user skills are later.

## Local Model Setup

Hermes can make local model setup a skill because it mostly points at model
servers. Plume should make local model setup native UX because that is the
product.

Plume should own:

- discover local caches,
- import a folder,
- download a verified variant,
- classify format,
- estimate memory,
- launch runtime,
- run text smoke,
- run image/audio smoke when supported,
- store "known good" capability results.

This is the difference between "agent that can use local inference" and
"local-model coding cockpit."

## Candidate Plume Slices

Recommended next slices after the current UI cleanup:

1. **D64 rendered UI smoke harness.**
   Add browser/Tauri screenshot checks for the trusted shell, chat form,
   local-model rows, and diagnostics disclosure. This addresses the actual
   current pain before adding more runtime surface.

2. **D65 session SQLite design.**
   Draft `docs/SESSION_STORE.md`: schema, FTS, lineage, compaction, tool
   calls, approvals, verifier runs, retention, privacy.

3. **D66 typed agent event protocol.**
   Draft `docs/AGENT_EVENT_PROTOCOL.md` and reserve IPC events. Keep code
   minimal until the UI shell is stable.

4. **D67 memory provider lifecycle design.**
   Extend `docs/MEMORY_DISTILLATION.md` with provider lifecycle,
   provenance/confidence/time, prefetch, sync, session switch/end, and
   compression hooks.

5. **D68 progressive tool disclosure design.**
   Draft `docs/TOOL_DISCLOSURE.md`: core tools, deferrable tools, threshold,
   catalog scope, search ranking, bridge unwrapping, approval/logging
   behavior.

6. **D69 observer telemetry design.**
   Draft local trace event schema with correlation ids and no-listener
   overhead rules.

7. **D70 model capability registry.**
   Add a local record that says a model is present, launches, chats, handles
   images, handles audio, supports tools, and what smoke proved it.

## What To Avoid

- Do not copy Hermes source.
- Do not copy their broad platform matrix too early.
- Do not ship a giant plugin system before Plume's editor loop is good.
- Do not treat Ollama/LM Studio as Plume's happy path.
- Do not let memory become invisible or permission-granting.
- Do not call a runtime "ready" until the actual chat/tool path works.
- Do not trust UI layout from unit tests alone.

## Bottom Line

Hermes is a strong agent runtime. Plume should learn from its spine:
typed events, scoped tools, persistent sessions, memory lifecycle, hooks,
tests, and remote safety.

Plume should not become Hermes. Plume should become the best local coding
desk for open models: own the weights, run them efficiently, show the user
what is happening, edit safely, verify, remember, and improve.
