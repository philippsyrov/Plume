# Plume Handbook and End-to-End Polish Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give non-technical users a truthful Plume Handbook, connect it to Help, and finish cross-slice recovery/accessibility issues found in a complete packaged walkthrough.

**Architecture:** Write `docs/USER_GUIDE.md` from verified product behavior and sanitized packaged screenshots. Add an in-app Help surface that opens bundled/local documentation without network dependence. Treat the walkthrough as an integration test: fix reproduced defects in their owning modules, with regressions, without adding new roadmap scope.

**Tech Stack:** Markdown, React 19, Tauri bundled resources or local app route, packaged-app screenshots, Vitest, Rust/frontend regression tests.

## Global Constraints

- Handbook language targets someone who knows ChatGPT or email, not harness terminology.
- Every capability statement must match `docs/FEATURE_INVENTORY.md` and current code.
- Screenshots contain no private paths, project names, tokens, accounts, or personal content.
- Planned features are visibly separate from available behavior.
- Integration polish fixes bugs only; artifacts, automation, agent browsing, semantic memory, and Chromium remain separate future work.

---

### Task 1: Handbook structure and truth table

**Files:**
- Create: `docs/USER_GUIDE.md`
- Modify: `docs/README.md`
- Modify: `README.md`
- Modify: `scripts/check-markdown-links.ts` only if image/resource validation needs a generic extension.

**Interfaces:**
- `docs/USER_GUIDE.md` is the canonical user guide; `docs/FEATURE_INVENTORY.md` remains canonical capability status.
- Every Available-now row links to inventory evidence; Planned rows contain no shipped wording.

- [ ] Draft sections: First launch, Chat vs Projects, local models, Browser, context/Library, Continue/Rewind, project actions/patch safety, permissions, troubleshooting, Available now/Planned.
- [ ] Build a claim table against `docs/FEATURE_INVENTORY.md`; remove or qualify every unsupported sentence before screenshots.
- [ ] Use short worked examples with exact visible labels and expected outcomes.
- [ ] Link the Handbook first under a new **Use Plume** section in the docs spine.
- [ ] Run `npm run verify:docs`; confirm all links are local and valid.
- [ ] Commit: `docs: write the Plume Handbook`.

### Task 2: Sanitized screenshot fixtures

**Files:**
- Create: `docs/assets/user-guide/README.md`
- Add: sanitized PNG files under `docs/assets/user-guide/`
- Modify: `docs/USER_GUIDE.md`
- Modify: `docs/SMOKE_TESTING.md`

- [ ] Create a disposable `/private/tmp` project with generic names/content and no secrets.
- [ ] Capture packaged light/dark overview, Chat/Project distinction, Browser split/expanded, Attach evidence, Library, Continue/Rewind, and patch apply/revert.
- [ ] Inspect every image at full resolution for usernames, paths, browser accounts, menu-bar private data, and unrelated apps; recapture rather than blur when possible.
- [ ] Record capture steps and the exact build commit in the asset README.
- [ ] Add image alt text that explains the action, not decorative appearance.
- [ ] Commit: `docs: add sanitized Handbook screenshots`.

### Task 3: In-app Help entry

**Files:**
- Modify: `src/features/project-shell/UnifiedSidebar.tsx`
- Modify: `src/features/project-shell/UnifiedSidebar.test.tsx`
- Create: `src/features/help/HelpPanel.tsx`
- Create: `src/features/help/HelpPanel.test.tsx`
- Modify: `src/App.tsx`
- Modify: `src-tauri/tauri.conf.json` if the guide must be bundled as a resource.

**Interfaces:**
- `ProjectWorkspaceView` gains `help` for both shells.
- `HelpPanel` reads bundled static content only; it exposes no arbitrary file or URL opener.

```tsx
<HelpPanel onClose={onClose} handbook={bundledHandbook} />
```

- [ ] Add failing tests for a quiet Help footer action, keyboard activation, offline availability, close/back behavior, and no external browser launch.
- [ ] Run `npm run test -- src/features/help/HelpPanel.test.tsx src/features/project-shell/UnifiedSidebar.test.tsx`; expected RED is the absent Help route/panel.
- [ ] Implement a local Help panel with a concise contents list and an **Open full Handbook** action to the bundled/local guide representation.
- [ ] Keep Help read-only and available with or without a project/model.
- [ ] Re-run sidebar/App/help tests and package-resource checks.
- [ ] Commit: `feat: add offline Help`.

### Task 4: Non-technical end-to-end walkthrough

**Files:**
- Modify owning source/test/docs files only for reproduced defects.
- Modify: `docs/SMOKE_TESTING.md`

- [ ] From a fresh app-data fixture, follow the Handbook without developer docs: create Chat, open Project, choose/start model, browse, attach, inspect Library, Continue, Rewind, apply/revert a safe patch.
- [ ] Repeat after relaunch and project switch; include offline model, blocked localhost, stale page, full context shelf, missing evidence, and corrupt Browser-state recovery.
- [ ] Test keyboard-only navigation, VoiceOver names, reduced motion, light/dark, and supported minimum window size.
- [ ] For each defect, first add the smallest failing regression in its owning module, then fix and re-run affected suites.
- [ ] For every reproduced defect, record the exact failing command in the PR description; expected RED must reproduce the visible failure before the patch and GREEN must cover the owning test plus `npm run typecheck` or the focused Rust module.
- [ ] Do not rewrite unrelated architecture during polish.
- [ ] Commit each coherent regression/fix pair with `fix: polish <owning surface>`; do not batch unrelated defects into one code commit.

### Task 5: Final status audit and campaign gate

- [ ] Reconcile `README.md`, `docs/USER_GUIDE.md`, `docs/FEATURE_INVENTORY.md`, `docs/ROADMAP.md`, `docs/UI_STYLE.md`, `docs/SAFETY.md`, and smoke results.
- [ ] Confirm explicit exclusions: no agent Browser actions, automatic retrieval, graph, Scheduled automation, Chromium, host computer use, or universal artifact quality claim.
- [ ] Run focused suites, `cd src-tauri && cargo test`, `npm run typecheck`, `npm run test`, and `PLUME_FULL_VERIFY=1 ./scripts/verify.sh`.
- [ ] Build/package once more and complete the full Handbook walkthrough against the exact PR head.
- [ ] Wait for GitHub verify/gitleaks and findings-only exact-head independent review; fix only confirmed findings, re-verify, and squash-merge.
