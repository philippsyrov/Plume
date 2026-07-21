# Final Demo UI Cleanup Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Remove the screenshot-identified demo clutter, repair Browser overlay/layout behavior, and package the supplied Plume icon.

**Architecture:** Keep authority and state paths unchanged. Make small presentation changes in the owning React components and existing layout styles, then regenerate the static Tauri icon set from the supplied raster source.

**Tech Stack:** React 19, TypeScript, Vitest, CSS, Tauri 2 icon assets.

## Global Constraints

- Browser evidence remains an opaque Rust-resolved reference.
- Browser remains human-controlled; Plume emits no computer actions.
- Qwen2-VL remains ordinary screenshot chat; Qwen Coder remains the strict research/export demo path.
- No downloads, new dependencies, external Finder image import, or extra idle UI.
- Exactly one packaged Plume instance may remain open during visual QA.

---

### Task 1: Compact chat-native evidence and copy

**Files:**
- Modify: `src/features/chat/ChatEntryRow.tsx`
- Modify: `src/features/chat/ContextShelf.tsx`
- Modify: `src/features/chat/disabledReason.ts`
- Test: colocated chat tests
- Modify: `src/styles/layout/chat.css`

- [ ] Write focused tests that reject visible role labels, long evidence labels,
  paragraph markers, and model-id placeholder copy.
- [ ] Run the focused tests and confirm they fail for those exact strings.
- [ ] Implement `Website`, `Screenshot`, and `Message Plume` visible copy while
  preserving accessible role/provenance semantics.
- [ ] Run the focused tests and TypeScript typecheck.

### Task 2: Browser overlay and joined split layout

**Files:**
- Modify: `src/features/browser/BrowserPanel.tsx`
- Modify: `src/features/browser/BrowserPanel.test.tsx`
- Modify: `src/styles/layout/browser.css`
- Modify only the owning shell layout stylesheet if chat breathing room cannot
  be expressed in `browser.css`.

- [ ] Write a focused structural/style test proving the open Attach menu does
  not add a Browser grid row and is anchored to the toolbar.
- [ ] Run the focused test and confirm it fails against the current chrome
  stack row.
- [ ] Move the menu to an absolute anchored overlay, preserve keyboard/outside
  dismissal, narrow the resize gutter visually, and add restrained chat inset.
- [ ] Run Browser tests and TypeScript typecheck.

### Task 3: Remove redundant model-workspace chrome and package the icon

**Files:**
- Modify: `src/features/model-picker/ModelChooser.tsx`
- Modify: `src/features/model-picker/ModelChooser.test.tsx`
- Replace: `src-tauri/icons/*`

- [ ] Write a focused test proving the model region remains accessible without
  the visible introduction/back band.
- [ ] Run the test and confirm the old text makes it fail.
- [ ] Remove only the redundant visible band.
- [ ] Copy the user-supplied raster into the isolated worktree and regenerate
  the existing Tauri icon outputs with project-local tooling.
- [ ] Run model tests and verify icon dimensions/formats.

### Task 4: Integrated verification and visual QA

**Files:**
- Create/update: `design-qa.md`
- Update current behavior docs only if visible copy/ownership changed.

- [ ] Run focused tests, `npm run typecheck`, and `npm run verify:docs`.
- [ ] Run `PLUME_FULL_VERIFY=1 ./scripts/verify.sh` outside the localhost
  sandbox; expect 53 checks, zero failures, and only documented soft-cap warnings.
- [ ] Rebuild the release app so the latest runtime policy and icon are inside
  the bundle; verify deep strict code signing.
- [ ] Open exactly one release instance and capture the model, chat/evidence,
  split Browser, Attach menu, and icon states at matching viewports.
- [ ] Compare supplied and implementation captures together, fix every P0-P2,
  and record `final result: passed` in `design-qa.md`.
- [ ] Commit, push PR #173, wait for Verify and Gitleaks, then squash-merge as
  explicitly authorized by the user.

