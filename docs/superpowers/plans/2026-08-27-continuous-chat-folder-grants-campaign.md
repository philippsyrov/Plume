# Continuous Chat And Folder Grants Campaign Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use
> `superpowers:subagent-driven-development` (recommended) or
> `superpowers:executing-plans` to implement this campaign slice-by-slice.
> Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace Plume's user-facing Project/session split with one durable
Home conversation, transparent compaction, reviewable learning, opaque
multi-folder read grants, one-writable-folder run leases, a bounded coding
loop, and evidence-backed local-model validation.

**Architecture:** New consumer conversations are app-private and may reference
zero or more Rust-owned folder grants. Durable history, projected model
context, compaction checkpoints, memory, source manifests, folder trust, and
run permissions remain separate typed state. Each coding run receives exactly
one writable folder plus optional read-only reference folders.

**Tech Stack:** Tauri 2, Rust 2021, rusqlite, React 19, TypeScript, Vitest,
Testing Library, CodeMirror 6, MLX-LM/MLX-VLM, packaged macOS smoke.

**Spec:**
[`docs/superpowers/specs/2026-08-27-continuous-chat-folder-grants-design.md`](../specs/2026-08-27-continuous-chat-folder-grants-design.md)

## Global Constraints

- Read `AGENTS.md` and its complete required chain before every slice.
- Re-check exact `HEAD`, origin relationship, remotes, worktrees, and
  cleanliness; never act from a handoff claim alone.
- Work only in the assigned Plume worktree. Preserve unrelated changes.
- Treat this campaign as ordered product authority, not permission for one
  monster branch or one monster PR.
- Start every behaviour change with a failing focused test.
- Keep full conversation history; compaction is derived state and never
  deletes source turns.
- Compaction, memory, folder grants, accepted sources, and run permissions are
  different types and never confer authority on one another.
- The frontend sends opaque references. Rust re-resolves roots and sources
  through trust, path, size, binary, hardlink, redaction, and ownership gates.
- One run has exactly one writable folder. Additional folders are read-only.
- No broad shell strings, arbitrary `tools.invoke`, hidden writes, blanket
  approval, autonomous Browser authority, or macOS host control.
- No install, runtime download, or model download without Philip's explicit
  approval.
- Preserve accessible names, keyboard paths, visible errors, approval,
  cancellation, Stop, diff review, and ordinary-language UI copy.
- Keep new code files at or below the 800-line cap and split by ownership.
- Update current contracts and `docs/FEATURE_INVENTORY.md` only for behaviour
  proved on the exact implementation head. Put chronology only in history.
- Run focused checks first, then
  `PLUME_FULL_VERIFY=1 ./scripts/verify.sh`, packaged smoke where required,
  and a findings-only exact-head review before commit/push workflow.
- Use one focused branch and PR per independently reviewable slice. Do not
  merge unless Philip commissions the merge.

---

## Driver workflow repeated for every phase

- [ ] Re-read the approved programme spec and the current roadmap entry.
- [ ] Inspect the owning frontend/Rust domain maps, contracts, implementation,
      migrations, and tests end to end.
- [ ] Confirm the previous dependency phase is present in the branch base.
- [ ] Use `superpowers:brainstorming` if repository truth reveals a product or
      authority decision not settled by the approved spec.
- [ ] Use `superpowers:writing-plans` to create one exact slice plan under
      `docs/superpowers/plans/`; include file paths, interfaces, failing tests,
      commands, expected failures/passes, documentation, smoke, and commits.
- [ ] Execute the slice with `superpowers:subagent-driven-development`.
- [ ] Give each implementation task a fresh worker and run spec-compliance and
      code-quality review before moving to the next task.
- [ ] Treat worker/reviewer claims as leads; verify important facts directly.
- [ ] Run the slice's focused tests after each task and repair regressions while
      their cause is local.
- [ ] Run the complete relevant test suite and full verifier.
- [ ] Run the required packaged-app smoke and record exact-head evidence.
- [ ] Update contracts, maps, inventory, roadmap status, and smoke docs to the
      behaviour actually proved.
- [ ] Run a final findings-only exact-head review; resolve every important
      finding and re-run affected checks.
- [ ] Commit the focused slice, then follow the repository's PR/CI/gitleaks
      gates. Stop at any merge decision requiring Philip.

## Campaign sequence

### Phase 0 — Contracts and evaluation fixtures

- [x] Specify typed history, projection, compaction, memory proposal, folder
      grant, accepted-source, and run-lease boundaries in current contracts.
      ([`STATE_OWNERSHIP.md`](../../STATE_OWNERSHIP.md))
- [x] Add deterministic fixtures for repeated compaction, correction/forget,
      grant revocation, reference-folder write rejection, cancellation, and
      legacy-session migration.
      ([corpus](../fixtures/continuous-chat/README.md))
- [x] Prove the fixtures fail against the current implementation for the
      intended missing behaviours without weakening shipped tests. The
      executable probe in `scripts/docs/campaign-fixtures.ts` reports all six
      scenarios unsatisfied against this head, and no existing test changed.
- [x] Ship no consumer behaviour or authority change in this phase. Nothing
      under `src/`, `src-tauri/`, or `src-tauri/capabilities/` was touched, and
      no `FEATURE_INVENTORY.md` record was added.

**Gate:** Reviewers can identify which typed record owns every piece of state,
and no summary or display label can be mistaken for permission.

### Phase 1 — Durable Home conversation

- [x] Add one backend-owned app-private Home identity with idempotent creation,
      load, stable save, and relaunch restoration. Schema v7 carries `is_home`
      behind a partial unique index, so "exactly one Home" is a database
      invariant. `sessions.home` is local-scope only and takes no id.
      (`3dca93e`; `src-tauri/src/sessions/home_tests.rs`)
- [x] Route startup and ordinary no-folder chat to Home while keeping existing
      local/project session APIs and stores compatible. Project scope keeps
      lazy creation and the most-recent heuristic.
      (`3dca93e`; `src/features/sessions/usePersistedChat.test.tsx`)
- [x] Preserve fork, rewind, archive, deletion, accepted-turn manifests,
      Browser ownership, cancellation, and streaming boundaries. Home is an
      ordinary row for every one of them except archive, which it refuses —
      archiving the one conversation that must always exist would hide it.
      (`3dca93e`; `home_tests.rs::home_cannot_be_archived`)
- [ ] Add packaged smoke proving repeated relaunch returns to the same visible
      chronology without opening or trusting a folder.
- [x] Define and enforce the durable storage cap: warn while approaching it,
      refuse further appends at it, and offer review and explicit deletion.
      Never trim or roll over a transcript to make room.
- [x] Test the cap directly — appends refused, existing history still readable,
      and recovery through explicit deletion.
- [ ] Offer export as a recovery path. Export itself has merged (`cbbbc28`),
      so the original reason this box was open no longer applies — but the box
      stays open for a different one: the storage-cap notice in
      `src/features/sessions/SessionNotices.tsx` still names deletion alone, and
      a failed export is logged to the console rather than shown. Both are
      addressed by PR #188, which is open and not merged.

**Gate:** Home works reliably while the existing Projects UI remains available
as a compatibility path.

### Phase 2 — Transparent provider-neutral compaction

- [ ] Add immutable checkpoint persistence and migrations without replacing or
      deleting transcript entries.
- [ ] Build context projection from canonical instructions, current structured
      authority, a validated checkpoint, complete recent turns, approved
      memory, and exact attached-source resolution.
- [ ] Keep user-turn and tool request/result boundaries intact.
- [ ] Add deterministic triggering against provider context budgets with
      reserve, bounded summary output, cancellation, and concurrency fences.
- [ ] Add Review and Rebuild from history without exposing internal noise in
      the default transcript.
- [ ] Add a revision to `MemoryEntry` and `UserMemoryEntry`, with its
      migration. Still true at `cbbbc28`: neither
      `src-tauri/src/memory/types.rs` nor `src-tauri/src/memory/user_store.rs`
      carries one, while `checkpoint.rs` already assumes one in
      `MemoryProvenance.revision`. Phase 2 cannot validate a
      revision that does not exist — so the field lands here rather than being
      pulled forward out of Phase 3 mid-slice. Phase 3 still owns correction
      and forget semantics on top of it.
- [ ] Record provenance on every checkpoint fact — source turn ids, and the
      memory entry id and revision when it restates one — and re-resolve that
      provenance on every projection rather than trusting the last one. The
      rule landed (`d6a5067`, `src-tauri/src/sessions/checkpoint.rs`) under
      `#![allow(dead_code)]`. It cannot close until there is a projection to
      run it in: nothing calls `resolve_facts`.
- [ ] Drop facts whose source memory was forgotten or revised, or whose source
      turns left retained history, and rebuild the stale checkpoint from
      history instead of re-summarizing it. Same status: `FactRefusal` and
      `resolve_facts` implement it (`d6a5067`), with no caller.
- [ ] Test at least three successive compactions, checkpoint corruption,
      cancellation, stale completion, overflow, relaunch, fork, and rewind.
- [ ] Regression for the laundering path specifically: compact a fact into a
      checkpoint, forget or correct its source memory, then prove the next
      projection excludes it — and still excludes it after a further
      compaction cycle.
- [ ] Persist a forget record naming the turns a forgotten memory was drawn
      from, and exclude those turns from rebuild summarization. Re-resolving an
      already-filtered checkpoint does not test this; the regression must
      rebuild from retained history, where the original turn still sits.
      The *rule* landed (`698bac3`: `ForgottenMemory`, `forgotten_turn_ids`,
      `rebuildable_turn_ids`). **Persistence has not** — there is no store, no
      column, and `src-tauri/src/memory/mod.rs` still documents forget as a hard
      delete with no tombstone. Do not read #185 as having closed this.

**Gate:** Long conversations continue without a new chat and without losing a
standing constraint, unsettled action boundary, or canonical safety state.

### Phase 3 — Reviewable learning

- [ ] Add typed memory proposals for direct remember requests, explicit user
      preferences/corrections, and repeated stable workflow choices only.
- [ ] Add Remember, Edit, Not now, Never suggest this, scope selection,
      provenance, revisions, conflict handling, correction, and forget.
- [ ] Preserve physical separation between app-private and folder memory.
- [ ] Keep ambient injection disabled in the first slice.
- [ ] If separately commissioned, add ambient use only with deterministic caps,
      exact accepted-memory manifests, valid scope, and immediate correction or
      forget on the next projection.

**Gate:** Plume can explain why it remembers a fact, where it applies, and prove
that correction/forget changes the next eligible projection.

### Phase 4 — Opaque read-only multi-folder grants

- [ ] Add native folder selection/trust that returns an opaque grant id and
      safe display metadata, not a caller-reusable root.
- [ ] Re-resolve every grant in Rust and reuse canonical path, symlink,
      hardlink, binary, size, secret-name, redaction, and ownership gates.
- [ ] Permit one conversation to hold multiple read-only grants and exact
      folder-bound context references.
- [ ] Add revoke, missing/moved folder, stale response, relaunch, and wrong
      grant/source tests.
- [ ] Prove attachment never grants commands, patches, exports, Browser
      actions, or model startup.

**Gate:** Home can answer from two approved folders with an exact manifest and
zero unapproved cross-folder reads or writes.

### Phase 5 — Chat-first shell and legacy compatibility

- [ ] Make Home chat the stable consumer entrypoint.
- [ ] Replace Open Project and permanent Projects navigation with contextual
      Add folder, folder chips, Working folder, and Reference folder language.
- [ ] Keep History, Library, Models, Settings, Browser, Files, diffs, and run
      trace progressively disclosed around chat.
- [ ] Preserve Continue, Rewind, branch, archive, search, and recovery.
- [ ] Surface legacy project chats only after their exact folder is granted;
      provide explicit copy/import without deleting or rewriting the source.
- [ ] Run fresh-install and upgraded-user packaged walkthroughs at normal and
      narrow window sizes with keyboard and accessibility checks.

**Gate:** A fresh user never needs to understand Projects, while an existing
user can still recover every prior chat and folder-owned record.

### Phase 6 — One-writable-folder run leases

- [ ] Add Rust-owned leases with one writable grant, optional read-only grants,
      file/argv allowlists, approval policy, iteration/time/output budgets,
      expiry, and cancellation.
- [ ] Add the plain-language run preview and visible trace.
- [ ] Reuse patch validation, checkpointed atomic apply, drift-checked revert,
      and verifier result capture for text files.
- [ ] Add the guarded whole-file artefact write for binary and generated
      documents: the run proposes a complete file, the preview names what it is,
      where it lands, and how large it is, approval writes it atomically inside
      the writable root, and the previous bytes become the checkpoint so revert
      restores the file. Never widen the patch path to cover binaries.
- [ ] Reject writes, commands, generated exports, and patch targets outside the
      writable grant even when a model or stale UI requests them.
- [ ] Contain every spawned process before it starts, by OS-enforced sandbox or
      by a purpose-built verifier whose reach is fixed in Plume's own code. An
      approved argv is not containment: a spawned verifier otherwise inherits
      Plume's full host filesystem access.
- [ ] Deny network egress from command sandboxes by default, and make network
      access its own visible grant. Filesystem containment alone still lets an
      approved test script upload everything it can read.
- [ ] Fail closed where containment is unavailable — refuse to execute, record
      the refusal in the trace, and leave the patch for the user to test
      outside Plume. Never fall back to an uncontained spawn.
- [ ] Drift-check artefact revert against the bytes Plume wrote, and define
      revert for a newly created file: remove it when unchanged, leave it when
      edited since, and remove a directory created for it only while empty.
- [ ] Test containment adversarially: a verifier that attempts to write outside
      the writable root, into a reference grant, and outside every grant.
- [ ] Test revocation/expiry between proposal and action, Stop during each
      action class, concurrent windows, process exit, relaunch, and uncertain
      command settlement.

**Gate:** A run can safely modify and verify one folder while every reference
folder remains provably read-only.

### Phase 7 — Bounded multi-iteration coding loop

- [ ] Connect the existing controller scaffold to lease-backed file reads,
      patch operations, and exact approved-command execution.
- [ ] Preserve typed progress, settled-action boundaries, retry policy,
      iteration caps, output caps, cancellation, checkpoints, and visible
      failures.
- [ ] Add representative success, failing-test/fix, malformed-model-output,
      repeated failure, cancellation, timeout, patch drift, and recovery tests.
- [ ] Complete packaged smoke with a real local model only after the fake
      runtime proves the same control flow deterministically.

**Gate:** Plume completes one read/edit/test/fix task through a failure and
correction without broad tools, an escaped path, or a repeated uncertain
action.

### Phase 8 — Evidence-backed local-model task matrix

- [ ] Evaluate the exact current runtime against Qwen3.8-27B compatibility
      before proposing a runtime change.
- [ ] If required, commission a separate pinned MLX-LM/MLX-VLM runtime update
      with hashes, build/packaging tests, cancellation, chat-template,
      reasoning, vision, structured-action, and rollback fixtures.
- [ ] Request explicit approval before any runtime or model download.
- [ ] Run Qwen3.8-27B on the real bounded task matrix: instruction retention,
      tool/action framing, read/edit/test/fix completion, cancellation, vision,
      time-to-first-token, generation speed, peak memory, and recovery.
- [ ] Compare Muse Glimmer only as a challenger against the exact selected Qwen
      3.x 30–37B-class checkpoint.
- [ ] Keep Qwen3.8-Flash-Next outside the practical catalogue at its current
      footprint and keep GLM-5.3 candidate-only until verified artifacts and
      runtime evidence exist.
- [ ] Update model tiers, catalogue, providers docs, benchmark records, and
      product claims only from exact hardware/runtime/fixture/commit evidence.

**Gate:** Model selection reflects measured Plume task completion and resource
behaviour, not vendor benchmarks or a successful generic chat response.

### Phase 9 — Later guarded capabilities

- [ ] Commission one allowlisted skill/tool execution path only after Phase 7.
- [ ] Keep agent Browser actions, scheduled work, sandboxed computer-use
      emission, and any host-level authority as separate designs and PRs.

**Gate:** No later capability inherits folder, memory, Browser, command, or
host authority merely because it is visible to the model.

## Final campaign proof

- [ ] One Home conversation survives relaunch and three compaction cycles.
- [ ] Full history remains inspectable and rebuildable.
- [ ] Approved learning is scoped and sourced; correction and forget affect the
      next eligible projection.
- [ ] One conversation reads from multiple granted folders with exact source
      manifests.
- [ ] Every coding run has one writable root and zero reference-folder writes.
- [ ] Stop settles model, command, verifier, and patch activity visibly.
- [ ] Legacy sessions and folder data remain recoverable.
- [ ] The bounded loop completes the representative failing-test/fix fixture.
- [ ] The selected local model has exact target-hardware evidence.
- [ ] `docs/FEATURE_INVENTORY.md`, contracts, domain maps, user guide, safety,
      smoke matrix, roadmap, and history match the exact final head.
