# Thermos Cleanup Rescue Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Finish the evidence-backed Plume cleanup campaign from exact inherited head `0e7d3412bd226391e1b8b7769f8805799c0f3d1d` without broadening product behavior or deleting retained evidence.

**Architecture:** Preserve the four inherited lifecycle/suppression commits, then make four independent cleanup commits. Behavior remains unchanged except for removing unreachable frontend-only shells; current production Settings, chat, patch, Browser, research, and model paths stay intact.

**Tech Stack:** Rust 2021/Tauri 2, React 19/TypeScript, Vitest, shell verification scripts, Markdown contracts.

**Spec:** `AGENTS.md`

## Global Constraints

- Work only in `/Users/philippsyrov/.codex/worktrees/13be/Plume`; never touch another worktree or the Desktop checkout.
- Preserve Build Week evidence, `docs/history/`, benchmark fixtures, Browser isolation proof, roadmap foundations, branches, tags, releases, and all remote state.
- No dependency or model downloads beyond the already approved `npm ci` and `cargo fetch --locked` restoration.
- Start behavior changes with a failing test. Pure deletion requires direct reachability/reference evidence plus focused verification.
- Keep the shipped/partial/scaffold/researched vocabulary exact. Do not claim a broad agent loop, arbitrary tools, autonomous Browser actions, or host control.
- Run commands through `./scripts/dev-env.sh` where they use Node or Cargo caches.
- Make local commits only. Do not push, open a PR, merge, release, rename branches/tags, or mutate remote state.

---

### Task 1: Accept the inherited Rust suppression cleanup

**Files:**
- Modify: `src-tauri/src/research/context.rs`

**Interfaces:**
- Consumes: `pack_source_summary` and `pack_synthesis`, both already `#[cfg(test)]`.
- Produces: a production build with no unused imports and unchanged test helper signatures.

- [ ] **Step 1: Reproduce the compiler warning**

Run:

```bash
./scripts/dev-env.sh bash -lc 'cd src-tauri && cargo check --all-targets'
```

Expected before the fix: exit 0 with unused imports for `RecoveryReason` and `ResearchBudget` in `research/context.rs`.

- [ ] **Step 2: Gate the test-only imports**

Keep production imports limited to live production symbols and import `RecoveryReason` / `ResearchBudget` under `#[cfg(test)]`. Do not add `allow(dead_code)` or `allow(unused_imports)`.

- [ ] **Step 3: Verify compiler and research tests**

Run:

```bash
./scripts/dev-env.sh bash -lc 'cd src-tauri && cargo check --all-targets'
./scripts/dev-env.sh bash -lc 'cd src-tauri && cargo test research::context_tests'
./scripts/dev-env.sh bash -lc 'cd src-tauri && cargo clippy --all-targets -- -D warnings'
```

Expected: all commands exit 0 with no warnings.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/research/context.rs docs/superpowers/plans/2026-08-28-thermos-cleanup-rescue.md
git commit -m "fix: finish research suppression cleanup"
```

### Task 2: Delete the unreachable frontend shells

**Files:**
- Delete: `src/features/agent/AgentWorkspace.tsx`
- Delete: `src/features/agent/AgentWorkspace.test.tsx`
- Delete: `src/features/agent/AgentDryRunPanel.tsx`
- Delete: `src/features/agent/AgentDryRunPanel.test.tsx`
- Delete: `src/styles/layout/agent-workspace.css`
- Modify: `src/styles/layout.css`
- Modify: `src/styles/layout/agent-events.css`
- Modify: directly related stale source comments only where reference searches prove they describe the deleted shell.

**Interfaces:**
- Consumes: current `App.tsx` / `UnifiedChrome.tsx` routing, where chat and the real Settings Advanced controls are reachable without either deleted component.
- Produces: no production import, stylesheet import, selector, or documentation-map pointer to the deleted components; `AgentEventLog`, `AgentSingleStepPanel`, `runHistory`, `DiffBody`, and their live styles remain.

- [ ] **Step 1: Record deletion evidence**

Run `rg` for both component names, every `plume-agent-workspace*` selector, and every `plume-agent-dryrun*` selector. Confirm production references are limited to each component's own file, the stylesheet import, its orphan CSS, stale comments, and self-tests. Confirm `UnifiedChrome.tsx` directly reaches `AgentSettingsPanel` and `AgentSingleStepPanel`.

- [ ] **Step 2: Delete only unreachable files and orphan styles**

Remove the four component/self-test files, the dead workspace stylesheet/import, and only the dry-run selector block from `agent-events.css`. Preserve live single-step/event-log/run-history/diff styles and code.

- [ ] **Step 3: Verify reachability and frontend behavior**

Run:

```bash
rg -n "AgentWorkspace|AgentDryRunPanel|plume-agent-workspace|plume-agent-dryrun" src
./scripts/dev-env.sh npm run typecheck
./scripts/dev-env.sh npm run test -- src/App.test.tsx src/features/project-shell/UnifiedChrome.test.tsx src/features/agent/AgentEventLog.test.tsx src/features/agent/AgentSingleStepPanel.test.tsx src/features/chat/DiffPreview.test.tsx
```

Expected: `rg` has no production hits; typecheck and focused tests pass. If the named DiffPreview test path does not exist, use its owning colocated test named by `src/features/README.md` without creating a new test solely for deletion.

- [ ] **Step 4: Commit**

```bash
git add -A src
git commit -m "refactor: remove retired agent shell"
```

### Task 3: Correct stale current UI documentation

**Files:**
- Modify: `docs/ARCHITECTURE.md`
- Modify: `docs/UI_STYLE.md`
- Modify: `docs/AGENT_OPERABILITY.md`
- Modify: `docs/MANUAL_TESTING.md`
- Modify: `src/features/README.md` only if deletion changes its exact owner map.
- Modify: `docs/FEATURE_INVENTORY.md` only if a current reachability/evidence path names deleted material.

**Interfaces:**
- Consumes: current `App.tsx`, `UnifiedChrome.tsx`, Settings category tests, and the exact feature inventory status vocabulary.
- Produces: current docs that describe the unified consumer shell, Settings `Advanced`, live single-step proof, and human-controlled Browser without claiming the retired center `AgentWorkspace`, left-column Agent card, or a production dry-run surface.

- [ ] **Step 1: Verify every stale statement against source**

For each edited paragraph, cite the current route/component or test in the implementer report. Do not rewrite product-history or design-history prose merely because it uses old terminology.

- [ ] **Step 2: Make narrow current-truth edits**

Replace current claims about the center `AgentWorkspace`, left-column Agent settings, and `Advanced project tools` disclosure with the actual consumer shell and Settings `Advanced` category. Keep the patch-only single-step and explicit Apply/Revert claims intact.

- [ ] **Step 3: Verify docs**

Run:

```bash
./scripts/dev-env.sh npm run verify:docs
rg -n "AgentWorkspace|Agent workspace|left-column.*Agent|Advanced project tools|production.*dry-run" docs/ARCHITECTURE.md docs/UI_STYLE.md docs/AGENT_OPERABILITY.md docs/MANUAL_TESTING.md src/features/README.md docs/FEATURE_INVENTORY.md
```

Expected: docs verification exits 0; any surviving search hit is historical/design context rather than a present-reachability claim and is listed in the report.

- [ ] **Step 4: Commit**

```bash
git add docs/ARCHITECTURE.md docs/UI_STYLE.md docs/AGENT_OPERABILITY.md docs/MANUAL_TESTING.md src/features/README.md docs/FEATURE_INVENTORY.md
git commit -m "docs: align current shell documentation"
```

### Task 4: Align the file-size policy with enforcement

**Files:**
- Modify: `AGENTS.md`
- Modify: `docs/DECOMPOSITION.md`
- Modify: `scripts/verify.sh` comments only if necessary for exact wording.
- Modify: `scripts/check-file-sizes.sh` comments only if necessary for exact wording.

**Interfaces:**
- Consumes: the existing checker, which excludes `*_test.rs`, `*_tests.rs`, `*.test.ts`, `*.test.tsx`, `tests/`, and `__tests__/`.
- Produces: one unambiguous policy: the 800-line hard gate applies to non-test Rust/TypeScript production files; standalone test files are exempt from the automated size gate but remain reviewable for clarity. Inline tests still count because they live inside production files.

- [ ] **Step 1: Record the contradiction**

Show that `docs/DECOMPOSITION.md` says every code file and standalone tests count while `scripts/check-file-sizes.sh` explicitly excludes standalone test paths and later decomposition examples call them test-exempt.

- [ ] **Step 2: Update prose, not enforcement**

Make AGENTS, DECOMPOSITION, and script comments state the actual enforced boundary above. Do not weaken the production-code gate or start splitting retained test/evidence files in this campaign.

- [ ] **Step 3: Verify the guard**

Run:

```bash
scripts/check-file-sizes.sh
./scripts/dev-env.sh npm run verify:docs
git diff --check
```

Expected: exit 0; file-size output truthfully reports production-code hard-cap and doc warnings.

- [ ] **Step 4: Commit**

```bash
git add AGENTS.md docs/DECOMPOSITION.md scripts/verify.sh scripts/check-file-sizes.sh
git commit -m "docs: align file size policy with enforcement"
```

### Task 5: Final verification and exact-head Thermos review

**Files:**
- Modify: only files required to fix a validated regression found by these gates.

**Interfaces:**
- Consumes: all commits since `origin/main` and the complete SDD ledger.
- Produces: a clean local detached HEAD with no unresolved Critical or Important finding.

- [ ] **Step 1: Run focused checks for every slice**

Re-run the task-specific Rust, frontend, docs, reference, and file-size commands on final HEAD.

- [ ] **Step 2: Run the full local completion gate**

```bash
PLUME_FULL_VERIFY=1 ./scripts/verify.sh
git diff --check
git status --short
```

Expected: verifier has zero failures. Record exact pass/warn counts and warning text. Packaged smoke is required only if the cleanup changes a reachable user-facing/native behavior; deletion of unreachable shells plus docs/comment changes does not by itself commission a package build.

- [ ] **Step 3: Run findings-only exact-head review**

Review `origin/main..HEAD` at the immutable final SHA through independent correctness and structural-quality lanes. Validate every lead against source/tests and report surviving Critical, Important, Low, and candidate items separately.

- [ ] **Step 4: Finish locally**

Do not push, merge, open a PR, release, or alter branch/tag/remote state. Report final SHA, commit list, dirty files, verification counts/warnings, surviving findings, exact deletions, and deliberately preserved material.
