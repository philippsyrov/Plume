# Plume Roadmap And Agent Navigation Design

## Purpose

Plume has strong implementation and research records, but no single reliable
answer to three basic questions:

1. What is shipped today?
2. What is partially wired or only scaffolded?
3. What should be built next, and which research source motivated it?

The current root `README.md` status stops near the early D-series work and
`AGENTS.md` carries a long chronological slice ledger. In addition,
`docs/HERMES_AGENT_RESEARCH.md` proposed candidate D64-D70 slices before those
numbers were later assigned to different shipped work. This makes good
evidence hard for both humans and agents to navigate.

This design creates a linked documentation spine. It does not change product
behavior or discard existing research. It was refreshed after the D131/D132
benchmark surfaces, the session fork/rewind work, project skills, exact prompt
context manifests, memory-topic links, and their post-merge integrity fixes had
all reached `main` at `5bcbf93`. Older campaign ordering in this document is
therefore evidence, not current priority.

## Decisions

### Sequence From Current Evidence

D131 model catalog presets and D132's in-app benchmark results viewer are
shipped. The full measured matrix on the 128 GB M5 Max and the D130
evidence-backed launch rewrite remain evidence-gated: neither should block
software work that does not depend on uncollected hardware records, and neither
may publish performance claims before those records exist.

The active product sequence after the documentation spine is:

1. **Knowledge workspace / Second Brain surface.** Turn the shipped memory
   entries, text search, curated topic files, topic links, exact context
   manifests, distillation audit, and session-to-skill flow into one visible
   workspace with topic navigation and backlinks.
2. **Explicit context placement.** Let the user choose and later drag visible
   files, memories, topics, and other sources onto a chat or agent target. Every
   source keeps provenance and is re-read through its owning backend boundary;
   no link silently gains prompt-selection authority.
3. **Browser Phase A.** Add a first-class sandboxed browser workspace for
   visible navigation, localhost testing, screenshots/observations, and explicit
   evidence attachment. Remote page content receives no Plume IPC authority.
4. **Deeper guarded agent execution.** Complete the bounded read/edit/test/fix
   loop, approvals, queues, status, and tool execution without bypassing the
   patch, command, project-trust, or computer-use gates.
5. **Computer-use emission.** Build guarded `computer.*` actions first against
   the Phase A sandbox; keep macOS host control as later, separate, per-session
   opt-in work.

The benchmark matrix runs when the target hardware exists. D130 follows that
evidence. These are parallel evidence gates, not speculative reasons to freeze
the product roadmap.

### Separate Four Kinds Of Truth

The documentation spine separates:

- **Product truth:** what Plume is and where it is going.
- **Implementation truth:** what code and UI are reachable today.
- **Roadmap truth:** ordered work, dependencies, and blockers.
- **Research evidence:** external or local references that informed a Plume
  decision.

No one document should try to serve all four roles.

### Preserve Evidence, Replace Chronology As Navigation

Existing research and slice history remain available. The new navigation
layer points to them and records whether their recommendations are shipped,
superseded, or still useful. Chronological history is evidence, not the
default entry point.

## Documentation Architecture

### Root Entry Points

`README.md` becomes a short product and contributor entrance:

- literal product identity and current capability summary;
- links to `docs/README.md`, `AGENTS.md`, quick start, and current roadmap;
- no chronological slice diary;
- performance claims only when D130 can cite generated benchmark evidence.

`AGENTS.md` remains the workflow contract:

- stack and product boundaries;
- source-of-truth links;
- safety and verification rules;
- current high-level status;
- no multi-thousand-line historical ledger.

The removed ledger is preserved under `docs/history/`, linked from the status
inventory and available when provenance is needed.

### Documentation Map

Add `docs/README.md` as the canonical map, organized by task:

- understand the product;
- inspect current capabilities;
- choose the next slice;
- work on frontend, Rust backend, memory, agent runtime, sessions, patching,
  providers, benchmarks, safety, or testing;
- inspect research evidence;
- inspect historical decisions.

The map points to current documents first. Historical material must be
explicitly labelled and never appear as current implementation guidance.

### Roadmap

Add `docs/ROADMAP.md` as the ordered dependency map. It contains tracks, not
an unbounded feature wish list:

1. Documentation and agent navigation.
2. Project knowledge and Second Brain.
3. Explicit context placement and linked work objects.
4. Sandboxed browser workspace and evidence capture.
5. Safe coding-agent execution.
6. Skills, tools, plugins, and external agents.
7. Operability, safety, observability, and computer use.
8. Local model ownership, benchmark evidence, and launch readiness.

Each track states its outcome, current floor, dependencies, next deliverable,
and explicit non-goals. Slice numbers are assigned only when work is
commissioned; research notes must not reserve speculative numbers.

### Feature Inventory

Add `docs/FEATURE_INVENTORY.md` as the implementation-status ledger. Every
entry uses one canonical status:

- `shipped`: reachable production behavior with automated evidence;
- `partial`: useful end-to-end behavior exists, but a named capability is
  missing;
- `scaffold`: types, pure logic, or UI shell exists without a production
  execution path;
- `researched`: behavior and adaptation are documented, with no shipped
  implementation;
- `blocked`: accepted work cannot proceed until a named dependency changes;
- `retired`: superseded behavior retained only for history.

Hardware verification is an evidence field, not a status. A shipped path can
be marked `hardware: pending` without pretending it was smoke-tested.

Each feature record includes:

```text
id
track
status
current behavior
missing behavior
frontend reachability
backend reachability
automated evidence
manual or hardware evidence
dependencies
implementation paths
source documents
next commissioned slice, if any
last verified commit and date
```

The inventory is the only place allowed to make a repository-wide status
claim. Domain documents may explain behavior but link back to the inventory
for status.

### Research Registry

Add `docs/research/README.md` and one note per research family:

- Sass memory and distillation;
- Codex desktop and public app-server workflows;
- Claude Code public behavior and source-hygiene lessons;
- Hermes Agent;
- Qoder and Repo Wiki / Knowledge Hub;
- Notion linked agent workflows;
- ZCode interaction patterns as local/public product observation;
- Rust coding-agent references;
- local inference engines and model research.

Each registry entry records:

```text
source and upstream URL
source type
date and revision inspected
license or source-hygiene classification
observed behavior
Plume adaptation
already implemented overlap
remaining gap
rejected ideas and reason
refresh trigger
```

Research sources use these hygiene levels:

1. `official-public`: official docs or public upstream source.
2. `local-observation`: black-box behavior observed in an installed product.
3. `clean-room-reference`: public implementation claiming independent or
   specification-based construction; license still governs reuse.
4. `behavior-report-only`: third-party description suitable only for leads.
5. `do-not-use-source`: leaked, proprietary, ambiguously derived, or
   unlicensed material. Record behavior-level lessons only; never vendor,
   translate, or reproduce implementation text.

Public GitHub availability is not permission to copy. For example, Claurst
is GPL-3.0 and claims a clean-room split between specification and Rust
implementation. It is useful for architecture comparison, but copying code
could impose license obligations on Plume. Claude-Code-derived repositories
with unclear provenance remain behavior references until a dedicated license
and provenance audit says otherwise.

### Domain READMEs

Add concise README files at stable ownership boundaries, not mechanically in
every folder. Initial domains:

- `src/features/agent/`
- `src/features/benchmarks/`
- `src/features/memory/`
- `src/features/project-shell/`
- `src/features/sessions/`
- `src-tauri/src/agent/`
- `src-tauri/src/memory/`
- `src-tauri/src/providers/`
- `src-tauri/src/sessions/`
- `scripts/benchmark/`
- `benchmarks/`

Each domain README answers only:

1. What does this domain own?
2. What are its public entry points?
3. What safety or data boundary must not be bypassed?
4. Where are tests and deeper contracts?
5. Which neighboring domain owns the next step?

READMEs must not duplicate detailed API contracts or carry a separate
roadmap.

### History And Archive

Add `docs/history/README.md` and `docs/archive/README.md`:

- `history/` preserves chronological decisions and landed-slice records.
- `archive/` preserves superseded design guidance.
- every archived document carries a replacement link or an explicit statement
  that no replacement exists;
- current docs never silently link to archive material as active guidance.

## Research Adaptation Ledger

The first inventory pass must explicitly distinguish the following.

### Already Shipped Or Partially Shipped

- persisted local and project sessions with FTS search;
- continue-in-new-chat, prefix fork, and rewind-to-new-chat with persisted
  parent/child provenance;
- visible JSONL project memory with CRUD, prompt injection, and text search;
- curated `INDEX.md`, `USER.md`, `SOUL.md`, and topic-file prompt context;
- user-managed memory-to-topic links as organization metadata only;
- exact prompt-context manifests listing the memory entries and topic files that
  actually reached the bounded prompt;
- exact-duplicate memory compaction with confirmation, deterministic link
  inheritance, stale-preview protection, and a visible audit log;
- project skill library plus session-to-skill draft promotion with source
  snapshot and trust checks;
- typed agent events, approval/config foundations, and patch-only single-step
  execution;
- Plume-managed MLX runtime, benchmark catalog/presets, and read-only results
  viewer;
- safe diff validate/apply/revert and checkpoints.

### Scaffolded But Not Fully Executing

- bounded loop controller;
- persistent command approval ledger consumers;
- progressive tool catalog beyond read-only discovery;
- complete read/edit/test/fix loop;
- broad external tool and MCP execution;
- multi-agent coordination;
- disabled Browser and Terminal workspace entries;
- optional `browser_open`, `browser_click`, and `computer_screenshot` catalog
  descriptions with no executor behind them.

### Researched But Not Shipped

- local semantic/vector memory;
- scheduled or background dreaming/distillation;
- LLM-assisted memory clustering and summaries;
- stale-fact contradiction detection and pruning;
- distilled project/session profiles;
- repository wiki generation and knowledge cards;
- backlinks and knowledge graph navigation;
- explicit memory/topic context selection and retrieval preview;
- linked `signal -> task -> spec -> run -> review -> artifact` objects;
- drag-and-drop context bundles and agent handoff;
- automatic skill creation and skill improvement;
- sandboxed browser execution and evidence capture;
- full computer-use execution, including opt-in macOS host control.

This ledger prevents “documented” from being interpreted as “implemented.”

## Source-Specific Decisions

### Codex Desktop And ZCode

Use official Codex documentation/public interfaces for capability claims and
direct product observation for interaction details. ZCode is a useful
behavior-level comparison because its visible task workspace closely follows
the Codex desktop pattern; it is not a source tree or architecture template.
Adapt selectively:

- one unified workspace instead of duplicate commands and parallel shells;
- explicit Goal, progress, diff, status, and background-task surfaces;
- Browser, Terminal, Files, Review/diff, side chat, scheduled work, and visible
  subagent activity as separately statused product candidates;
- follow-up tasks and context placement that preserve source provenance.

Do not copy branding, product text, proprietary assets, or Electron-specific
implementation. Plume remains Tauri/Rust and local-first.

### Qoder

Study the installed product for interaction flow and official documentation
for capability claims. Adapt:

- dedicated task workspace;
- knowledge hub combining Repo Wiki, Knowledge Cards, and conversation
  memory;
- explicit execution environment selection;
- task summary that links knowledge, tools, changes, and artifacts;
- safe rollback when an earlier instruction is edited.

Do not treat the Electron application bundle as reusable implementation.

### Hermes

Use the official public repository and docs, pinned to a revision for each
refresh. Adapt selectively:

- memory provider lifecycle;
- prompt-size accounting and stable/context/volatile tiers;
- FTS session recall and summaries;
- skills as procedural memory;
- progressive tool disclosure;
- typed events, background processes, scheduling, and subagent isolation.

Do not clone Hermes' platform breadth before Plume's local coding loop is
complete.

### Notion

Adapt the visible placement model, not hidden ambient authority: pages,
excerpts, files, memories, topics, and later screenshots can be placed onto a
chat or agent target. The resulting context shelf names every source, scope,
and provenance record before send. Removing an item removes it from the next
send. Nothing becomes sticky or automatically trusted because it was linked.

### Sass

Reuse concepts from the local Sass project, not Discord-specific behavior:

- semantic memory retrieval;
- dedupe, age pruning, and hard caps;
- scheduled distillation;
- compact generated profiles;
- recent-response tracking to reduce repetition.

Plume replacements are project facts, task outcomes, verifier results,
session profiles, and repeated-patch avoidance.

### Claude Code And Rust Reimplementations

Maintain the existing clean-room boundary. Public behavior can inform tool
contracts, compaction, permissions, hooks, background work, diagnostics,
skills, and orchestration. Leaked implementation text cannot be copied or
translated.

Evaluate Rust references in a separate comparison table covering license,
provenance claim, architecture, safety model, recovery, memory, tools,
provider support, test quality, and reusable-vs-observe-only judgment.

## First Product Track After The Spine

### Knowledge Workspace

The first product track is graph-first, not embedding-first. Add a dedicated
`Knowledge` workspace view backed initially by the shipped `memory.index` and
`memory.topics` reads:

- topic navigation shows the capped, validated Markdown already returned by
  the backend;
- each topic shows backlinks derived from memory entries whose `links` contain
  that exact topic ref;
- each memory shows its created time, redaction count, topic links, and stable
  entry id as provenance;
- lexical search covers visible memories first and never presents itself as
  semantic retrieval;
- unlinked memories remain visible rather than disappearing from the graph;
- settings retain the mutation controls (remember, edit, forget, link,
  distill), while the workspace is the calmer browse/search surface.

The first slice can derive backlinks in the frontend because both source sets
already arrive from trusted, bounded backend reads. A later backend projection
is justified only if scale, pagination, or cross-source search makes that
derivation expensive.

### Explicit Context Shelf

The next slice introduces one typed context-source contract used by buttons and
drag/drop alike. A shelf item is a reference, never frontend-supplied prompt
text. The backend resolves the reference at send time through the owning store,
re-applies trust, path, size, redaction, and existence checks, and reports the
exact accepted sources in the prompt-context manifest. Initial source kinds are
project file/selection, memory entry, and curated topic file.

The shelf is visible before send and copied onto the persisted user turn after
acceptance. Stale or rejected sources remain visible with a typed reason and do
not silently fall out. Local chats cannot carry project-scoped sources. Memory
topic links remain organization metadata and do not populate the shelf by
themselves.

### Retrieval Staging

Retrieval grows in audited stages:

1. lexical search and manual `Use in chat`;
2. query-time retrieval preview with the candidate reason and byte cost;
3. user-confirmed insertion into the same context shelf;
4. measured local semantic retrieval after an evaluation set exists;
5. opt-in automatic retrieval only if the preview/manifests prove it is honest.

Distillation remains manual and auditable. Dreaming/background consolidation is
later opt-in work with resource budgets, traces, and undo; this design does not
grant it authority.

### State And Failure Rules

The Knowledge view loads memory entries and curated topics as two named source
states. If one read fails, the other remains visible and the failed region shows
its typed error plus Retry; a topic-read failure must not make remembered facts
disappear. Project switch, trust revocation, or view unmount invalidates every
in-flight response so stale data cannot repaint the next project.

Backlinks use the exact canonical topic ref as their key. Missing or newly
removed topic refs remain visible on the memory as stale organization metadata
until the user repairs them; they never resolve to an arbitrary file. Duplicate
context refs collapse by `(kind, stable id/path, line range)` while preserving
the user's first insertion order.

The context shelf belongs to one persisted project session. It never crosses to
a local chat or another project, and switching sessions renders only that
session's shelf. Preview reports every requested source independently. Send is
all-or-nothing for requested sources: if any shelf item became stale, blocked,
oversize, or untrusted, the backend rejects before starting a model stream and
the UI keeps the full shelf with the failing item marked. This prevents a
partially accepted prompt from looking complete.

### Product Verification

The Knowledge/context track requires:

- pure projection tests for topics, backlinks, unlinked memories, stable
  ordering, and duplicate-source collapse;
- component tests for partial load failures, retry, empty states, keyboard
  navigation, project/session switching, and stale async response suppression;
- backend tests for trust, local-versus-project scope, id/path validation,
  symlink and hardlink posture, byte caps, redaction, stale refs, and atomic
  all-or-nothing context acceptance;
- integration tests proving preview and send produce the same ordered manifest,
  and proving memory links alone never affect source selection;
- packaged UI smoke for Knowledge navigation, `Use in chat`, shelf removal,
  drag/drop, visible blocked-source recovery, and persisted-turn provenance.

### Browser And Computer Use Follow-Up

Browser is the next major product track, not a buried incidental note. Phase A
uses a dedicated remote-content webview with zero Plume IPC capability. Before
embedding it, Plume must narrow the current main-window capability to the
bundled application webview so a child webview cannot inherit event or command
authority. The visible Browser workspace owns navigation state, localhost
allowlists, screenshots/observations, and explicit evidence attachment.

Only after this user-driven surface is proven may the local agent emit bounded
browser actions. Those actions run inside a named session with foreground
approval, target allowlist, append-only visible trace, Pause/Stop, and no
persistent blanket permission. Phase B macOS host control is a separate later
capability. Plume being operable by an external computer-use agent remains a
different, already-shipped receiving role.

## Verification And Freshness

Add `scripts/check-markdown-links.ts`, run through the repository's existing
vite-node toolchain, and call it from `scripts/verify.sh` over tracked Markdown
files. Plume should not introduce Python solely for documentation checks. When
Node dependencies are unavailable, `verify.sh` reports a clear `[WARN]` and
skips this check, matching its existing pre-bootstrap frontend posture. Once
vite-node is available, broken links are a hard failure. The checker must:

- ignore external URLs;
- validate anchors and relative local targets where practical;
- reject links escaping the repository root;
- ignore generated, dependency, and benchmark-artifact directories;
- report the source file and broken target.

Documentation verification also checks:

- required entry documents exist;
- inventory statuses belong to the canonical vocabulary;
- each research entry has a source date and hygiene classification;
- archive documents identify their replacement state;
- no feature inventory row claims `shipped` without an evidence link.

Add a soft inventory-freshness check for rows in `shipped`, `partial`, or
`scaffold` state. Each such row names one or more repository-relative
`implementationPaths`. The checker verifies that `lastVerifiedCommit` exists
and is an ancestor of `HEAD`, then asks Git which named paths changed between
that commit and `HEAD`. A changed owned path without a refreshed row emits a
`[WARN]` naming the feature id and paths. R1 keeps this warn-only while the
inventory is seeded; a later rollout may make it a failure once false-positive
rates are understood. Research-only and blocked rows do not need implementation
paths. If `lastVerifiedCommit` is missing or is not an ancestor of `HEAD` after
history was rewritten, the checker emits the same re-verification warning and
does not attempt a path diff across unrelated history.

Status freshness is maintained at merge time: any feature PR that changes a
capability must update its inventory row in the same PR. Research refreshes
update the inspected revision/date but do not change implementation status
without direct repository evidence.

## Rollout

### R1: Navigation Spine

- add `docs/README.md`, `ROADMAP.md`, `FEATURE_INVENTORY.md`, and research /
  history indexes;
- add the Markdown-link checker;
- link existing documents without moving them;
- seed the inventory from current code and tests;
- record D131/D132 as shipped, keep D130 evidence-gated, and commission the
  Knowledge workspace as the next product track;
- make Browser Phase A and its capability-isolation prerequisite first-class
  roadmap entries rather than incidental computer-use notes.

### R2: Domain Orientation

- add the initial domain READMEs;
- link each to contracts and tests;
- add a lightweight README-coverage check for the named ownership domains.

### R3: Current-Versus-History Cleanup

- move the slice ledger out of `AGENTS.md` without losing it, split into
  era files under `docs/history/slices/` (for example D000-D049, D050-D099,
  and D100-D149) so no relocated ledger creates a permanent 1,500-line doc
  warning; do not exempt history from the existing soft cap;
- replace the stale root README status with inventory-backed current status;
- archive superseded candidate-slice sections;
- repair links and run strict documentation verification.

### R4: Ongoing Research Refresh

- refresh Codex, ZCode, Qoder, Notion, Hermes, and Rust-agent comparisons
  against pinned upstream revisions or dated local observations;
- record new ideas through the registry before adding roadmap work;
- reject duplicates by linking new observations to existing feature ids.

Each rollout is its own PR and stops for exact-head review. No rollout changes
product behavior.

## Non-Goals

- implementing product behavior inside the documentation migration PRs;
- claiming semantic retrieval, dreaming, Browser execution, host control,
  multi-agent execution, or agent authority before those paths ship;
- making the hardware-gated D130 launch rewrite without measured records;
- copying external source code;
- importing leaked Claude Code material;
- moving every existing document in one PR;
- creating README files in folders without a stable ownership boundary;
- turning research freshness into an automatic internet crawler.

## Acceptance Criteria

1. A new agent can find current product, implementation, roadmap, research,
   safety, and historical truth from `README.md` in two links or fewer.
2. Every major feature has one canonical status and evidence trail.
3. “Documented,” “scaffolded,” and “shipped” cannot be confused.
4. The roadmap names Knowledge workspace first, Browser Phase A next, and
   deeper guarded agent execution after them, while keeping hardware evidence
   work parallel and honest.
5. External research has explicit provenance and reuse constraints.
6. Existing decisions remain reachable after cleanup.
7. Broken internal Markdown links fail verification.
8. Domain ownership can be understood without reading implementation files.
9. Future research adds to the registry instead of growing `AGENTS.md`.
10. Each migration rollout is small enough for exact-head review.
11. Memory links remain organization metadata until an explicit context or
    retrieval action reaches the manifest.
12. Browser and computer-use receiving/emitting roles have distinct statuses,
    safety contracts, and evidence.
