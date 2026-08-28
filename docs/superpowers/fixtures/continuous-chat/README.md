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

Phase 0 ships the corpus only. No scenario is backed by real behaviour yet.

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
| `fixtureRevision` | Revision of the record shape. Currently `v1`. |
| `phase` | The campaign phase that owns the scenario. |
| `intent` | One sentence of ordinary language. |
| `ownedState` | The typed record(s) that own the state under test. |
| `steps` | Ordered plain-language steps. |
| `expectedOutcome` | Assertions a later phase must satisfy. |
| `mustNotHappen` | Authority or data-loss outcomes that must never occur. |
| `implementationStatus` | `unimplemented` or `implemented`. |
| `automatedEvidence` | Paths to the real tests that prove the scenario. |

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
3. Set `implementationStatus` to `implemented` and list the real test file
   paths in `automatedEvidence`.

`automatedEvidence` must name repository-relative paths that resolve to real
regular files inside the repository — an absolute path, a path that escapes the
root, or a bare directory is a checker failure, not a judgement call. A
scenario claiming `implemented` with empty evidence fails too, and so does an
`unimplemented` scenario that carries evidence. Editing the status without
shipping the test in the same commit is the one thing this corpus exists to
prevent.

Run the checker on its own with `npx vite-node scripts/check-campaign-fixtures.ts`;
it is also part of `npm run verify:docs` and of the frontend suite that
`scripts/verify.sh` runs.
