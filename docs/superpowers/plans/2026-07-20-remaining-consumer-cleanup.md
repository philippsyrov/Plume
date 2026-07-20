# Remaining Consumer Cleanup — Implementation Plan

> Implement the approved cleanup as three reviewable PRs. Preserve the current
> trust and authority boundaries, keep one Plume instance at most during native
> QA, and use failing tests before each behavior change.

## PR 1 — Project opening

1. Add failing frontend tests for native choose, cancel, Finder drop, manual
   disclosure, busy suppression, and stale picker completion.
2. Add failing Rust tests that pin a narrow `project_choose_folder` command and
   its capability registration without weakening `project.open`.
3. Extract the shared drag/drop candidate-path hook from `OpenForm`.
4. Add the native macOS directory panel using the existing AppKit dependency;
   return `null` on cancel and a typed unsupported error off macOS.
5. Wire the chooser through a typed frontend IPC wrapper and both opening
   surfaces. Keep all candidates routed through the current open/trust flow.
6. Update domain maps, IPC contract, capability manifest, inventory evidence,
   and focused user-facing copy.
7. Run focused tests, full verification, one-instance packaged smoke, and an
   exact-head findings review; then commit, push, and open the stacked PR.

## PR 2 — Shell and archives

1. Add failing tests for the tools-only drawer and compact footer/project menu.
2. Add failing tests proving archived links leave the sidebar and appear in
   scoped Settings groups with existing behavior preserved.
3. Add failing tests for compact Continue/Rewind rows and their disclosed help.
4. Implement the navigation, Settings, and menu presentation changes without
   changing session persistence or fork/rollback contracts.
5. Update current docs and run focused tests, full verification, packaged
   keyboard/appearance smoke, and exact-head review before opening the PR.

## PR 3 — Browser, context, and consistency

1. Add failing tests for the compact expanded Browser composer, mounted-state
   preservation, and hidden keyboard exclusion.
2. Add failing tests for human-first context summaries and disclosed technical
   evidence, including visible blocked states.
3. Implement the Browser and context presentation changes using existing
   tokens, icon components, disclosures, and chat state.
4. Audit remaining type scale, spacing, focus, truncation, light/dark, and
   narrow-window behavior without adding decorative CSS or new destinations.
5. Update current docs and run focused tests, full verification, one-instance
   packaged QA, and exact-head review before opening the PR.

