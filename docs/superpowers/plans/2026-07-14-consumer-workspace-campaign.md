# Consumer Workspace Campaign Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Deliver the approved consumer workspace as five independently reviewable PRs: per-session Browser state, an integrated task Browser, a unified shell, Library, and a human Handbook.

**Architecture:** Keep chat sessions as the ownership spine. Add Browser persistence beside session rows, then bind one bounded native child-WebView workspace to the selected `{scope, sessionId}`. Build the shell and Library over existing typed APIs without weakening project trust, prompt manifests, or the rule that memory links are metadata only.

**Tech Stack:** Rust, rusqlite, Tauri 2, macOS WebKit/WKWebView, TypeScript, React 19, Vitest, Testing Library, packaged-app smoke.

## Global Constraints

- Read `AGENTS.md`, `docs/PLUME_PROJECT_SPEC.md`, and the approved design before each PR.
- Branch every slice from clean, verified `origin/main`; never stack unmerged implementation branches.
- Preserve the product boundary: **Chat answers; Projects act.**
- Browser navigation never becomes prompt context automatically.
- Keep Browser and evidence authority tied to an exact `{scope, sessionId, tabId, pageGeneration}`.
- Keep project evidence in the project store and casual-chat evidence in private app data.
- Keep memory-topic links organization metadata only; Library must not add retrieval authority.
- Do not ship Scheduled, agent browsing, Chromium, hidden retrieval, semantic memory, or computer use.
- New code files must remain below the repository's 800-line hard cap.
- Every behavior change starts with a failing focused test and ends with docs/status truth.

---

## Campaign sequence

- [ ] **PR 1 — Session Browser foundation:** execute [session Browser foundation](2026-07-14-session-browser-foundation.md).
- [ ] Wait for focused tests, full verifier, GitHub verify, gitleaks, exact-head review, and squash merge.
- [ ] **PR 2 — Integrated task Browser:** execute [integrated task Browser](2026-07-14-integrated-task-browser.md) from the new main.
- [ ] Include packaged WebKit smoke, exact-head review, and squash merge.
- [ ] **PR 3 — Unified consumer shell:** execute [unified consumer shell](2026-07-14-unified-consumer-shell.md) from the new main.
- [ ] Include light/dark and narrow/large packaged visual smoke, exact-head review, and squash merge.
- [ ] **PR 4 — Library:** execute [Library workspace](2026-07-14-library-workspace.md) from the new main.
- [ ] Include scope/backlink/drag packaged smoke, exact-head review, and squash merge.
- [ ] **PR 5 — Handbook and polish:** execute [Handbook and end-to-end polish](2026-07-14-plume-handbook-and-polish.md) from the new main.
- [ ] Run the full non-technical walkthrough and repair only genuine cross-slice defects before final review/merge.

## Gate repeated for every PR

- [ ] Run the plan's focused Rust/frontend suites.
- [ ] Run `cd src-tauri && cargo fmt --all -- --check`.
- [ ] Run `cd src-tauri && cargo test`.
- [ ] Run `PLUME_FULL_VERIFY=1 ./scripts/verify.sh` and record the exact pass/warn/fail totals.
- [ ] Update `docs/FEATURE_INVENTORY.md`, contracts, safety/UI docs, and smoke steps only for behavior actually shipped.
- [ ] Commit, push, and open one focused PR.
- [ ] Wait for GitHub verify and gitleaks on the exact head.
- [ ] Commission findings-only exact-head review; treat claims as hypotheses and fix only reproduced findings.
- [ ] Re-run affected focused tests plus the full verifier after review fixes.
- [ ] Squash-merge, delete the remote branch, fetch, and verify clean `HEAD == origin/main` before the next slice.

## Final campaign proof

- [ ] Prove migration from current schema with empty Browser state and intact transcripts.
- [ ] Prove casual/project separation, deletion, fork, rewind, relaunch, project switch, and stale callback behavior.
- [ ] Prove five-tab/20-history caps, unsafe-URL persistence, localhost approval lifetime, and evidence deletion/capacity.
- [ ] Prove split/expanded Browser, compact composer, streaming behavior, keyboard access, and reduced motion.
- [ ] Prove Library scope, search, backlinks, stale loaders, and exact click/drag context placement.
- [ ] Read `docs/USER_GUIDE.md` as a non-technical user and complete every worked example in the packaged app.
- [ ] Confirm the final inventory says what is available now and what remains planned without capability inflation.
