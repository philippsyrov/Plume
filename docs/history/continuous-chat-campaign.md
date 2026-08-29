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
   needs. Every scenario carries a grounded `capabilityProbe`.

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

### Second correction pass (2026-08-28)

Re-review accepted the projection contract, the `v2` pin, the scenario-to-phase
mapping, and the ROADMAP reconciliation, and found two remaining defects.

**The probe was coupled backwards.** An `unimplemented` scenario became a hard
error the moment its struct or command appeared. That forbids ordinary
test-driven development: `FolderGrant` can exist for many commits before
revocation actually works, and the build would have failed for all of them. The
probe is now one-directional. Missing prerequisites still fail an `implemented`
claim, because absence really does prove the scenario cannot pass; prerequisites
arriving under an `unimplemented` scenario is a warning that invites a check.
Only passing behavioural evidence flips a status.

The same finding covered probe breadth. `requiredCommandSubstrings` matched any
command containing the substring, so `import` would have been answered by an
unrelated command. It is now `requiredCommandNames` with exact matching, and the
migration scenario names `sessions_import`.

**Evidence still accepted things that are not tests.** A Vitest `describe`
block counts as a grouping and can contain no tests at all; a Rust `fn` inside a
`_tests.rs` file is often a helper. Evidence now requires an `it`/`test`
declaration in TypeScript, or a `fn` carrying a test attribute in Rust.
`it.skip` does not qualify either.

**Stale pins settled.** Review agreed they belong to the established post-merge
smoke-and-repin slice: repinning here would fabricate packaged-smoke evidence
this slice did not gather, and squash-merge would orphan the branch hashes
anyway. They stay untouched.

### Third correction pass (2026-08-28)

Re-review accepted the one-directional probe, exact command matching, the
projection contract, and the roadmap/campaign agreement, and found two
remaining holes in evidence detection. Both were text-matching defects.

**TypeScript detection scanned raw text.** A comment, a string literal, or
`helper.test('name', fn)` could each masquerade as a runnable test. Detection
now parses the file with the TypeScript compiler and looks for a real call node
whose callee is the bare identifier `it` or `test`, with a matching string
literal and a function body. That also rules out `it.each` and the
one-argument `it('name')` placeholder.

**The Rust check accepted any nearby attribute containing the word `test`.**
`#[cfg(test)]` therefore certified a plain helper — and it is the attribute
most likely to be sitting above one, since it marks the whole test module. An
attribute now counts only when its path *is* `test` or ends in `::test`.
Comments are stripped before the search.

The language-specific logic moved to `scripts/docs/campaign-evidence.ts`, which
keeps both files inside the size guardrail.

### Fourth correction pass (2026-08-28)

Re-review found the evidence checker still conflating *declared* with *runs*.
Three cases, all now rejected with a regression test each:

- **TypeScript execution context.** The walk visited every call node, so a test
  inside `describe.skip(...)`, inside a function nobody calls, or behind an `if`
  counted. A test now qualifies only when it runs on load: a statement at module
  top level, or nested solely inside bare `describe(...)` suite bodies.
- **`#[ignore]`.** `cargo test` skips ignored tests by default, so one can never
  prove a scenario passes. The attribute now disqualifies the function.
- **Nested Rust block comments.** Rust nests them; the non-greedy regex ended an
  outer comment at the first inner close marker and re-exposed the rest,
  uncovering a commented-out `#[test]`. Replaced with a lexer that tracks
  nesting depth and skips string literals and raw strings.

Checked for over-tightening rather than only for the new rejections: every
`it(...)` name in a real React test file and every `#[test]` function in a real
Rust test module is still detected, and an invented name in each is still
refused.

### Fifth correction pass (2026-08-28)

Review ran runtime probes against an isolated snapshot and found two false
positives the suite had not covered.

- **A locally shadowed runner.** `const test = (_n, _f) => {}` above a
  `test('name', fn)` call made the "test" a no-op that reports nothing, and the
  checker accepted it. The runner must now resolve to an unaliased `vitest`
  import with no other binding of that name in the file. The frontend suite runs
  with `globals: false`, so that import is the positive fact worth checking.
- **`#[cfg_attr(test, ignore)]`.** An ignore in disguise. `cfg`, `cfg_attr`, and
  every `ignore` form now disqualify a Rust function.

**Measured cost of the `cfg` rule, stated rather than discovered later.** A
sweep over the repository found 56 of 913 existing `#[test]` functions are no
longer citable as evidence, all of them behind `#[cfg(unix)]` — tests that do
run on this platform. Deciding a `cfg` honestly would mean resolving the whole
feature and platform graph, so the rule fails closed instead. Campaign evidence
must name an unconditional test, which is a cheap constraint on code that is not
written yet, and no existing test changed.

The same sweep confirmed the tightening costs nothing on the TypeScript side:
all 1002 test names across 103 real test files still resolve.
