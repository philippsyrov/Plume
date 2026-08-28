# Continuous Chat Scenario Corpus

Deterministic scenario records for the continuous chat and folder grants
campaign. Each JSON file describes one end-to-end situation the campaign must
eventually satisfy, written in plain language so a reviewer can read it without
opening any source file.

**Spec:**
[`2026-08-27-continuous-chat-folder-grants-design.md`](../../specs/2026-08-27-continuous-chat-folder-grants-design.md)

**Campaign:**
[`2026-08-27-continuous-chat-folder-grants-campaign.md`](../../plans/2026-08-27-continuous-chat-folder-grants-campaign.md)

## What this corpus is for

It fixes the acceptance targets before the behaviour exists. Each record names
the typed record that owns the state under test, the ordered steps, the
assertions a later phase must satisfy, and — most importantly — the authority
and data-loss outcomes that must never occur. The `mustNotHappen` list is the
part that outlives any particular implementation: it is where the spec's
security and privacy invariants are written down per scenario.

Phase 0 ships the corpus and an executable probe that observes, against the
current tree, whether the capability each scenario needs exists yet. Today
every probe reports unsatisfied, which is the machine-checked form of "these
scenarios cannot pass against this implementation".

## What this corpus is not

It is **not** a status ledger. `docs/FEATURE_INVENTORY.md` remains the only
repository-wide implementation-status ledger, and nothing here changes or
duplicates it.

To keep that boundary mechanical, `implementationStatus` deliberately avoids
the inventory vocabulary. Its only two values are `unimplemented` and
`implemented`. The inventory words — `shipped`, `partial`, `scaffold`,
`researched`, `blocked`, `retired` — are never valid here, so a scenario can
never be read as a competing claim about what the product does.

## Record shape

Every file uses exactly these keys, with no extras and no omissions:

| Key | Meaning |
| --- | --- |
| `scenarioId` | Equals the filename stem. |
| `fixtureRevision` | Revision of the record shape. Must be exactly `v2`. |
| `phase` | The campaign phase that owns the scenario. Must match the canonical `scenarioId` → phase mapping in the checker; a scenario cannot reassign itself. |
| `intent` | One sentence of ordinary language. |
| `ownedState` | The typed record(s) that own the state under test. |
| `steps` | Ordered plain-language steps. |
| `expectedOutcome` | Assertions a later phase must satisfy. |
| `mustNotHappen` | Authority or data-loss outcomes that must never occur. |
| `capabilityProbe` | What must exist in the tree before the scenario could pass. See below. |
| `implementationStatus` | `unimplemented` or `implemented`. |
| `automatedEvidence` | `{ path, testName }` objects naming the real tests that prove the scenario. |

## Executable capability probe

`capabilityProbe` is the part a reader can run. It names the concrete things
that must exist before the scenario could possibly pass:

- `requiredTypeDeclarations` — Rust type names that must be declared under
  `src-tauri/src`. The probe matches a declaration
  (`struct <Name>` or `enum <Name>` on a word boundary), never a bare
  substring. That distinction is load-bearing: a substring search for
  `RunLease` matches the unrelated `ResearchRunLease` in
  `src-tauri/src/research/run_registry.rs` and would report the capability as
  present when it is not.
- `requiredCommandNames` — exact command names that must appear as quoted
  strings inside the `APP_COMMANDS` list in `src-tauri/src/app_commands.rs`.
  Exact, not substring: a probe for `import` would be answered by any unrelated
  command whose name merely contains it.

`probeScenarios({ root })` returns one observation per scenario and
`renderProbeReport` prints it.

**The probe is one-directional, and that is deliberate.** Missing prerequisites
prove the scenario cannot pass, so a scenario claiming `implemented` over a
missing prerequisite is a hard error. The converse does not hold: a
`FolderGrant` type can exist for many commits before revocation actually works,
and failing the build the moment the type appears would forbid ordinary
test-driven development. So an `unimplemented` scenario whose prerequisites have
landed produces a **warning**, not an error — a nudge to check whether a passing
test can now flip it.

Only passing behavioural evidence flips a status. The probe is a necessary
condition, never a sufficient one.

## Scenarios

| File | Phase | Owning state |
| --- | --- | --- |
| `repeated-compaction.json` | 2 | `CompactionCheckpoint` |
| `memory-correction-and-forget.json` | 3 | `MemoryProposal`, `UserMemoryEntry` |
| `grant-revocation.json` | 4 | `FolderGrant` |
| `legacy-session-migration.json` | 5 | `SessionRecord` |
| `reference-folder-write-rejection.json` | 6 | `RunLease`, `FolderGrant` |
| `run-cancellation.json` | 6 | `RunLease` |

## How a later phase flips a scenario

A scenario is flipped in the **same commit** as the real test that proves it:

1. Write the focused test and watch it fail against the current head.
2. Implement the behaviour until it passes.
3. Set `implementationStatus` to `implemented` and list the real tests in
   `automatedEvidence` as `{ path, testName }` pairs.

Each evidence `path` must be a repository-relative path resolving to a real
regular file inside the repository — an absolute path, a path that escapes the
root, or a bare directory is a checker failure, not a judgement call. It must
also *be a test file* (`.test.ts`, `.test.tsx`, `.spec.ts`, `.spec.tsx`, or
`_tests.rs`), and its `testName` must name a test that really runs:

- in TypeScript, a real `it('name', fn)` or `test('name', fn)` call node, found
  by parsing the file with the TypeScript compiler rather than searching its
  text. A regex cannot tell a test from the same characters inside a comment,
  inside a string literal, or in `helper.test('name', fn)`. The callee must be
  the bare identifier, so `describe` (a grouping that may hold no tests),
  `it.skip` (never runs), and `it.each` (the name is a template, not this
  string) are all rejected, as is a one-argument `it('name')` placeholder. A
  scenario proved by a table-driven test names a plain test instead.

  The runner itself must resolve to the real one: an unaliased import of `it`,
  `test`, or `describe` from `vitest`, with no other binding of that name in
  the file. The suite runs with `globals: false`, so a genuine runner is always
  such an import — and without that check a local `const test = () => {}`
  no-op above the call passes as evidence.

  The call must also *run on load*: a statement at module top level, or nested
  only inside bare `describe(...)` suite bodies. Declared is not the same as
  runs — walking every call node would accept a test inside `describe.skip`,
  inside a helper nobody calls, or behind an `if`.
- in Rust, a `fn` whose attribute block contains a genuine test attribute —
  one whose path *is* `test` or ends in `::test`, so `#[test]` and
  `#[tokio::test]` qualify. `#[cfg(test)]` does not: it marks an item as
  compiled under the test profile, which every helper in the module carries
  too. Anything in the attribute block that could stop cargo running the test
  disqualifies it: `ignore` in every form, and `cfg` / `cfg_attr` — which
  covers `#[cfg_attr(test, ignore)]`, an ignore in disguise. Deciding a `cfg`
  honestly would mean resolving the whole feature and platform graph, so the
  rule fails closed and campaign evidence must name an unconditional test.
  That refuses `#[cfg(unix)]` tests too, which is a cheap constraint on code
  that is not written yet.

  Comments are stripped by a small lexer, not a regex. Rust block comments
  nest, so a non-greedy pattern ends an outer comment at the first inner close
  marker and re-exposes the rest — enough to uncover a commented-out
  `#[test]`. The lexer tracks nesting depth and skips string literals, so a
  comment marker inside a string cannot open a comment.

Naming `README.md`, a `describe` block, a commented-out test, or an
unattributed Rust helper is rejected.

The language-specific half of this lives in
[`scripts/docs/campaign-evidence.ts`](../../../../scripts/docs/campaign-evidence.ts).

A scenario claiming `implemented` with empty evidence fails too, and so does an
`unimplemented` scenario that carries evidence. Editing the status without
shipping the test in the same commit is the one thing this corpus exists to
prevent.

Run the checker on its own with `npx vite-node scripts/check-campaign-fixtures.ts`;
it is also part of `npm run verify:docs` and of the frontend suite that
`scripts/verify.sh` runs.
