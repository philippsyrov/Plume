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
behavior, reprioritize the active benchmark campaign, or discard existing
research.

## Decisions

### Finish The Active Campaign First

The active sequence remains:

1. D131 model catalog and benchmark presets.
2. D132 in-app benchmark results viewer.
3. Full measured matrix on the 128 GB M5 Max when available.
4. D130 evidence-backed README and launch rewrite.
5. Second Brain and linked-work-object implementation track.

The documentation work may record later tracks now, but it must not present
them as the next implementation priority.

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

1. Benchmark evidence and launch readiness.
2. Local model ownership and runtime quality.
3. Safe coding-agent execution.
4. Project knowledge and Second Brain.
5. Linked work objects and context handoff.
6. Skills, tools, plugins, and external agents.
7. Operability, safety, observability, and computer use.
8. Documentation and agent navigation.

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
- Claude Code public behavior and source-hygiene lessons;
- Hermes Agent;
- Qoder and Repo Wiki / Knowledge Hub;
- Notion linked agent workflows;
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
- visible JSONL project memory with CRUD, prompt injection, and text search;
- curated `INDEX.md`, `USER.md`, `SOUL.md`, and topic-file prompt context;
- exact-duplicate memory compaction with confirmation and audit log;
- typed agent events, approval/config foundations, and patch-only single-step
  execution;
- Plume-managed MLX runtime and benchmark harness;
- safe diff validate/apply/revert and checkpoints.

### Scaffolded But Not Fully Executing

- bounded loop controller;
- persistent command approval ledger consumers;
- progressive tool catalog beyond read-only discovery;
- complete read/edit/test/fix loop;
- broad external tool and MCP execution;
- multi-agent coordination.

### Researched But Not Shipped

- local semantic/vector memory;
- scheduled or background dreaming/distillation;
- LLM-assisted memory clustering and summaries;
- stale-fact contradiction detection and pruning;
- distilled project/session profiles;
- repository wiki generation and knowledge cards;
- backlinks and knowledge graph navigation;
- linked `signal -> task -> spec -> run -> review -> artifact` objects;
- drag-and-drop context bundles and agent handoff;
- automatic skill creation and skill improvement;
- full computer-use execution.

This ledger prevents “documented” from being interpreted as “implemented.”

## Source-Specific Decisions

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
paths.

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
- keep D131/D132/D130 priority unchanged.

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

- refresh Qoder, Hermes, and Rust-agent comparisons against pinned upstream
  revisions;
- record new ideas through the registry before adding roadmap work;
- reject duplicates by linking new observations to existing feature ids.

Each rollout is its own PR and stops for exact-head review. No rollout changes
product behavior.

## Non-Goals

- implementing the Second Brain;
- changing the D131-D132-D130 campaign order;
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
4. The active benchmark campaign remains the first implementation priority.
5. External research has explicit provenance and reuse constraints.
6. Existing decisions remain reachable after cleanup.
7. Broken internal Markdown links fail verification.
8. Domain ownership can be understood without reading implementation files.
9. Future research adds to the registry instead of growing `AGENTS.md`.
10. Each migration rollout is small enough for exact-head review.
