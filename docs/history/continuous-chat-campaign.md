# Continuous Chat And Folder Grants — Campaign Record

Chronological implementation evidence for the
[Continuous Chat and Folder Grants programme](../superpowers/specs/2026-08-27-continuous-chat-folder-grants-design.md).
This file is history, not status. Current capability lives in
[`FEATURE_INVENTORY.md`](../FEATURE_INVENTORY.md), ordered work in
[`ROADMAP.md`](../ROADMAP.md), and per-record ownership in
[`STATE_OWNERSHIP.md`](../STATE_OWNERSHIP.md).

One entry per phase, appended when that phase's slice is verified.

## Phase 0 — Contracts and evaluation fixtures (2026-08-28)

**Branch:** `claude/continuous-chat-phase-0`, based on
`662c9d70b52a5978f196f3e0da593aa2ffe5a3e8`.

**Landed in this slice:** documentation and repository data only.

- `docs/STATE_OWNERSHIP.md` — which typed Rust record owns each piece of
  conversation, context, memory, folder, and run state, plus a separate table
  for the four records the programme specifies but has not implemented.
- `docs/SAFETY.md` — new `## Authority separation` section.
- `docs/superpowers/fixtures/continuous-chat/` — six deterministic scenario
  records and their README.
- `scripts/docs/campaign-fixtures.ts` and its test — the corpus checker and
  the executable capability probe.

**Deliberately not shipped:** no consumer behaviour, no authority change, and
no `FEATURE_INVENTORY.md` record. Phase 0 proves no behaviour, so it earns no
status entry. `git diff` for this slice touches nothing under `src/`,
`src-tauri/`, or `src-tauri/capabilities/`.

**Baseline measured on the base commit before any change:**

- `PLUME_FULL_VERIFY=1 ./scripts/verify.sh` — 53 pass, 3 doc soft-cap
  warnings, 0 fail.
- `cd src-tauri && cargo test` — 1229 passed, 0 failed, 1 ignored, plus 7 in a
  second target.

**Absence verified by grep over `src-tauri/src` at the base commit**, and the
reason the corpus is data rather than red tests: no `*Grant*` type, no
`*Proposal*` type, no transcript compaction or summarization record, and no
session-store summary column — `src-tauri/src/sessions/schema.rs` carries every
migration through v6 and adds none.

`scripts/verify.sh` runs the whole vitest suite, and `PLUME_FULL_VERIFY=1` runs
`cargo clippy --all-targets`, which compiles test targets. A Rust fixture
naming `FolderGrant`, `CompactionCheckpoint`, `RunLease`, or `MemoryProposal`
would therefore not fail — it would fail to compile and break the verifier. So
the acceptance targets are recorded as a deterministic data corpus, and the
"these cannot pass yet" half is proved by an executable probe rather than by a
red test: `probeScenarios` searches the current tree for the type declarations
and commands each scenario needs, and reports all six unsatisfied. Each later
phase flips its scenario in the same commit as the real failing-then-passing
test.

**Correction made during this slice:** the first draft of the slice plan said
to append to `docs/history/slice-ledger.md`. That file is a frozen snapshot of
the former chronology-heavy `AGENTS.md`, so this campaign record was created
instead and the plan was corrected in place rather than silently changed.

**Known pre-existing condition, untouched by this slice:**
`scripts/check-roadmap-docs.ts` emits 45 `may be stale` warnings at this base
commit — inventory records whose owned paths changed since their
`lastVerifiedCommit` during the preceding cleanup commits. They are warnings,
the checker exits 0, and repinning requires proving behaviour on this head, so
it belongs to its own slice.

### Correction pass (2026-08-28)

Review found the first cut of this slice not merge-ready. Three important gaps,
all closed on this branch:

1. **The corpus could not prove failure.** JSON-schema validation plus grep
   evidence cannot distinguish history, projection, memory, grants, or run
   authority. Replaced with `probeScenarios({ root })` — an executable probe
   that searches the tree for the declarations and commands each scenario
   needs. Every scenario carries a grounded `capabilityProbe`, and the checker
   treats probe/status disagreement as an error in both directions.

   The probe matches declarations (`struct <Name>` / `enum <Name>`), not bare
   substrings. A substring search for `RunLease` returns five hits today, all
   of them the unrelated `ResearchRunLease` at
   `src-tauri/src/research/run_registry.rs:98` — so a naive probe would have
   reported that capability present and been silently useless. A regression
   test pins this.

2. **The evidence ratchet accepted any repository file.** A scenario could
   claim `implemented` citing `README.md`. `automatedEvidence` entries are now
   `{ path, testName }` objects: the path must be a test file by extension, and
   the named test must actually appear in it.

3. **`STATE_OWNERSHIP.md` omitted projected model context.** Added a
   `## Projected model context` section covering eleven ephemeral derived
   types across `assemble.rs`, `explicit_context.rs`, and
   `context_manifest.rs`, each with zero authority.

Lower-severity drift closed in the same pass: `fixtureRevision` must now be
exactly `v2`; the canonical `scenarioId` → phase mapping is enforced so a
scenario cannot reassign its own phase; and the campaign plan's Phase 0
checkboxes and this ROADMAP entry now describe the same state.

The 45 stale feature-inventory pins remain untouched and unresolved. Nine of
the ten records nearest this work rest on packaged-app or hardware smoke
evidence taken at older heads; repinning them here would assert a verification
this slice did not perform. The repository has four precedents for handling
that as its own post-merge slice: `015125e`, `eb8024d`, `8f59035`, `a0b62de`.
