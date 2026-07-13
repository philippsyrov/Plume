# Roadmap And Navigation Spine Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use
> `superpowers:subagent-driven-development` (recommended) or
> `superpowers:executing-plans` to implement this plan task-by-task. Steps use
> checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace Plume's stale chronological documentation entry points with a
verified map, ordered roadmap, evidence-backed feature inventory, and
source-honest research registry that commissions the Knowledge workspace as the
next product track.

**Architecture:** Markdown remains the human-readable source of truth. The
feature inventory embeds one machine-readable JSON block so a dependency-free
TypeScript checker can validate status vocabulary, evidence, and freshness
without parsing free-form prose. Separate link and roadmap validators keep the
verification boundaries small; `scripts/verify.sh` runs both when the existing
Node toolchain is available and warns honestly when it is not.

**Tech Stack:** Markdown, TypeScript 5.6, Node.js built-ins, Vitest, vite-node,
Bash, Git.

## Global Constraints

- `AGENTS.md` remains the authoritative workflow contract.
- MLX-LM remains Plume's local-first happy path; Ollama remains compatibility.
- Do not add Electron, Python, a Markdown parser dependency, or any default
  network call.
- Do not claim semantic retrieval, dreaming, Browser execution, computer-use
  emission, multi-agent execution, or broad tool authority is shipped.
- Memory-to-topic links remain organization metadata and do not select prompt
  context.
- Browser Phase A is first-class roadmap work and requires a zero-IPC remote
  webview boundary before execution.
- D130 remains blocked on real 128 GB M5 Max benchmark evidence.
- Speculative work receives no reused or pre-reserved D-number.
- If `node_modules` is absent, obtain explicit user permission before running
  `./scripts/dev-env.sh npm install`; do not treat skipped frontend checks as
  equivalent to passing checks.
- Each task is committed separately on draft PR #118's branch.
- Run `PLUME_FULL_VERIFY=1 ./scripts/verify.sh` before pushing the exact head.

---

### Task 0: Restore The Declared Frontend Toolchain

**Files:**

- Verify unchanged: `package.json`
- Verify unchanged: `package-lock.json`

**Interfaces:**

- Consumes: the committed lockfile through Plume's dependency-isolation wrapper.
- Produces: local `node_modules` needed for Vitest, vite-node, typecheck, and the
  packaged-app smoke build. This directory stays gitignored.

- [ ] **Step 1: Confirm explicit install permission and the clean lockfile**

```bash
git status --short
git diff -- package.json package-lock.json
```

Expected: both commands are silent before installation. Do not continue without
the user's explicit permission to install declared dependencies.

- [ ] **Step 2: Install exactly the locked dependencies inside the project**

```bash
./scripts/dev-env.sh npm install
```

Expected: exit `0`; npm cache and prefix remain under the project-managed
directories configured by `scripts/dev-env.sh`.

- [ ] **Step 3: Prove installation did not rewrite dependency truth**

```bash
git diff --exit-code -- package.json package-lock.json
./scripts/dev-env.sh npm run typecheck
./scripts/dev-env.sh npm run test
```

Expected: manifest/lockfile diff is empty, typecheck passes, and the merged-main
frontend suite passes before documentation code is added.

---

### Task 1: Create The Canonical Documentation Map And Ordered Roadmap

**Files:**

- Create: `docs/README.md`
- Create: `docs/ROADMAP.md`
- Modify: `README.md`

**Interfaces:**

- Consumes: current product contracts under `docs/` and the approved roadmap
  design.
- Produces: the two-link navigation path from root README to product,
  implementation, roadmap, research, safety, and history truth.

- [ ] **Step 1: Write `docs/README.md` as a task-oriented map**

Use these exact top-level sections and keep every destination repository-relative:

```markdown
# Plume Documentation

This is the current documentation map. Status claims live in
[FEATURE_INVENTORY.md](FEATURE_INVENTORY.md); ordered future work lives in
[ROADMAP.md](ROADMAP.md). Chronological slice history is evidence, not current
navigation.

## Understand Plume

- [Product specification](PLUME_PROJECT_SPEC.md)
- [Local-agent north star](LOCAL_AGENT_NORTH_STAR.md)
- [Architecture](ARCHITECTURE.md)
- [Safety](SAFETY.md)
- [UI style](UI_STYLE.md)

## Inspect Current Capability

- [Feature inventory](FEATURE_INVENTORY.md)
- [IPC contract](IPC_CONTRACT.md)
- [Agent runtime](AGENT_RUNTIME.md)
- [Model providers](MODEL_PROVIDERS.md)
- [Benchmark harness](BENCHMARK_HARNESS.md)

## Choose And Implement Work

- [Ordered roadmap](ROADMAP.md)
- [Development workflow](DEVELOPMENT.md)
- [Decomposition boundaries](DECOMPOSITION.md)
- [Smoke testing](SMOKE_TESTING.md)
- [Manual testing](MANUAL_TESTING.md)

## Research And History

- [Research registry](research/README.md)
- [Chronological history](history/README.md)
- [Superseded guidance](archive/README.md)
```

- [ ] **Step 2: Write `docs/ROADMAP.md` with commissioned order and status firewall**

Use these tracks in this exact order:

```markdown
# Plume Roadmap

Status vocabulary comes from [FEATURE_INVENTORY.md](FEATURE_INVENTORY.md).
Research is not implementation. Slice numbers are assigned only when a slice is
commissioned.

## Commissioned Sequence

1. Documentation and agent navigation.
2. Knowledge workspace and backlinks.
3. Explicit context shelf and drag/drop.
4. Sandboxed Browser Phase A.
5. Deeper guarded coding-agent execution.
6. Computer-use emission inside the sandbox.

The 128 GB M5 Max benchmark matrix runs when the hardware exists. The D130
launch rewrite follows measured evidence and does not block unrelated product
work.

## Track: Documentation And Agent Navigation
## Track: Project Knowledge And Second Brain
## Track: Explicit Context Placement And Linked Work
## Track: Sandboxed Browser And Evidence Capture
## Track: Safe Coding-Agent Execution
## Track: Skills, Tools, Plugins, And External Agents
## Track: Operability, Safety, Observability, And Computer Use
## Track: Local Models, Benchmarks, And Launch Readiness
```

For every track, add the five fixed labels `Outcome`, `Current floor`,
`Dependencies`, `Next deliverable`, and `Non-goals`. The Knowledge track's next
deliverable is the dedicated read-only workspace; the Browser track's next
deliverable is the capability-isolation proof, not agent clicks.

- [ ] **Step 3: Replace root README's stale slice diary with a current literal summary**

Keep the product sentence, Stack, Quick start, Repo layout, and License. Replace
only `## Status` and `## Read this first` so they point to:

```markdown
## Status

Plume is an early local-first coding editor with persisted local/project chat,
MLX-LM and compatibility-provider chat, trusted project context, safe
diff/apply/revert, project memory and curated topics, session branching,
project skills, and a reproducible benchmark evidence viewer. The bounded agent
loop, semantic retrieval, Browser execution, computer-use emission, and broad
tool execution are not shipped.

For exact evidence, see [docs/FEATURE_INVENTORY.md](docs/FEATURE_INVENTORY.md).
For ordered work, see [docs/ROADMAP.md](docs/ROADMAP.md).
```

- [ ] **Step 4: Check links and diff**

Run:

```bash
git diff --check
rg -n "D1|D17|286 cargo|Apply button.*disabled|agent loop.*not implemented" README.md
```

Expected: `git diff --check` is silent; the stale status phrases return no
matches.

- [ ] **Step 5: Commit**

```bash
git add README.md docs/README.md docs/ROADMAP.md
git commit -m "docs: add current roadmap entry points"
```

---

### Task 2: Seed The Evidence-Backed Feature Inventory

**Files:**

- Create: `docs/FEATURE_INVENTORY.md`

**Interfaces:**

- Consumes: exact merged-main implementation paths and automated tests.
- Produces: one `inventory-json` fence consumed by
  `scripts/docs/roadmap-docs.ts` in Task 6.

- [ ] **Step 1: Create the inventory contract and status definitions**

The file must define exactly these statuses:

```markdown
# Plume Feature Inventory

This is the only repository-wide implementation-status ledger. Domain docs
explain behavior; this file says whether the behavior is reachable.

- `shipped`: reachable production behavior with automated evidence.
- `partial`: useful end-to-end behavior exists, with a named missing capability.
- `scaffold`: types, pure logic, or UI shell exist without production execution.
- `researched`: adaptation is documented without shipped execution.
- `blocked`: accepted work waits on a named external dependency.
- `retired`: superseded behavior retained only for history.

Hardware evidence is independent from status. `hardware: pending` never means
the implementation is absent, and `shipped` never implies unrun hardware proof.
```

- [ ] **Step 2: Add the machine-readable inventory block**

Use a single fenced block named `inventory-json` containing a JSON array. Every
record has these exact keys:

```json
{
  "id": "memory.links",
  "track": "project-knowledge",
  "status": "shipped",
  "currentBehavior": "Users link remembered entries to validated curated topic files.",
  "missingBehavior": "Links do not select prompt context or semantic retrieval.",
  "frontendReachability": "Memory settings link editor.",
  "backendReachability": "memory.setLinks over the trusted project store.",
  "automatedEvidence": [
    "src-tauri/src/memory/memory_tests.rs",
    "src/features/memory/MemoryPanel.test.tsx"
  ],
  "manualOrHardwareEvidence": "not required",
  "dependencies": ["trusted project", "curated topic file"],
  "implementationPaths": [
    "src-tauri/src/memory/links.rs",
    "src/features/memory/MemoryPanel.tsx"
  ],
  "sourceDocuments": [
    "docs/IPC_CONTRACT.md",
    "docs/MEMORY_DISTILLATION.md"
  ],
  "nextCommissionedSlice": "Knowledge workspace backlinks",
  "lastVerifiedCommit": "5bcbf93dc2e948418b2360d1dd5a591f088243f5",
  "lastVerifiedDate": "2026-07-13"
}
```

Add complete records for these ids, using direct implementation/test paths and
the exact merged-main commit above:

```text
chat.streaming
sessions.persistence
sessions.branching
project.trust-and-context
context.exact-manifest
patch.safe-lifecycle
memory.entries
memory.topics
memory.links
memory.distillation
memory.semantic-retrieval
skills.project-library
skills.session-promotion
agent.single-step
agent.bounded-loop
tools.catalog
providers.mlx-managed
benchmarks.evidence
knowledge.workspace
browser.workspace
computer.external-operability
computer.emitting-sandbox
computer.host-control
```

Required status decisions:

```text
shipped: chat.streaming, sessions.persistence, sessions.branching,
         project.trust-and-context, context.exact-manifest,
         patch.safe-lifecycle, memory.entries, memory.topics, memory.links,
         memory.distillation, skills.project-library,
         skills.session-promotion, providers.mlx-managed,
         benchmarks.evidence, computer.external-operability
partial: agent.single-step
scaffold: agent.bounded-loop, tools.catalog, browser.workspace
researched: memory.semantic-retrieval, knowledge.workspace,
            computer.emitting-sandbox, computer.host-control
```

For `memory.semantic-retrieval`, `knowledge.workspace`,
`computer.emitting-sandbox`, and `computer.host-control`, use empty
`implementationPaths` and name the relevant research/source docs. For
`browser.workspace`, name the disabled drawer and optional catalog descriptors
as the current scaffold while stating that no webview or executor exists.

- [ ] **Step 3: Add a human summary table generated from the same decisions**

Above the JSON block, add a compact table containing `Feature`, `Status`,
`Current floor`, and `Next honest step`. Do not introduce a status or claim not
present in the JSON record.

- [ ] **Step 4: Verify every shipped record has direct evidence**

Run:

```bash
rg -n '"status": "shipped"|"automatedEvidence"|"implementationPaths"' docs/FEATURE_INVENTORY.md
git diff --check
```

Expected: each shipped record is followed by non-empty evidence and
implementation arrays; diff check is silent.

- [ ] **Step 5: Commit**

```bash
git add docs/FEATURE_INVENTORY.md
git commit -m "docs: seed evidence-backed feature inventory"
```

---

### Task 3: Add The Research, History, And Archive Registries

**Files:**

- Create: `docs/research/README.md`
- Create: `docs/research/codex-zcode.md`
- Create: `docs/research/qoder-notion.md`
- Create: `docs/research/hermes.md`
- Create: `docs/research/memory-systems.md`
- Create: `docs/research/rust-agents.md`
- Create: `docs/research/local-inference.md`
- Create: `docs/history/README.md`
- Create: `docs/archive/README.md`

**Interfaces:**

- Consumes: existing research documents and dated direct observations.
- Produces: dated `research-metadata` JSON fences consumed by Task 6.

- [ ] **Step 1: Create the registry index and hygiene vocabulary**

`docs/research/README.md` must define:

```markdown
# Plume Research Registry

Research records behavior and evidence; it does not mark a feature shipped.

## Hygiene Levels

1. `official-public`
2. `local-observation`
3. `clean-room-reference`
4. `behavior-report-only`
5. `do-not-use-source`

## Families

- [Codex desktop and ZCode](codex-zcode.md)
- [Qoder and Notion](qoder-notion.md)
- [Hermes Agent](hermes.md)
- [Sass and memory systems](memory-systems.md)
- [Rust agent references](rust-agents.md)
- [Local inference](local-inference.md)
```

- [ ] **Step 2: Add exact metadata to every research note**

Every note starts with one JSON fence using this exact schema:

```research-metadata
{
  "family": "codex-zcode",
  "sourceDate": "2026-07-13",
  "hygiene": "local-observation",
  "sources": ["https://zcode.z.ai/en"],
  "refreshTrigger": "Meaningful upstream product or public API release"
}
```

Allowed `hygiene` values are the five registry values. Each note then uses the
headings `Observed behavior`, `Plume adaptation`, `Already shipped overlap`,
`Remaining gap`, and `Rejected or deferred`.

`codex-zcode.md` must distinguish official Codex capability claims from dated
ZCode visual observation and forbid copying branding/assets/Electron code.
`qoder-notion.md` must specify explicit drag/drop provenance rather than hidden
retrieval. `memory-systems.md` must say current links are metadata only and
dreaming is later opt-in work. `local-inference.md` must preserve MLX-first and
the evidence gate on D130.

- [ ] **Step 3: Add history and archive entry points**

Use:

```markdown
# Plume History

Chronological slice records are implementation evidence, not current roadmap
ordering. The ledger remains in `AGENTS.md` until the dedicated history-cleanup
rollout moves it without loss. Current status lives in
[../FEATURE_INVENTORY.md](../FEATURE_INVENTORY.md).
```

and:

```markdown
# Plume Archive

This directory holds superseded design guidance. Every archived document must
contain a line beginning `Replacement:` with either a current relative link or
`none` plus a reason. No current document may silently treat archive content as
active guidance.
```

- [ ] **Step 4: Verify metadata and links manually**

Run:

```bash
rg -L 'research-metadata' docs/research/*.md
rg -L 'sourceDate' docs/research/*.md
rg -L 'refreshTrigger' docs/research/*.md
git diff --check
```

Expected: only `docs/research/README.md` may be listed by the first three
commands; diff check is silent.

- [ ] **Step 5: Commit**

```bash
git add docs/research docs/history docs/archive
git commit -m "docs: add source-honest research registry"
```

---

### Task 4: Build The Markdown-Link Checker With TDD

**Files:**

- Create: `scripts/docs/markdown-links.ts`
- Create: `scripts/docs/markdown-links.test.ts`
- Create: `scripts/check-markdown-links.ts`

**Interfaces:**

- Consumes: repository root plus tracked Markdown paths.
- Produces: `LinkIssue[]` from pure validation and CLI exit `0`/`1`.

- [ ] **Step 1: Write failing unit tests**

Cover these exact cases in `scripts/docs/markdown-links.test.ts`:

```typescript
// @vitest-environment node

import { mkdtempSync, mkdirSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';

import { checkMarkdownLinks } from './markdown-links.ts';

describe('checkMarkdownLinks', () => {
  it('accepts relative files and GitHub-style heading anchors', () => {
    const root = mkdtempSync(join(tmpdir(), 'plume-links-'));
    mkdirSync(join(root, 'docs'));
    writeFileSync(join(root, 'README.md'), '[Safety](docs/SAFETY.md#hard-links)');
    writeFileSync(join(root, 'docs/SAFETY.md'), '# Safety\n\n## Hard links\n');
    expect(checkMarkdownLinks(root, ['README.md', 'docs/SAFETY.md'])).toEqual([]);
  });

  it('reports missing files and missing anchors with the source path', () => {
    const root = mkdtempSync(join(tmpdir(), 'plume-links-'));
    writeFileSync(join(root, 'README.md'), '[Nope](docs/NOPE.md) [Bad](#missing)');
    expect(checkMarkdownLinks(root, ['README.md']).map((issue) => issue.kind)).toEqual([
      'missingFile',
      'missingAnchor',
    ]);
  });

  it('rejects repository escapes and ignores external URLs and fenced code', () => {
    const root = mkdtempSync(join(tmpdir(), 'plume-links-'));
    writeFileSync(
      join(root, 'README.md'),
      '[escape](../secret.md) [web](https://example.com)\n```md\n[x](missing.md)\n```',
    );
    expect(checkMarkdownLinks(root, ['README.md'])).toMatchObject([
      { kind: 'pathEscape', source: 'README.md' },
    ]);
  });
});
```

- [ ] **Step 2: Run the tests and confirm the expected failure**

Run:

```bash
./scripts/dev-env.sh npx --no-install vitest run scripts/docs/markdown-links.test.ts
```

Expected: FAIL because `markdown-links.ts` does not exist.

- [ ] **Step 3: Implement the pure checker**

Export these exact interfaces:

```typescript
export type LinkIssueKind = 'missingFile' | 'missingAnchor' | 'pathEscape';

export type LinkIssue = {
  source: string;
  target: string;
  kind: LinkIssueKind;
  message: string;
};

export function checkMarkdownLinks(root: string, relativeFiles: string[]): LinkIssue[];
```

Implementation rules:

```text
strip fenced code blocks before link extraction
ignore http:, https:, mailto:, data:, and empty targets
resolve relative paths from the source document
allow same-document #anchors
reject any resolved path outside root
validate files with lstatSync(...).isFile()
derive duplicate-aware GitHub-style heading slugs
return issues sorted by source, target, kind
```

- [ ] **Step 4: Implement the CLI**

`scripts/check-markdown-links.ts` uses
`execFileSync('git', ['ls-files', '*.md'])`, calls the pure checker, prints
`<source>: <message>` per issue, and exits `1` when any issue exists. It must not
scan `.git`, `node_modules`, `src-tauri/target`, `.cargo-home`, `.cache`, or
`benchmark-artifacts` because only tracked Markdown is passed in.

- [ ] **Step 5: Run focused tests**

```bash
./scripts/dev-env.sh npx --no-install vitest run scripts/docs/markdown-links.test.ts
```

Expected: all tests pass.

- [ ] **Step 6: Commit**

```bash
git add scripts/docs/markdown-links.ts scripts/docs/markdown-links.test.ts scripts/check-markdown-links.ts
git commit -m "test: verify internal markdown links"
```

---

### Task 5: Build The Roadmap-Truth Checker With TDD

**Files:**

- Create: `scripts/docs/roadmap-docs.ts`
- Create: `scripts/docs/roadmap-docs.test.ts`
- Create: `scripts/check-roadmap-docs.ts`

**Interfaces:**

- Consumes: `FEATURE_INVENTORY.md`, research notes, archive notes, and a
  replaceable Git runner.
- Produces: hard `errors` plus warn-only `warnings` for stale inventory paths.

- [ ] **Step 1: Write failing tests for schema and status honesty**

Define fixtures proving:

```text
unknown inventory status -> error
shipped record with empty automatedEvidence -> error
researched record with empty implementationPaths -> accepted
research note without sourceDate or hygiene -> error
archive note without Replacement: -> error
valid lastVerifiedCommit with unchanged paths -> no warning
owned path changed since lastVerifiedCommit -> warning naming id and path
non-ancestor lastVerifiedCommit -> warning without attempting a path diff
```

Use this result interface in test expectations:

```typescript
export type DocsCheckResult = {
  errors: string[];
  warnings: string[];
};
```

- [ ] **Step 2: Run the tests and confirm the expected failure**

```bash
./scripts/dev-env.sh npx --no-install vitest run scripts/docs/roadmap-docs.test.ts
```

Expected: FAIL because `roadmap-docs.ts` does not exist.

- [ ] **Step 3: Implement inventory and research parsing**

Export:

```typescript
export const FEATURE_STATUSES = [
  'shipped',
  'partial',
  'scaffold',
  'researched',
  'blocked',
  'retired',
] as const;

export function checkRoadmapDocs(options: {
  root: string;
  git: (args: string[]) => { ok: boolean; stdout: string };
}): DocsCheckResult;
```

Parse the single `inventory-json` fence as JSON and validate every required key
from Task 2. Parse every `docs/research/*.md` file except `README.md` for one
`research-metadata` fence. Scan `docs/archive/*.md` except `README.md` for a
line beginning `Replacement:`.

- [ ] **Step 4: Implement freshness warnings**

For each `shipped`, `partial`, or `scaffold` record:

```bash
git merge-base --is-ancestor "$last_verified_commit" HEAD
git diff --name-only "$last_verified_commit..HEAD" -- "${implementation_paths[@]}"
```

Missing/non-ancestor commits and changed owned paths are warnings, not errors.
Never run the path diff after the ancestor check fails.

- [ ] **Step 5: Implement the CLI and focused tests**

`scripts/check-roadmap-docs.ts` prints `error:` and `warning:` lines, exits `1`
only for errors, and exits `0` for warnings alone.

Run:

```bash
./scripts/dev-env.sh npx --no-install vitest run scripts/docs/roadmap-docs.test.ts
```

Expected: all tests pass.

- [ ] **Step 6: Commit**

```bash
git add scripts/docs/roadmap-docs.ts scripts/docs/roadmap-docs.test.ts scripts/check-roadmap-docs.ts
git commit -m "test: enforce roadmap status honesty"
```

---

### Task 6: Wire Documentation Verification And Repair Existing Links

**Files:**

- Modify: `scripts/verify.sh`
- Modify: `package.json`
- Modify: tracked Markdown files reported by the new checker

**Interfaces:**

- Consumes: both TypeScript CLIs from Tasks 4 and 5.
- Produces: two explicit verifier gates and one `npm run verify:docs` command.

- [ ] **Step 1: Add the package script**

Add exactly:

```json
"verify:docs": "vite-node scripts/check-markdown-links.ts && vite-node scripts/check-roadmap-docs.ts"
```

- [ ] **Step 2: Add required structure entries**

Append these to `REQUIRED_FILES` in `scripts/verify.sh`:

```bash
"docs/README.md"
"docs/ROADMAP.md"
"docs/FEATURE_INVENTORY.md"
"docs/research/README.md"
"docs/history/README.md"
"docs/archive/README.md"
"scripts/check-markdown-links.ts"
"scripts/check-roadmap-docs.ts"
```

- [ ] **Step 3: Add a Documentation section before Frontend**

Use the existing `ok`, `warn`, and `fail` helpers:

```bash
section "Documentation"

if ! command -v node >/dev/null 2>&1; then
  warn "node not installed — skipping documentation checks"
elif [ ! -d "node_modules" ]; then
  warn "node_modules missing — skipping documentation checks"
else
  if npx --no-install vite-node scripts/check-markdown-links.ts; then
    ok "Internal Markdown links clean"
  else
    fail "Internal Markdown link check failed"
  fi
  if npx --no-install vite-node scripts/check-roadmap-docs.ts; then
    ok "Roadmap status and research metadata clean"
  else
    fail "Roadmap documentation check failed"
  fi
fi
```

- [ ] **Step 4: Run the link checker and repair every genuine failure**

```bash
./scripts/dev-env.sh npm run verify:docs
```

Expected first run: existing stale links may fail. Repair the source Markdown
link or heading; do not weaken the checker or add path-specific exemptions.
Repeat until exit `0`.

- [ ] **Step 5: Run the relevant test group and typecheck**

```bash
./scripts/dev-env.sh npx --no-install vitest run scripts/docs scripts/verify-diagnostics.test.ts
./scripts/dev-env.sh npm run typecheck
```

Expected: all tests pass and TypeScript exits `0`.

- [ ] **Step 6: Commit**

```bash
git add package.json scripts/verify.sh README.md docs scripts
git commit -m "ci: verify roadmap documentation"
```

---

### Task 7: Verify The Navigation Spine As One Exact Head

**Files:**

- Modify only if verification finds a genuine defect.

**Interfaces:**

- Consumes: complete R1 branch state.
- Produces: exact-head evidence suitable for moving PR #118 out of draft.

- [ ] **Step 1: Run focused documentation verification**

```bash
./scripts/dev-env.sh npm run verify:docs
./scripts/dev-env.sh npx --no-install vitest run scripts/docs scripts/verify-diagnostics.test.ts
```

Expected: all commands exit `0`.

- [ ] **Step 2: Run the full verifier**

```bash
PLUME_FULL_VERIFY=1 ./scripts/verify.sh
```

Expected: zero failures. Existing documentation soft-cap warnings are allowed;
new warnings introduced by this rollout are not.

- [ ] **Step 3: Check repository and history integrity**

```bash
git diff --check origin/main...HEAD
git status --short
git log --oneline origin/main..HEAD
```

Expected: diff check is silent, status is clean, and history contains the
design refresh plus the focused R1 commits.

- [ ] **Step 4: Push with an exact lease and update PR #118**

```bash
expected_remote_head="$(git rev-parse origin/codex/roadmap-navigation-design)"
git push origin HEAD:codex/roadmap-navigation-design \
  --force-with-lease="codex/roadmap-navigation-design:$expected_remote_head"
gh pr view 118 --json headRefOid,isDraft,mergeable,statusCheckRollup
```

Expected: `headRefOid` equals local `HEAD`; GitHub verify and gitleaks complete
successfully before merge readiness is claimed.

- [ ] **Step 5: Hand off for exact-head review**

Report the exact SHA, changed docs/checkers, focused test counts, full verifier
summary, GitHub checks, and any remaining pre-existing warnings. Do not begin the
Knowledge workspace implementation on top of an unmerged documentation branch.
