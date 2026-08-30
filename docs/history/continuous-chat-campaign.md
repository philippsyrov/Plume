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

## Phase 1A — Durable Home conversation (2026-08-29)

**Merged:** `3dca93e` (#178), branch `claude/phase-1a-home-conversation`.

Local chat opens into one backend-owned Home conversation in app-private
storage. Schema v7 adds `is_home` plus a **partial unique index** on
`chat_sessions(is_home) WHERE is_home = 1`, which makes "at most one Home" a
database invariant rather than a call-site convention — two simultaneous
callers of `sessions.home` cannot produce two Homes, because one inserts and
the other reads what it inserted.

`sessions.home` takes an empty payload and is local-scope only. The frontend
learns Home's id every launch and never persists it, so it cannot choose which
conversation is Home.

Two decisions worth keeping:

- **Home is exempt from `MAX_SESSIONS`.** A store at its 200-session cap would
  otherwise be a store with no Home, which is the one conversation that must
  always exist.
- **Home refuses to be archived.** Archiving it would hide the conversation the
  user returns to, leaving them with an empty surface and no way back.

Startup resolves Home from the backend rather than selecting the most recently
updated chat. That heuristic stops pointing at Home the moment a second
conversation exists, which would defeat the whole point of relaunch landing in
the same place. Project scope keeps the heuristic; it has no Home.

**Found during review, after the branch was already written:** `is_home` was
missing from the `SELECT` in `home()` and in `branch.rs`, so `sessions.home`
always returned `isHome: false`. The test that should have caught it asserted
through `list()`, a different query, and passed.

**Open:** the packaged relaunch smoke. `docs/SMOKE_TESTING.md` step S13.

## Phase 1B — Durable storage cap (2026-08-29)

**Merged:** `816e37e` (#180), branch `claude/phase-1b-storage-cap`.

Each session store carries a 512 MB budget with a warning from nine tenths. A
save is refused before mutation. A fork or rewind is measured after tentative
writes inside its transaction and rolled back completely when over cap, so no
mutation is committed. Nothing is ever trimmed or deleted to make room: the tempting
failure — quietly dropping the oldest turns — would break the one guarantee the
whole conversation design rests on, so the store refuses instead. A refusal is
a failure the user can see and act on; a silent deletion is one they cannot.

Three measurement decisions, each of which was wrong first:

- **Pages in use, not file size and not `page_count`.** Neither shrinks after a
  delete — SQLite moves emptied pages onto a freelist and reuses them — so
  either would have left the documented recovery path a dead end: the user
  deletes a conversation and writes still refuse. `page_count - freelist_count`
  reflects the deletion immediately.
- **Bytes on both sides.** SQL `LENGTH()` counts *characters* for TEXT while
  Rust's `.len()` counts bytes, so a Cyrillic conversation looked half its real
  size in the store. A user could delete a third of it and still be refused.
  Every test that missed this was ASCII.
- **Projected usage, not `is_full()`.** Asking only "is it full yet?" admits any
  single write while one page remains, and a transcript can be megabytes. Two
  tests had enshrined the bug by asserting that a nearly-full store admits any
  write.

A save that shrinks or leaves a conversation the same size always lands.
Otherwise a user who filled the store could not edit their way back under it,
and deleting whole conversations would be the only exit.

## Conversation export (2026-08-29)

**Merged:** `cbbbc28` (#182), branch `claude/transcript-export`.

`sessions.export` renders one conversation to Markdown through the native Save
panel. It exists because the cap needs an exit: deletion is the only way to
reclaim space, and forcing that without a way to keep a copy first is a bad
trade.

The rendering keeps what an export could most easily misrepresent. A cancelled
turn keeps the partial answer that was on screen; an error turn appears as
itself; a research entry carries the note body resolved from the artifact
store, because deleting the conversation deletes the note too. Transcript prose
is escaped only where it could restructure the document from column zero, and
text placed inside the export's own emphasis markers is escaped so it cannot
close them early.

**Hardened:** `90e07d3` (#188) makes export failures visible and has the
storage-cap recovery notice direct the user to the row-menu export action. No
packaged export smoke is recorded.

## Phase 2A — Compaction provenance rules (2026-08-29)

**Merged:** `d6a5067` (#181), then `698bac3` (#185).

`src-tauri/src/sessions/checkpoint.rs` holds the rules a compaction checkpoint
must obey. It is deliberately dead code: `#![allow(dead_code)]`, exercised by
its tests only. Settling and reviewing the resolution policy before anything
depends on it was the point.

Every fact a checkpoint carries names where it came from, and that provenance is
re-checked on every use rather than trusted from the last one. Without that,
compaction quietly defeats forget: a fact copied into a checkpoint outlives the
memory entry it came from, and because the next compaction summarizes the
checkpoint rather than the source, each generation launders the fact further
from anything the user can inspect or revoke.

**The forget hole, found after #181 merged.** Refusing a stale fact marks its
checkpoint for rebuild — and a rebuild reads retained history, where the turn
that produced the fact is still sitting, because Plume never deletes history. So
the rebuild derives the same fact again, this time with no memory link to refuse
it by, and forget lasts exactly one projection. `#185` closes it with a forget
record naming the turns the memory was drawn from, excluded from
*summarization only*. The turn stays in the transcript, on screen, and
exportable: the user asked Plume to stop knowing something, not to erase what
they said.

**Not landed, and easy to misread as landed:** the forget record is a type, not
a store. There is no column and no write path; `src-tauri/src/memory/mod.rs`
still documents forget as a hard delete with no tombstone. Phase 2B owns
persistence, projection, and triggering.

## Phase 2B-1 — Durable memory revisions (2026-08-30)

**Merged:** `e285ff7` (#192).

`MemoryEntry` and `UserMemoryEntry` now carry a durable revision that starts at
zero and advances on text rewrites, while link-only edits leave it unchanged.
Legacy rows default to zero and zero stays omitted on disk so a rewrite does
not grow the bounded store merely to spell out the default.

The next dependency is stable transcript identity. A transcript save still
replaces every `chat_messages` row and mints fresh database ids, so those ids
cannot yet be persisted as `FactProvenance.source_turn_ids`.
