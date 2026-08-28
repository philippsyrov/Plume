# Phase 0 — Contracts And Evaluation Fixtures (Slice Plan)

> **For agentic workers:** each task below gets a fresh implementation worker,
> then a spec-compliance review and a code-quality review before the next task
> starts. Treat every worker claim as a lead and verify it directly.

**Campaign:**
[`2026-08-27-continuous-chat-folder-grants-campaign.md`](2026-08-27-continuous-chat-folder-grants-campaign.md)

**Spec:**
[`2026-08-27-continuous-chat-folder-grants-design.md`](../specs/2026-08-27-continuous-chat-folder-grants-design.md)

**Base:** `claude/continuous-chat-phase-0` @
`662c9d70b52a5978f196f3e0da593aa2ffe5a3e8`

**Goal:** Make it mechanically checkable which typed record owns every piece of
conversation, context, memory, folder, and run state, and land a deterministic
scenario corpus the later phases flip from unimplemented to implemented. Ship
no consumer behaviour and no authority change.

## Verified baseline (measured on this exact head)

- `PLUME_FULL_VERIFY=1 ./scripts/verify.sh` → 53 pass, 3 doc soft-cap warnings,
  0 fail.
- `cd src-tauri && cargo test` → 1229 passed, 0 failed, 1 ignored (+7 in a
  second target).
- Absent today, confirmed by grep over `src-tauri/src`: any `*Grant*` struct or
  enum, any `*Proposal*` struct or enum, any transcript compaction or
  summarization record, and any session-store column for a summary
  (`src-tauri/src/sessions/schema.rs:161-265` covers every migration v1→v6).
- Present today: `ResearchRunRegistry` / `ResearchRunLease`
  (`src-tauri/src/research/run_registry.rs:24`, `:98`) — research-scoped only;
  `ChatStreamRegistry` (`src-tauri/src/chat/stream.rs:24`) is a cancellation
  map, not a lease.

## Binding constraint that shapes this slice

Phase 0 cannot ship literal red tests for the missing behaviours:

- `scripts/verify.sh` runs the **entire** vitest suite, so a red frontend test
  fails the verifier and CI.
- `PLUME_FULL_VERIFY=1` runs `cargo clippy --all-targets`, which **compiles
  test targets**. A Rust fixture naming `FolderGrant`, `CompactionCheckpoint`,
  `RunLease`, or `MemoryProposal` would not fail — it would fail to compile and
  break the verifier.

Therefore "prove the fixtures fail against the current implementation" is
delivered as: a deterministic **data** corpus whose scenarios each declare an
`implementationStatus`, a checker that refuses to let a scenario claim
implementation without existing evidence paths, and recorded absence evidence.
Each later phase's "failing test first" step is flipping its scenario entry and
writing the real test at the same commit.

## Task 1 — Typed state ownership contract

**Files**

- Create: `docs/STATE_OWNERSHIP.md`
- Modify: `docs/README.md` (index the new doc)
- Modify: `docs/ARCHITECTURE.md` (link from the state discussion)
- Modify: `docs/SAFETY.md` (authority-separation invariants)

**Content requirements**

1. One table: state category | owning Rust type | `file:line` | persistence
   location. Every row's `file:line` must be verified against this head.
2. A second table for the four **specified-but-absent** records
   (`FolderGrant`, `RunLease`, `CompactionCheckpoint`, `MemoryProposal`) marked
   plainly as not implemented, naming the phase that introduces each. These
   must not be described as scaffolded, partial, or reachable.
3. An explicit invariants section, at minimum:
   - compaction prose, display names, and summaries never confer authority;
   - a folder grant permits bounded reads only;
   - a run has exactly one writable root;
   - the frontend never supplies a trusted root after grant creation;
   - app-private and folder memory remain physically separate stores.
4. A statement that `docs/FEATURE_INVENTORY.md` remains the only
   repository-wide implementation-status ledger, and that this document
   describes ownership, not status.

**Checks:** `npx --no-install vite-node scripts/check-markdown-links.ts` and
`npx --no-install vite-node scripts/check-roadmap-docs.ts` both clean.

**Not in scope:** no `FEATURE_INVENTORY.md` record (Phase 0 proves no
behaviour), no Rust or TypeScript source change.

## Task 2 — Deterministic scenario corpus

**Files**

- Create: `docs/superpowers/fixtures/continuous-chat/README.md`
- Create one JSON per scenario under
  `docs/superpowers/fixtures/continuous-chat/`:
  - `repeated-compaction.json`
  - `memory-correction-and-forget.json`
  - `grant-revocation.json`
  - `reference-folder-write-rejection.json`
  - `run-cancellation.json`
  - `legacy-session-migration.json`

**Record shape (exact keys, no extras)**

```json
{
  "scenarioId": "repeated-compaction",
  "fixtureRevision": "v1",
  "phase": 2,
  "intent": "one sentence in ordinary language",
  "ownedState": ["CompactionCheckpoint"],
  "steps": ["ordered plain-language steps"],
  "expectedOutcome": ["assertions a later phase must satisfy"],
  "mustNotHappen": ["authority or data-loss outcomes that must never occur"],
  "implementationStatus": "unimplemented",
  "automatedEvidence": []
}
```

**Rules**

- `implementationStatus` vocabulary is exactly `unimplemented` or
  `implemented`. It deliberately does **not** reuse the inventory vocabulary
  (`shipped`/`partial`/`scaffold`/`researched`/`blocked`/`retired`) so the
  corpus can never read as a competing status ledger.
- Every scenario starts at `unimplemented` with an empty `automatedEvidence`.
- `mustNotHappen` must include the authority invariant relevant to that
  scenario (for example: the reference-folder scenario must assert no write
  ever lands outside the single writable root).

## Task 3 — Corpus checker and tests

**Files**

- Create: `scripts/docs/campaign-fixtures.ts`
- Create: `scripts/docs/campaign-fixtures.test.ts`

**Why here:** `scripts/docs/roadmap-docs.ts` + `roadmap-docs.test.ts` is the
established idiom for checked-in machine-validated repository data, and
`vitest.config.ts` has no `include` restriction, so `scripts/docs/*.test.ts`
already runs inside `npm run test` — and therefore inside `scripts/verify.sh` —
without editing the verifier.

**Checker must fail on**

- an unknown or missing key in any scenario record;
- an unknown `scenarioId`, a duplicate `scenarioId`, or a missing required
  scenario (all six must be present);
- an `implementationStatus` outside the two-word vocabulary;
- a scenario claiming `implemented` while `automatedEvidence` is empty or names
  a path that does not exist on disk;
- a scenario reusing an inventory status word in `implementationStatus`;
- an empty `intent`, `steps`, `expectedOutcome`, or `mustNotHappen`.

**Tests (write first, watch each fail, then implement)**

Cover: happy path; missing scenario; duplicate id; unknown key; bad status
word; `implemented` with empty evidence; `implemented` with a nonexistent
evidence path; empty required array. Use fixture objects in the test, not the
real corpus, plus one test that the **real** corpus parses clean.

**Expected result:** all new tests pass; total vitest count rises; no existing
test changes.

## Task 4 — Recorded gap evidence

**Files**

- Create: `docs/history/continuous-chat-campaign.md`
- Modify: `docs/history/README.md` (index the new campaign record)
- Modify: `docs/ROADMAP.md` (programme item 1 status line only)

**Correction recorded during planning:** an earlier draft of this plan said to
append to `docs/history/slice-ledger.md`. That is wrong. That file is a frozen
snapshot of the former chronology-heavy `AGENTS.md` (`docs/history/README.md`
says so, and `git log -- docs/history/` shows only two commits, both navigation
work). Appending per-slice entries there would corrupt a preserved artifact, so
this campaign gets its own chronological record instead.

**Content:** the measured baseline above, the grep-verified absence of the four
records, and the explicit statement that Phase 0 changed no consumer behaviour
and no authority. No status word in `FEATURE_INVENTORY.md` changes.

## Slice verification

1. `npx vitest run scripts/docs/campaign-fixtures.test.ts` — new tests pass.
2. `npm run test` — full frontend suite green, no pre-existing test modified.
3. `cd src-tauri && cargo test` — unchanged from baseline (1229 passed).
4. `PLUME_FULL_VERIFY=1 ./scripts/verify.sh` — 0 fail; warnings only the 3
   known doc soft-caps.
5. `npm run verify:docs`.
6. `git diff --stat` shows **zero** changes under `src/`, `src-tauri/src/`,
   and `src-tauri/capabilities/` — the mechanical proof of "no behaviour and no
   authority change".
7. Findings-only exact-head review; resolve every important finding.

**Packaged smoke:** not required. This slice changes no user-facing surface,
no native window, and no IPC. Record that reasoning rather than skipping
silently.

## Commits

One focused commit per task, on `claude/continuous-chat-phase-0`. Stop at the
PR/merge boundary for Philip's decision.
