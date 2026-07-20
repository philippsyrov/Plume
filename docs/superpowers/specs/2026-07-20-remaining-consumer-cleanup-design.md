# Remaining Consumer Cleanup — Design

**Date:** 2026-07-20
**Status:** Approved
**Base:** `codex/product-wide-ui-polish@f52de15668d0dcb53dfb5d6bbd2b3a9c9cc6f556`

## Goal

Finish the approved consumer cleanup left after the product-wide visual pass.
The result should keep one obvious home for each destination, prefer ordinary
language, and hide technical evidence until a person asks for it. This campaign
does not expand model, Browser, filesystem, or agent authority.

## Decisions

### Project opening

The Open Project surface leads with a native **Choose folder…** button and a
visible Finder drop target. **Enter path instead** reveals the existing manual
path field. Choosing, dropping, and typing produce only a candidate path; the
existing `project.open` validation and trust review remain authoritative.

The native picker is a narrow macOS AppKit command implemented with Plume's
existing `objc2-app-kit` stack. It returns one directory path or cancellation.
It does not read project contents, grant ambient filesystem access, or bypass
the existing project-open command.

The current drag/drop interpretation becomes shared behavior used by both
pre-project and modal surfaces. Drops and picker completions are ignored while
an open is busy or after the owning surface has become stale. Cancellation
leaves the surface open and changes nothing. Typed backend errors remain visible
on the opening surface.

### Navigation and settings

Workspace views contains tools only: Files, Browser, and Benchmarks. Library
stays in the permanent sidebar, and chats stay in the task/project lists.

The sidebar footer becomes one compact Settings row with icon-only Help beside
it. Close project moves to a project-specific overflow action so it does not
crowd global navigation.

Archived task and project chats leave the sidebar. Settings gains an Archived
destination that reuses the existing scoped lists, unarchive/delete behavior,
and streaming guards. Local and current-project archives remain separate.

### Chat actions

Continue and Rewind remain available because they protect the original chat
while letting a person branch or remove recent turns. Their popover rows become
compact and their wording becomes plain. Longer safety explanations move behind
an information disclosure; Rename, Archive, and Delete remain direct actions.

This does not claim work Undo/Redo. Generalized Undo/Redo remains a separate
future system with its own transaction and drift-safety design.

### Browser composer

Expanded Browser uses a compact bottom composer rather than a large floating
sheet. The real ChatPanel remains mounted so draft, selected model, pending
context, and streaming state survive hide/show. Hidden chat is removed from
keyboard navigation. Split Browser behavior is unchanged.

### Context evidence

Default chat context uses short human labels: what is attached, whether it is
ready, and whether something is blocked. Paths, byte counts, redaction counts,
source identifiers, dimensions, and previews remain available only inside
Details disclosures. Blocked context stays visibly actionable and is never
hidden as a cosmetic simplification.

### One Library

Library has one browsing destination in the sidebar. Personal and Project
Settings continue to manage stored knowledge; those are management controls,
not duplicate Library navigation.

## Explicit exclusions

- Scheduled tasks wait for a real automation contract.
- Generalized Undo/Redo waits for a reversible transaction ledger.
- No broad shell/tool execution, agent Browser control, computer-use emission,
  semantic retrieval, or cross-project memory authority is added.

## Delivery

The work is split into reviewable stacked PRs:

1. Native project chooser, Finder drop target, and manual fallback.
2. Workspace/sidebar/archive/session-menu cleanup.
3. Expanded Browser composer, context disclosure, final consistency polish.

Each PR starts with focused failing tests, runs the relevant frontend/Rust
tests, then runs `PLUME_FULL_VERIFY=1 ./scripts/verify.sh`. Native-window changes
also receive one-instance packaged-app smoke in light, dark, narrow, and
keyboard-only states. No PR is merged without explicit instruction.

