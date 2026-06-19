# Progressive Tool Disclosure

Design for Plume's local tool catalog and tool search (D86). This is a
**design doc + a pure types scaffold** (`src-tauri/src/agent/catalog.rs`),
not a wired feature: nothing assembles a prompt from it yet, and there is
no MCP integration and no tool execution. It fixes the model both ends
will share before the executing slice plugs a real tool registry in.

It is the doc `docs/HERMES_AGENT_RESEARCH.md § Progressive Tool
Disclosure` reserved. The design is **clean-room and Hermes-inspired**:
the *idea* (core tools stay direct, the long tail hides behind a search
verb) is borrowed; none of Hermes' code, scoring, or schemas are. Plume's
ranking, tiers, and types are its own and deliberately simpler.

## The problem

A local model has a small, precious context window. Serializing every
tool's full JSON schema into the system prompt — name, description, every
parameter — costs hundreds to thousands of tokens before the user has
typed anything. A cloud model can shrug that off; a 7–14B local model
spends a meaningful fraction of its budget on tools it will never call
this turn, and the noise measurably degrades tool selection.

Progressive disclosure is the fix: show the model only the handful of
tools it almost always needs, and let it *retrieve* the rest by name or
intent when a task actually calls for one.

## Two tiers

Every tool declares a tier:

- **Core** — always serialized into the prompt, verbatim. The tools a
  coding turn reaches for constantly, where the round-trip cost of a
  search would dwarf the schema cost of just showing it. Kept small on
  purpose; adding a core tool is a deliberate budget decision.
- **Optional** — omitted from the prompt by default. The model sees only
  that a `tool_search` verb exists; it searches by name/intent, gets back
  a few matching specs, then calls one. The long tail lives here.

Plume's starting split (subject to change as tools land):

| Core (always visible)        | Optional (searchable)               |
| ---------------------------- | ----------------------------------- |
| file read                    | plugin tools                        |
| file search / grep           | MCP tools                           |
| patch validate / apply / revert | model-library download / import |
| memory read / write          | browser / computer-use tools        |
| verifier (run tests)         | optional GitHub / Hugging Face connectors |
| stop / cancel                |                                     |
| model / runtime diagnostics  |                                     |

## Search

`ToolCatalog::search(query, limit)` ranks **only the optional tools**
(core tools are already in the prompt — re-surfacing them in results
wastes the budget twice) and returns the top `limit` by score. Scoring is
deterministic, case-insensitive, and intentionally crude — a weighted
substring/token match over the fields a model actually phrases a search
in, highest-signal first:

1. exact name match,
2. name token / prefix match,
3. name substring,
4. summary token match,
5. parameter-name match.

No BM25, no embeddings, no index — the catalog is small and rebuilt from
live definitions each assembly, so a linear scan is fine and has no stale
state to invalidate. If retrieval ever needs to scale, this is the seam
to swap; the *interface* (query in, ranked specs out) would not change.

`visible_specs(query, limit)` is the one call the prompt assembler makes:
it returns **core ⧺ (search hits for the query, if any)** — the exact set
of tool schemas to serialize this turn.

## Hard rules

- **Unknown ≠ hidden.** A tool the catalog doesn't recognize is never
  silently dropped. (Relevant once a live registry feeds the catalog;
  the scaffold's catalog is exactly what you construct it with.)
- **Search visibility is permission, enforced elsewhere.** The catalog is
  a *presentation* concern: it decides what the model can *see*. It does
  **not** authorize execution. Whether a found tool may actually run is
  the approval/allowlist gate's call (`agent::approval`,
  `AgentConfig.commandAllowlist`/`fileAllowlist`). A later slice wires the
  rule that a session which cannot call a tool should not be able to
  search or describe it either — but that gating lives in the assembler
  that owns the session config, not in this pure catalog.
- **A search verb is itself a (core) tool.** The model only knows to
  search because `tool_search` is in the core set. The scaffold models
  the catalog of *target* tools; the verb that searches it is wired by
  the executing slice.
- **No execution, no MCP, no downloads here.** This slice is types +
  ranking + this doc. Consistent with the rest of the agent-loop
  foundation: build the safe substrate first, wire execution behind the
  trust gates later.

## Why this keeps local models effective

The serialized tool surface stays roughly constant — core tools plus at
most `limit` search hits — no matter how many optional tools (plugins,
MCP servers, connectors) the user has installed. A user with fifty MCP
tools and a user with zero pay almost the same prompt-token tax. The
model spends its context on the task and the few tools in front of it,
not on a catalog it has to mentally filter every turn. That is the whole
point: disclosure scales with *intent*, not with *inventory*.

## Status

- `src-tauri/src/agent/catalog.rs` — pure `ToolTier` / `ToolParam` /
  `ToolSpec` / `ToolCatalog` with `core()`, `optional()`, `search()`,
  `visible_specs()`. `allow(dead_code)` until the prompt assembler
  consumes it. Unit-tested (ranking, core-never-in-results,
  visible-set composition, limit, empty query).
- No frontend surface yet — the model-facing view is the serialized
  prompt, not a panel. A future "installed tools" UI would read the same
  catalog.
