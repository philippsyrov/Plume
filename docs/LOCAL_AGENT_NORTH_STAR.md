# Local Agent North Star

Plume's goal is not to become an Ollama skin.

Plume's goal is to become a local-first coding agent cockpit: the user
opens a project, chooses or imports local model weights, Plume runs the
model in the most efficient local path available, and the agent can read,
edit, test, remember, and improve under visible safety controls.

Plain version:

> Plume should make local open models feel like a real coding agent, not
> like a chat box duct-taped to a repo.

This matters because the target user is not only the person who can pay
for frontier cloud agents. It includes students and indie hackers who
have a laptop, open model weights, and no appetite for another monthly
subscription. The product should make that path useful without lying
about local-model limits.

## Product Stance

### MLX-first on Apple Silicon

For Mac users, the preferred path is Plume-managed MLX where possible:

1. The user imports, downloads, or points Plume at model weights.
2. Plume stores or indexes them under the local model library.
3. Plume verifies what format they actually are.
4. Plume estimates memory and context cost before load.
5. Plume starts and supervises the runtime.
6. Plume routes chat, diff, edit, and agent-loop calls through that
   runtime.

Gemma and Qwen MLX-format checkpoints are the practical first targets.
If the user already has optimized weights in another local app, Plume
should eventually let them import or reference those weights instead of
forcing a duplicate download.

The key word is **verified**. A folder that looks like a HuggingFace
checkpoint is not automatically MLX. Plume must not label a model as
MLX unless it has checked enough on disk to justify that claim.

D36 landed the first floor of this verification: the local model
inventory upgrades a `transformer-folder` to `mlx-folder` only when
either `weights.npz` is present at the folder root OR `config.json`
carries the MLX-LM quantization shape (`{"quantization":
{"group_size": _, "bits": _}}`). HuggingFace's `quantization_config`
field is intentionally not sufficient — that key is bitsandbytes /
HF-quantized, not MLX. Unquantized MLX safetensors uploads can look
identical to a vanilla HF safetensors checkpoint on disk; those stay
in `transformer-folder` rather than risk a false-positive claim.

The runtime contract for the Plume-managed MLX-LM server is pinned
in `docs/MLX_RUNTIME.md`: verified `mlx_lm.server` CLI shape,
default 127.0.0.1:8080 with a port-allocation plan that dodges
llama-server's 8080, GET /health for ready detection, OpenAI-style
SSE chat on POST /v1/chat/completions, and SIGINT-graceful shutdown.
That doc is the implementation-ready contract the next runtime
slice consumes; D38 ships the doc only.

### Ollama is compatibility, not the center

Ollama is useful because many users already have it and it exposes an
HTTP API. Plume can keep supporting it as a connected runtime. But it is
not the product's best path for Mac-local performance, memory honesty, or
model ownership.

Plume should not require the user to run Ollama to get the real Plume
experience. If the only happy path is "install Ollama, pull a model, then
point Plume at it", Plume has failed its own local-agent goal.

### Benchmark and publishing posture

MLX-first is a product direction, not a license to make unmeasured speed or
quality claims. When the 128 GB Apple Silicon machine arrives, Plume will use
the contract in `docs/MODEL_BENCHMARKS.md` to measure the same exact weights,
revision, quantization, fixture, and configuration across Plume-managed
MLX-LM, Ollama MLX, and llama.cpp. Raw runtime inference, Plume orchestration,
resource use, context scaling, agent quality, and reliability stay separate.

Public performance language must link to the recorded hardware,
configuration, fixture, raw result, and Plume commit. The result contract
labels measured facts, inferences, and marketing copy separately. Until D68
implements the harness and D69 turns its evidence into product copy, Plume
has no new benchmark result to advertise.

### Local agent, not only local model

Local model serving is only one layer. A serious agent also needs:

- project truth
- context selection
- tool permissions
- safe file edits
- rollback
- verification
- session memory
- skill/procedure memory
- visible progress and cancellation

Plume already has the beginning of this: project trust, file browsing,
chat, propose-diff, validation, apply, checkpoints, revert, and a
provider panel. The next step is making the local runtime and memory
layers first-class instead of treating them as optional extras.

## Lessons From Hermes Agent

Hermes is useful as a competitive reference because it is an agent that
tries to improve across sessions, not just a stateless chat client.
Public Hermes docs show several patterns Plume should adapt cleanly:

- Persistent memory: small curated memory files plus searchable session
  history.
- User/profile memory: stable facts about the user and preferences.
- Skill memory: reusable procedures saved as skills, loaded only when
  relevant.
- SOUL.md-style personality: durable baseline identity separate from
  project instructions.
- Toolsets: explicit bundles of tools that can be enabled or disabled.
- MCP and plugins: extension points without bloating the core.
- Background process management: start, poll, log, wait, kill, and write
  to long-running processes.
- LSP diagnostics after writes: semantic feedback, not only "file wrote".
- Sandboxed execution backends: local, container, SSH, or remote
  execution environments.
- Session search: SQLite/FTS-style recall over past work.

Hermes already works with local models through OpenAI-compatible
endpoints and first-class local integrations such as Ollama or LM
Studio. That means Hermes can run with local inference, assuming the
local model is strong enough, has enough context, and supports the tool
patterns the task needs.

That does **not** erase Plume's angle.

Hermes is primarily an agent runtime that can point at local servers.
Plume is a desktop coding workspace that should own the model library,
the editor, the visible safety layer, the diff/revert path, and the
MLX-first Mac experience. Hermes proves the feature class is valuable;
Plume's job is to make the local coding version feel native, inspectable,
and cheap to run.

The deeper clean-room Hermes source pass lives in
`docs/HERMES_AGENT_RESEARCH.md`. Treat that doc as the source for future
Plume slices inspired by Hermes/Teknium patterns.

## Harness Radar (local-first read)

The canonical, pillar-by-pillar list of the latest Hermes/Codex harness
lessons lives in `docs/AGENT_RUNTIME.md § Harness Radar`. This section reads
the same nine lessons through the north-star lens — *which ones matter most
when the model is small, the laptop is modest, and there is no cloud
fallback* — so they slot into the capability roadmap below rather than drift
as abstract harness envy. Clean-room as always: behavior patterns, not copied
schemas.

For a small local model, the load-bearing lessons are the ones that protect
context and stay honest:

- **Scoped progressive tool disclosure** and **per-turn tool/subagent caps**
  are not nice-to-haves here — they are how a 3B–8B model keeps a clean,
  bounded job instead of drowning in a tool dump or looping forever. These
  directly serve the "smaller models get a clean job" thesis.
- **Writes-only approval mode** is the approval posture that fits
  `propose-diff` / `scoped-edit`: let read/search run, prompt once on a write,
  always prompt on destructive. It keeps the safe path frictionless without
  pretending a small model earned full autonomy. The internal tool-risk
  metadata + pure policy helper is the next code slice (D106).
- **Memory delimiter/schema hardening** matters more locally, not less: the
  same memory that makes a cheap model feel competent across sessions is a
  prompt-injection surface if a remembered line can forge delimiters. This
  extends the "small, inspectable, reversible, never grants permission" memory
  rules above, adding "never escapes its slot."
- **Structured tool/inference telemetry** is the backbone of resource honesty
  — the product promise to laptop users. Per-tool and per-loop duration / token
  / failure-class data is what lets Plume tell the truth about cost instead of
  hiding a slow local run.
- **Typed event stream expansion** and **transport-failure auto-pause** keep a
  local session legible and recoverable when a hand-managed runtime hiccups —
  exactly the failure mode a self-hosted model path hits more often than a
  hosted API.
- **Namespaced tool ids** and **remote gateway auth + backend-workspace
  routing** are mostly forward-looking for the local build; the gateway/routing
  lesson stays firmly post-MVP (it only bites once execution can leave the
  box), but is noted so the `engines.*` surface reserves auth + per-workspace
  routing from the start rather than retrofitting it.

None of this changes the north star: build the harness first, then add agency
only where the model and the visible safety layer can support it. The radar
just sharpens *which* harness pieces pay off first for the local-first user.

## Lessons From Sass

The useful Sass lesson is not the tsundere/waifu voice. That belongs to
that Discord bot. The reusable lesson is the memory machinery that made
the bot improve with use.

Sass has working versions of these patterns:

- Explicit remember flow: users can make the bot store facts.
- Semantic memory search: stored facts can be retrieved by meaning, not
  only by exact text.
- Distilled profiles: raw facts, memories, and interaction history are
  periodically compressed into compact per-user summaries.
- Memory cleanup: duplicate memories are removed, stale low-value
  memories are pruned, and each user has a hard cap.
- Periodic distillation: a delayed startup pass and recurring interval
  keep memory from becoming a junk drawer.
- Recent-response memory: the bot sees its last few outputs so it avoids
  repeated phrasing.
- Context-aware behavior: the runtime chooses how much memory to fetch
  based on whether the user is asking a normal question or a history /
  memory question.
- Bounded state: maps, cooldowns, and caches have size caps or cleanup.

The Plume adaptation should be about coding work:

- project facts instead of Discord-user facts
- repo/session profiles instead of social profiles
- task memory instead of roast memory
- verifier and edit outcomes instead of message reactions
- recent assistant outputs to avoid repetitive patch proposals
- compact session summaries to survive long local-model sessions

The rule is simple: memory should make future work better, but it must
stay small, inspectable, and reversible.

## Plume Memory Design Direction

Start with local project memory only.

Suggested layout:

```text
.plume/
  memory/
    INDEX.md
    USER.md
    SOUL.md
    topics/
      architecture.md
      commands.md
      testing.md
      decisions.md
  sessions/
    state.sqlite
    logs/
      2026-05-15T12-00-00Z.jsonl
```

### Always-loaded memory

Keep this tiny:

- `INDEX.md`: pointers to durable project facts and topic files.
- `USER.md`: user preferences relevant to Plume, such as explanation
  style and workflow.
- `SOUL.md`: the agent's durable voice/personality baseline.

These files should have strict size caps. They are prompt fuel, so every
line costs tokens.

### Searchable memory

Use SQLite with FTS first. It is local, simple, fast, and dependency-light.

Later, add local embeddings when there is a good local embedding path.
Do not require a cloud embedding API for Plume's main memory feature.

### Distillation loop

Add a local "dream" or "distill" pass after the basic logs exist:

1. Read recent session logs and memory files.
2. Extract facts worth keeping.
3. Convert relative dates to absolute dates.
4. Remove contradicted facts instead of stacking both versions.
5. Merge duplicates.
6. Prune stale entries.
7. Keep indexes under cap.
8. Write a visible summary of what changed.

The first version can be manual: user clicks "Distill memory". Later it
can run in the background only when the app is idle and the project is
trusted.

### Memory MVP (D37, landed)

The smallest visible floor is in place. D37 added a flat JSONL store
at `<project>/.plume/memory/entries.jsonl` plus three IPC verbs:

- `memory.index` — read entries + limits + on-disk size.
- `memory.remember` — append a redacted text entry.
- `memory.forget` — remove an entry by id, idempotent.

The Memory panel (left column, togglable from the chip strip) shows
the current entries with a small input for adding new ones and a
Forget button per row.

Caps: 100 entries, 1 KiB per entry, 64 KiB total file size.
Reaching any cap rejects with `capacityReached` until the user
forgets one. The pre-redaction text never reaches disk — every
remembered string passes through the same `prompts::redact` secret
redactor used by the prompt-read pipeline, so an `sk-…` or `ghp_…`
shows up as `[REDACTED:<kind>]` in the stored entry. A small
`N redacted` badge surfaces this to the user.

Symlinks at `.plume/` are refused (same guard as the patch
checkpoint dir). Entry ids are validated as `m_[0-9a-fA-F]{32}` —
nothing path-shaped slips through `memory.forget`.

Out of scope for D37 (reserved for follow-ups):

- Topic files (`INDEX.md`, `USER.md`, `SOUL.md`, `topics/`).
  (Read floor landed in D71: `memory.topics` reads + surfaces the
  curated Markdown, capped and symlink-safe; the user authors them
  in their editor. Wiring the always-loaded trio into the chat
  prompt context is the D72 follow-up.)
- SQLite-backed session search / FTS.
- Local embeddings, semantic recall.
- Distillation passes.
- Background dream / cleanup jobs.

### Memory safety rules

- Never store secrets.
- Never let memory grant permissions.
- Never hide memory writes.
- Every memory write needs an undo/delete path.
- The user can inspect memory from the UI.
- Session logs are append-only.
- Project memory belongs under the project `.plume/` folder.
- Global user memory belongs in the app data directory, not inside every
  repo.

## Personality Without Wasting Local Context

Personality should be thin, durable, and cheap.

Plume should not spend half the context window telling a 4B model how to
be charming. It needs a short baseline identity that shapes tone without
burying the coding task.

Suggested split:

- `SOUL.md`: stable Plume voice and behavior.
- `USER.md`: what the user prefers.
- `AGENTS.md`: project-specific coding rules.
- Session mode overlay: temporary posture such as reviewer, tutor, or
  driver.

Example baseline:

```text
You are Plume, a local-first coding agent.
Be direct, careful, and useful.
Prefer small safe edits over clever rewrites.
When unsure, inspect files before answering.
Never pretend a local model can do more than it can.
```

That is enough. The agent's "personality" should come mostly from
consistent behavior: remembering useful facts, avoiding repeated
mistakes, giving honest resource warnings, and improving the workflow
over time.

## Skills / Procedural Memory

Plume should eventually learn procedures, not just facts.

Examples:

- "How this repo verifies releases."
- "How to run the Tauri packaged smoke test."
- "How to add a new provider adapter."
- "How to review a patch.apply change safely."

Store these as small skill documents with:

- name
- description
- when to use
- exact steps
- verification
- known failure modes

Use progressive disclosure: list skill names/descriptions first, load the
full skill only when it is actually relevant. Skills must not auto-grant
tool permissions.

## Agent Capability Roadmap

The correct sequence is:

1. **MLX runtime ownership** - direct Plume-managed model path.
2. **Memory MVP** - visible local memory, manual remember/forget, FTS
   session search.
3. **Scoped edit mode** - approved files, patch apply/revert, verifier.
4. **Distillation** - summarize sessions into durable project memory.
   (v1 rule-based dedupe landed: D48 preview scaffold → D54 preview
   IPC/UI → D64 `memory.distillApply` compaction. LLM-summary v2 is
   still roadmap; see `docs/MEMORY_DISTILLATION.md`.)
5. **Skills** - reusable local procedures loaded on demand.
6. **Agent loop** - bounded read/edit/test/fix with budget and stop
   conditions.
7. **LSP diagnostics** - semantic feedback after edits.
8. **Computer use Phase A** - in-app/browser sandbox.
9. **Computer use Phase B** - host desktop control, per-session opt-in,
   target allowlist.

Do not start with full autonomy. Build the harness first, then increase
agency only where the model and safety layer can support it.

## Competitive Summary

Hermes is ahead on broad agent features: memory, skills, gateways,
toolsets, plugins, and execution backends.

Plume can still win a real niche:

- MLX-first local model management on Mac.
- Native desktop editor, not a terminal-only agent.
- CodeMirror project workspace with visible diff/apply/revert.
- Resource honesty for small laptops.
- Local project memory designed for coding work.
- Safety gates visible to both humans and accessibility agents.
- No default cloud dependency.

The product should be judged by this question:

> Can a student with a normal laptop and open weights get a useful,
> private coding-agent workflow without paying for a frontier cloud
> subscription?

If yes, Plume is doing the thing.

## Public Research Sources

- Hermes Agent memory:
  https://hermes-agent.nousresearch.com/docs/user-guide/features/memory
- Hermes Agent memory providers:
  https://hermes-agent.nousresearch.com/docs/user-guide/features/memory-providers
- Hermes Agent skills:
  https://hermes-agent.nousresearch.com/docs/user-guide/features/skills
- Hermes Agent personality / SOUL.md:
  https://hermes-agent.nousresearch.com/docs/user-guide/features/personality
- Hermes Agent tools:
  https://hermes-agent.nousresearch.com/docs/user-guide/features/tools
- Hermes Agent tool search:
  https://hermes-agent.nousresearch.com/docs/user-guide/features/tool-search
- Hermes Agent PR #34493:
  https://github.com/NousResearch/hermes-agent/pull/34493
- Hermes Agent PR #37250:
  https://github.com/NousResearch/hermes-agent/pull/37250
- Hermes Agent PR #37405:
  https://github.com/NousResearch/hermes-agent/pull/37405
- Hermes Agent PR #38350:
  https://github.com/NousResearch/hermes-agent/pull/38350
- Hermes Agent PR #38352:
  https://github.com/NousResearch/hermes-agent/pull/38352
- Hermes Agent PR #38232:
  https://github.com/NousResearch/hermes-agent/pull/38232
- Hermes Agent issue #625:
  https://github.com/NousResearch/hermes-agent/issues/625
- Hermes Agent issue #523:
  https://github.com/NousResearch/hermes-agent/issues/523
- Hermes Agent MCP:
  https://hermes-agent.nousresearch.com/docs/user-guide/features/mcp
- Hermes Agent providers:
  https://hermes-agent.nousresearch.com/docs/integrations/providers
- LM Studio Hermes integration:
  https://lmstudio.ai/docs/integrations/hermes
- Ollama Hermes integration:
  https://docs.ollama.com/integrations/hermes

Local project references:

- `/Users/philippsyrov/Desktop/CS Projects/Sass/.claude/reference-claude-code-patterns.md`
- `/Users/philippsyrov/Desktop/CS Projects/Sass/src/vectorMemory.ts`
- `/Users/philippsyrov/Desktop/CS Projects/Sass/src/userMemory.ts`
- `/Users/philippsyrov/Desktop/CS Projects/Sass/src/responseMemory.ts`
- `/Users/philippsyrov/Desktop/CS Projects/Sass/CLAUDE.md`
