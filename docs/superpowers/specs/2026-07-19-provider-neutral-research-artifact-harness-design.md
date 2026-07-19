# Provider-Neutral Research Artifact Harness

**Date:** 2026-07-19

**Status:** Approved for implementation planning

**Base:** `origin/main@9a76c744f14180f0a9ca9196460c34d5629244d8`

## Goal

Give every supported Plume model a lightweight, bounded action harness that can
turn user-selected evidence into a cited Markdown artifact. The harness belongs
to Plume, not to Qwen, Apple, Ollama, a search vendor, or a shell process.

This design establishes the shared runtime for later online research, DOCX,
slides, and other artifact skills. It does not claim those later capabilities
as shipped when the first slice lands.

## Product promise

The user asks Plume to create a research note, sees exactly which sources and
actions the run uses, can stop it, previews the result, and explicitly exports
the Markdown file. Plume verifies that every citation points to evidence the
run actually received.

The first complete proof uses Browser evidence that the user already captured
through Plume's shipped human-controlled Browser. It performs no non-model-
transport network I/O. Qwen generation still uses bounded loopback HTTP to the
already-running Plume-managed MLX server.
Later slices add bounded URL fetching and interchangeable search adapters
without changing the model/tool or artifact contracts.

## Design principles

1. Plume owns orchestration; providers translate model turns only.
2. Reuse the existing loop controller, approval core, tool catalog, and typed
   event stream. Do not build a parallel agent runtime.
3. Small local models receive a tiny disclosed tool set and bounded jobs.
4. Remote content and model output are untrusted data. Rust validates every
   tool call, source, citation, budget, and write.
5. Local-model inference receives the majority of RAM, compute, and thermal
   budget. The harness has zero idle subprocesses and bounded active buffers.
6. Sources, artifacts, and exports keep their exact session and trust scope.
7. A candidate or later slice never becomes a shipped claim through UI copy or
   documentation.

## Staged campaign

Each stage is a separate implementation plan, PR, exact-head review, and
packaged smoke gate. The immediate plan covers Stage A only.

### Stage A — cited note from attached evidence

The user explicitly attaches eligible Browser text evidence to the current
local or trusted-project chat, selects **Create → Research note**, enters the
question, and starts a bounded run.

The harness:

1. resolves every opaque evidence reference through its existing owning Rust
   resolver and session identity;
2. redacts and bounds the accepted source content;
3. summarizes each source in an isolated model turn;
4. synthesizes a Markdown draft from the bounded summaries;
5. deterministically verifies every citation against the accepted source ids;
6. requests at most two citation-repair revisions;
7. stages the draft and evidence manifest under the owning session scope;
8. renders an inert preview; and
9. exports only after the user completes a native Save dialog.

This stage proves real multi-turn tool use, context packing, events, Stop,
artifact staging, citation verification, and explicit export with no search
provider, daemon, API key, or network permission.

### Stage B — bounded URL fetch

Add one `web.fetch` tool after a separate network-authority safety review. The
tool accepts an exact HTTP(S) URL, requires visible per-run domain approval,
uses a dedicated no-cookie client, refuses private/loopback/link-local targets,
re-checks policy after DNS and every redirect, accepts only reviewed text
content types, and records the bounded redacted extract in the same evidence
bundle.

Stage B does not grant agent Browser authority. It never inherits Browser
cookies, human localhost approval, WebKit state, downloads, or popup behavior.

### Stage C — provider-neutral web search

Add `web.search` behind a small Rust `SearchProvider` interface. Optional
adapters may target a user-configured SearXNG-compatible endpoint, Tavily,
Brave, or another separately reviewed search service. No backend is bundled or
required, and none becomes the product.

Search result titles, snippets, URLs, and suggested follow-ups are untrusted.
They are bounded, sanitized, and sent through the same domain gate as direct
fetches. A configured adapter adds no idle process and is dropped after the
run.

When no search adapter is configured, Stage A remains the honest usable
fallback: attach Browser sources and create the same artifact.

## Architecture

### One harness core

Add a production step adapter over `agent::controller::run_loop`. The adapter
owns run identity, budgets, cancellation, context packing, model turns, tool
validation, tool execution, evidence accumulation, artifact staging, and typed
events.

The pure controller remains the single budget/abort/terminal-state driver.
`agent::approval` remains the place that decides whether an action may proceed
without asking. `agent::catalog` remains the disclosure source. The research
workflow registers only the tools allowed for its current stage.

The first stage exposes no shell, patch, file-write, Browser-action, MCP,
plugin, computer-use, or arbitrary `tools.invoke` capability.

### Strict provider-neutral tool protocol

Introduce one internal tool-call shape:

```text
ToolCall {
  call_id,
  namespaced_tool_id,
  arguments
}
```

Only one tool call is accepted per model turn in Stage A. Rust parses the
provider output strictly against the disclosed schema. Unknown tool ids,
unknown fields, malformed JSON/text framing, duplicated terminal records, and
ambiguous mixed prose/tool output fail closed. Plume never guesses what the
model intended to execute.

Stage A discloses exactly two namespaced calls:

- `research.summary.submit { sourceId, summary }` submits the bounded summary
  for the one source Rust placed into that turn; and
- `artifact.markdown.submit { markdown }` submits a synthesis or repair draft
  for deterministic validation and staging.

Neither call grants arbitrary storage authority. Rust verifies the current
phase, exact source id, summary/draft caps, run identity, and citation contract
before accepting it. Stage B adds `web.fetch`; Stage C adds `web.search`.

Provider adapters translate between this internal shape and the model:

- Qwen/MLX uses concise ChatML-compatible text framing plus the reviewed stop
  sequence.
- Apple uses a concise instructions-channel text framing in the first stage.
  Native Foundation Models `Tool`/guided-generation support is a later adapter
  upgrade, not a different harness.
- Ollama/OpenAI-compatible models may reuse the text adapter when their exact
  model capability has not been independently verified as native tool-calling.

Adapters translate messages, output, token counts, and context capacity. They
never choose permissions, tools, budgets, sources, or writes.

### Provider-aware context packing

The harness asks the adapter for its usable context size and token-count
capability. Apple's helper exposes runtime `contextSize` where the linked SDK
and host make it available, and exact token counts where the API is available,
so future OS/model increases can be used without changing product code. The
macOS 26.0 deployment path must treat conservative token estimation as an
expected capability level, not an exceptional failure. If any adapter cannot
report an exact size or count, the harness uses a conservative documented
fallback rather than trusting model metadata it cannot verify.

Every turn reserves space for instructions, the disclosed tool schema, the
expected response, and safety framing. Source content receives only the
remaining budget.

Stage A uses map/reduce:

- one fresh bounded session per source summary;
- one synthesis session over summaries, never raw full pages;
- at most one context-overflow repack/retry per turn;
- no silent dropping of a source; truncated or omitted content is visible in
  its source record.

This keeps Apple's current 4,096-token on-device session useful for narrow
summaries while allowing verified larger Qwen budgets. A future larger Apple
context window is adopted dynamically.

### Evidence and artifact bundle

The backend persists an immutable, versioned bundle owned by the exact session
identity. Local-chat bundles stay in app-private storage. Trusted-project
bundles stay in the project-private session domain; they are never aggregated
into user memory or another project.

Each bundle records:

- opaque artifact id and version;
- owning session scope/id and project identity when applicable;
- user request;
- provider, model, runtime identity, and implementation version;
- accepted source order;
- per-source id, origin kind, sanitized URL, title, capture/retrieval time,
  bounded extract hash, byte count, redaction count, and truncation flag;
- per-source bounded summary and its model-turn provenance;
- Markdown draft versions;
- paragraph-to-source citation map;
- citation-verification status and diagnostics;
- searches, fetches, model turns, revisions, bytes, tokens, duration, and
  terminal outcome;
- export history containing destination display name and time, never a secret
  path copied into prompts.

Memory links/backlinks never select bundle sources. User memory is never added
implicitly. Project memory/topics remain governed by their existing trusted
prompt path and are not research sources unless a later separately reviewed
source kind explicitly adds them.

### Citation verification

The model cites stable source ids minted by Rust. A pure verifier parses the
draft and rejects:

- unknown source ids;
- citations to sources not accepted for this run;
- malformed citation syntax;
- missing source references for factual paragraphs when the workflow contract
  requires them; and
- source ids whose persisted extract hash no longer matches the bundle.

The internal citation syntax is `[[S1]]`, using ids `S1` through `S10` in the
accepted-source order. Every non-empty prose paragraph and list item outside a
Rust-owned **Sources** section must contain at least one valid inline source
id. Headings and fenced code blocks are exempt. The model never writes the
Sources section: Rust renders it from the immutable source records and converts
inline ids to ordinary Markdown footnotes during preview/export. This makes
missing and fabricated citations mechanically checkable without pretending to
judge factual truth.

The verifier checks provenance, not factual truth. The UI labels this honestly
as **Citations verified** rather than **Facts verified**.

After a failed check, the harness supplies only the draft, concise diagnostics,
and allowed source ids for a repair turn. After two failed repairs, the bundle
is staged as **Draft — citations need review**. The user may inspect or export
it, but Plume never presents it as citation-verified.

For small local models, this review-needed result is an expected ordinary
terminal outcome rather than an exceptional crash state. Product copy and
tests must make the next action clear without implying that provenance checks
also established relevance or factual truth.

## User experience

Research uses the normal composer rather than a separate technical dashboard.

- A compact **Create** action opens **Research note**.
- Eligible Browser sources remain visible in the existing context shelf.
- The start surface summarizes source count, output type, selected model, and
  the current hard limits in ordinary language.
- During a run, calm collapsible rows show summarizing, writing, checking
  citations, revising, paused, failed, stopped, or complete.
- **Stop** remains visible for the entire active run.
- The completed artifact card exposes **Preview**, **Sources**, and **Export
  Markdown**.
- Raw typed events, token/byte budgets, hashes, redaction counts, provider
  diagnostics, and exact manifests live under **Details**.

The preview renderer is inert. It never fetches remote images, generates link
previews, executes HTML, loads scripts, or follows links automatically. Remote
images render as labelled blocked placeholders. Opening a link is a separate
explicit human action through the existing Browser boundary.

## Export boundary

Staging is not export. Artifact bytes remain under the owning Plume/session
store until the user chooses **Export Markdown** and completes a native macOS
Save dialog.

Use `NSSavePanel` through the already-present `objc2-app-kit` dependency by
enabling only its required feature set. The panel is created and operated on
the macOS main thread; the async IPC handler waits through a bounded typed
bridge rather than touching AppKit from a worker. Do not add an always-resident
dialog runtime or a frontend path write. Rust writes only the exact selected
file, uses atomic replacement semantics, refuses symlink/hardlink surprises,
and returns a typed cancelled/saved/failed outcome.

Cancelling the dialog changes nothing. An export failure leaves the staged
artifact intact and visible. No half-written destination is reported as
successful.

## Network authority for later stages

Before Stage B code, `docs/SAFETY.md` and `docs/IPC_CONTRACT.md` must define a
third authority axis independent from file and command allowlists:

- network disabled by default;
- explicit per-run domain scope;
- exact search-provider hosts distinguished from result hosts;
- unknown domains prompt or fail closed;
- loopback, RFC-1918, link-local, non-HTTP(S), embedded credentials, and
  disallowed ports refused;
- DNS rebinding and every redirect hop re-checked;
- no cookie jar, credential store, Browser profile, proxy inheritance, or
  ambient authorization;
- content type, transfer bytes, retained bytes, redirects, deadlines, and
  concurrency bounded;
- snippets and page content sanitized, redacted, delimiter-escaped, and
  labelled as untrusted source data;
- network approval, policy decisions, and failures recorded as typed events.

Remote content may suggest another URL only as inert data. That URL must enter
the same domain gate as a new proposal.

## Budgets and lightweight operation

All ceilings are enforced in Rust and reported through typed events. Reaching
a cap stops or pauses honestly; it never silently expands the budget.

### Stage A defaults

- maximum accepted sources: 10;
- retained source extract: 64 KiB each;
- total retained evidence: 4 MiB;
- maximum logical workflow turns: 13 (10 summaries, one synthesis, two
  citation repairs);
- maximum recovery calls: 13 total and one per logical turn, used for either a
  malformed-framing re-ask or a context-overflow repack, never both;
- absolute maximum provider calls: 26;
- citation-repair revisions: 2;
- context-overflow retries: 1 per turn;
- one active model turn at a time;
- Markdown artifact: 256 KiB;
- run deadline: 5 minutes;
- no subprocess, renderer, web client, or timer remains after terminal state.

### Stage B/C additional ceilings

- searches: 5;
- fetched pages: 10;
- concurrent fetches: 2;
- redirects per fetch: 5;
- transfer cap: 2 MiB per response;
- retained extract: 64 KiB per source;
- one retry per failed tool call when policy allows it.

The implementation must measure and record idle RSS delta, active peak RSS,
duration, retained bytes, and model-turn counts. Acceptance requires zero new
idle child processes and no eager renderer/search service. Buffers, fetched
pages, adapters, and preview state are dropped when no longer needed.

Bundled SearXNG, Python/Node research services, headless browsers, and
always-on artifact workers are prohibited in this campaign.

## State and failure behavior

- A run is bound to one exact session generation. Switching sessions,
  projects, or source ownership cancels it; late events cannot repaint the new
  identity.
- A malformed or unsupported tool call fails the turn and executes nothing.
- A malformed framing response may receive one bounded re-ask containing only
  the parse diagnostic and required schema. The re-ask consumes that logical
  turn's sole recovery allowance; a second malformed response fails closed.
- Unknown or over-budget sources fail before a model call.
- Provider transport failure pauses with a typed resumable reason only when a
  safe resume point exists; otherwise the run fails closed.
- Context overflow triggers one smaller repack. A second overflow fails with a
  provider-specific visible explanation.
- Cancellation is checked before and after every model/tool/store boundary.
- Partially generated draft text may be shown as diagnostic output but is not
  staged as an artifact unless the terminal policy explicitly marks it as a
  review-needed draft.
- Store capacity failure preserves earlier bundles and writes no partial
  record.
- Preview never mutates or performs network I/O.
- Export cancellation is not an error; export failure never deletes the
  staged bundle.

## Accessibility

- **Create**, **Research note**, **Stop**, **Preview**, **Sources**, **Export
  Markdown**, and Details have stable accessible names.
- The Create menu keeps the existing keyboard menu contract: arrow keys,
  Home/End, Escape, outside dismissal, and focus restoration.
- Run status uses visible text plus polite live-region updates without
  announcing every token.
- Collapsed event rows remain keyboard expandable and expose their terminal
  status in the accessible name.
- Citation diagnostics identify the affected paragraph and source id without
  relying on color.
- Native Save dialog cancellation returns focus to **Export Markdown**.

## Ownership boundaries

Likely Stage A ownership:

- `src-tauri/src/agent/harness.rs`: production adapter over the pure loop;
- `src-tauri/src/agent/protocol.rs`: strict internal tool-call protocol and
  provider framing boundaries;
- `src-tauri/src/research/bundle.rs`: bounded immutable bundle store;
- `src-tauri/src/research/citations.rs`: pure citation verifier;
- `src-tauri/src/research/context.rs`: provider-aware packing and map/reduce
  inputs;
- `src-tauri/src/commands/research.rs`: thin IPC validation and identity gate;
- `src-tauri/apple-model/`: context-size/token-count capability reporting and
  Apple framing adapter;
- `src/features/research/`: run progress, inert preview, sources, and export;
- `src/lib/api/research.ts`: typed IPC wrappers.

Stage B owns `src-tauri/src/research/fetch.rs` and the network policy helpers.
Stage C owns `src-tauri/src/research/search.rs` plus separately reviewed
adapters. Domain maps and decomposition docs must be updated if exact files
differ.

The frontend carries opaque run, source, and artifact ids. It never receives
authority to read arbitrary files, fetch URLs, select an export path, or
declare citations valid.

## Testing strategy

Start every behavior change with a failing test.

### Pure/domain tests

- strict parse/serialize corpus for Qwen and Apple framing;
- unknown, duplicate, malformed, mixed-prose, oversized, and injection-shaped
  tool replies execute nothing;
- controller stops at every budget, abort, pause, failure, and completion;
- provider packer reserves output/schema space, respects reported context,
  repacks once, and never silently drops a source;
- map/reduce source ordering and provenance are exact;
- citation verifier accepts only recorded ids and catches fabricated,
  malformed, missing, and stale-hash citations;
- bundle caps, atomic writes, symlink/hardlink refusal, corruption recovery,
  session ownership, and local/project separation;
- cancellation and stale-generation fences at every async boundary;
- inert Markdown rendering rejects/blocks remote media and active HTML;
- native export cancellation, overwrite, atomic failure, focus return, and
  exact bytes.

### Integration tests

- fake Qwen/Apple adapters complete the same source-summary/synthesis/repair
  workflow;
- exact events stay monotonic and expose caps, sources, citation status,
  telemetry, Stop, and terminal outcome;
- provider failure, second context overflow, citation exhaustion, store
  capacity, and export failure preserve honest recoverable state;
- a local run cannot read project evidence and a project switch cannot reuse a
  prior project's source or artifact id;
- Stage A performs zero non-model-transport network calls and starts zero
  non-model child processes. The existing Qwen loopback transport and bounded
  Apple generation helper remain provider transports; every Apple helper is
  reaped before the provider call returns.

### Packaged verification

At the exact implementation head:

- run one cited note with Qwen and one with Apple;
- exercise Stop, context packing, a citation repair, review-needed fallback,
  inert preview, source inspection, Save cancellation, successful export, and
  quit/relaunch restoration;
- confirm no Browser authority or hidden network call appears;
- record idle RSS before/after feature availability and active peak RSS;
- run focused suites, full frontend, Rust/Swift tests, TypeScript,
  `PLUME_FULL_VERIFY=1 ./scripts/verify.sh`, pre-commit/gitleaks, GitHub CI,
  and findings-only exact-head review.

## Non-goals

- Agent-driven Browser navigation, clicks, capture, or localhost approval.
- Bundled SearXNG, headless Chromium, Python/Node research services, or any
  always-on helper.
- Unrestricted shell, generic `tools.invoke`, MCP/plugin execution, or command
  approval changes.
- Automatic semantic retrieval, background research, dreaming, scheduled
  tasks, or links/backlinks gaining prompt authority.
- User memory entering research implicitly or project evidence crossing scope.
- DOCX, PPTX, PDF, images, remote media, or deterministic layout renderers in
  Stage A.
- Any search adapter, URL fetch, or network permission in Stage A.
- Apple Private Cloud Compute, automatic cloud fallback, or a claim that every
  Apple host supports the model.
- Computer-use emission or macOS host control.

## Candidate follow-ups

After Stages A–C are independently green:

- deterministic DOCX renderer over the verified evidence bundle;
- deterministic SlideSpec/PPTX renderer with overflow, contrast, density,
  alignment, and provenance checks;
- native Apple Foundation Models tool/guided-generation adapter;
- additional search adapters;
- artifact version comparison and explicit reuse across sessions;
- PDF source extraction; and
- semantic retrieval over user-approved artifact bundles.

These remain candidate-only until separately designed and commissioned.

## Completion criteria

Stage A is complete when Qwen and Apple can both use the same Plume-owned
bounded harness to transform explicitly attached Browser evidence into an
honestly citation-checked Markdown artifact, stage it under the correct
session scope, preview it without network activity, and export it only through
an explicit native dialog. The implementation adds no idle child process,
preserves every existing trust/memory/Browser boundary, passes exact-head
packaged and automated verification, and leaves fetch/search/DOCX/slides
clearly unshipped.
