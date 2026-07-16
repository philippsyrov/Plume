# Plume Consumer Shell Corrections — Design

**Date:** 2026-07-15
**Status:** Approved direction; post-merge audit disposition recorded
**Base:** `origin/main@337f7de54bcfb53f7d7b2a71423657056bb0ab0e`

## Problem

The consumer-workspace campaign made Browser, Library, Settings, Help, and
session history usable, but the final packaged walkthrough exposed navigation
duplication and several controls that still describe implementation mechanics
instead of ordinary user intent.

The visible problems are:

- Library appears in both the permanent sidebar and Workspace views.
- Workspace views includes Chat even though the active task in the sidebar is
  already the stable way back to its conversation.
- expanded Browser shows a large chat sheet when the user only needs a compact
  composer.
- Settings and Help consume two full-width footer rows.
- Open project leads with a pasted filesystem path instead of a native folder
  choice or drop target.
- chat action menus are oversized and explain branching mechanics inline.
- archived chats appear in the task list instead of account/app management.
- `Rewind into new chat` sounds like Undo even though it only creates a safe
  conversation branch and restores no work.

## Product Principles

1. Keep one obvious home for each destination.
2. Keep task tools separate from app navigation.
3. Prefer ordinary intent labels over database or harness vocabulary.
4. Preserve mounted state when hiding a surface.
5. Never call conversation branching “Undo.”
6. Undo/Redo may claim only mutations Plume recorded and can safely reverse.
7. Preserve current trust, provenance, project/local separation, and Browser
   ownership boundaries.

## Decisions

### One Library

Library remains a first-class item in the permanent left sidebar. It is removed
from Workspace views.

Settings may continue to contain Library **editing** controls for About you and
project memory. That is not a second Library destination: the sidebar is where
people browse and use knowledge; Settings is where they manage stored data.

### Workspace Views Contains Tools Only

Workspace views contains:

- Files — requires an open project.
- Browser — available per saved task.
- Benchmarks — requires an open project.
- Terminal — visibly unavailable until shipped.

Library and Chat are removed. Closing Workspace views only closes the picker.
Selecting the active task in the left sidebar returns to its chat. Browser keeps
its existing explicit Return to split view control.

### Expanded Browser Composer

Expanded Browser continues to use the full available canvas. Its chat affordance
becomes one compact floating composer capsule near the bottom edge:

- no full-width white sheet or empty container around it;
- the real `ChatPanel` remains mounted so drafts, pending context, and action
  state survive hide/show;
- Show chat reveals the capsule;
- Hide chat removes it visually and from keyboard navigation;
- reduced-motion behavior remains respected;
- model-unavailable and streaming states use the same logic as normal chat.

This correction changes presentation only. It does not create a second chat or
new sending path.

### Footer Controls

The sidebar footer is a single compact row:

- Settings stays labelled and occupies the available width.
- Help becomes an icon-only question-mark button immediately to its right.
- both controls retain accessible names, keyboard focus, and tooltips.
- Close project remains a project-specific action and must not crowd this row;
  its final placement may stay in project controls until a later project menu is
  designed.

### Open Project

The primary project-opening flow offers two equal human-friendly inputs:

1. **Choose folder…** opens the native macOS folder selector.
2. A visible drop target accepts a folder dragged from Finder.

An **Enter path instead** disclosure keeps manual path entry available for
advanced users, copied terminal paths, and accessibility fallbacks.

All three inputs produce only a candidate local path. They still pass through
the existing `project.open` validation and explicit project trust review. The
folder selector grants no broader filesystem authority, and dropped non-folder
paths fail through the same typed backend validation.

The existing pre-project drag/drop listener should be reused rather than
creating a second event interpretation. The modal must add the same busy and
stale-request guards.

### Compact Chat Menu

The sidebar chat menu contains compact, single-line actions:

- Rename
- Archive
- Delete

Long explanatory paragraphs leave the popover. Accessible labels and the
Handbook retain the safety explanation. Delete remains visually destructive and
keeps its confirmation dialog.

`Continue in new chat` and `Rewind into new chat` leave this menu.

### Archived Chats Move To Settings

Archived-chat entry points leave the sidebar. Settings gains a Chats section
with an Archived destination. It presents local and current-project archives as
separate groups and reuses the existing unarchive/delete behavior and streaming
guards.

This is the first Settings navigation section, not permission to imitate every
Codex settings page. Appearance, Providers, Local models, Library management,
and Archived chats remain the only shipped Plume sections.

### Forking Becomes “Try Another Direction”

The persisted fork capability remains because it supports alternative answers,
model comparisons, and risky experiments without destroying the original.

It is not exposed in the sidebar menu in this correction. A later turn-level
design may expose:

> **Try another direction**
> Starts a new chat from here. This chat stays unchanged.

Internally that action may keep exact fork provenance. Consumer UI does not need
the word `fork` unless advanced details are open.

### Work Undo/Redo Is A Separate System

Plume will not replace Rewind with a misleading chat-only Undo.

A future tracked-work milestone should provide editor-style Undo/Redo:

- every reversible Plume mutation is recorded as one task-owned transaction;
- a transaction stores enough before/after evidence to reverse or reapply it;
- multi-file or single-artifact work is atomic from the user’s perspective;
- `Command-Z` undoes the latest eligible transaction;
- `Command-Shift-Z` redoes the latest undone transaction;
- a new mutation after Undo clears the redo branch;
- drift detection refuses to overwrite work changed after the checkpoint;
- irreversible external actions never advertise Undo;
- a later activity history may offer Restore to this point by reversing several
  tracked transactions in order.

The existing patch checkpoint and drift-detecting revert path are useful
foundations, but this document does not commission a generalized action ledger.
Slides, documents, Browser actions, messages, and future computer-use actions
need kind-specific reversibility contracts. That milestone requires its own
design and safety review before implementation.

## State And Failure Behavior

- Removing drawer destinations does not remove their routes or persisted data.
- Hiding the Browser composer must never unmount the live chat hook.
- Settings overlays continue waiting for Browser suspension acknowledgement.
- A cancelled native folder picker leaves the modal open and changes nothing.
- A folder-picker, drop, or manual-path failure stays in the modal with a plain
  error; no trust entry is created.
- Archived lists preserve local/project scope and never aggregate memories,
  sessions, or project authority.
- Existing continue/fork/rewind backend data remains loadable for backward
  compatibility even when the old controls leave the consumer UI.

## Accessibility

- The icon-only Help control has `aria-label="Help"` and a visible focus ring.
- The folder drop target has a keyboard-equivalent Choose folder button and
  descriptive text; drag/drop is never the only path.
- Workspace views preserves focus trapping, Escape close, and focus return.
- Hidden Browser chat is both visually hidden and removed from the tab order.
- Compact menus retain arrow-key navigation, Home/End, Escape, outside-click
  close, and focus restoration.
- Settings navigation communicates the current section with `aria-current`.

## Implementation Sequence

### PR 0 — Browser suspension and overlay recovery

Small reliability gate before visual changes:

- recover when the native Browser activates but its first suspension sync
  fails, instead of leaving the runtime permanently unready;
- give requested HTML overlays a deterministic fallback when native suspension
  acknowledgement never arrives;
- normalize an over-wide restored Browser split width after the container is
  measured, avoiding a permanently stale descriptor and oversized first paint;
- generation-guard capture failures so an obsolete request cannot replace the
  current Browser error state;
- add focused regressions for mount-time failure, missing suspension
  acknowledgement, recovery, and restored-width normalization.

This PR changes recovery behavior, not Browser authority or navigation policy.
It must fail closed: Plume does not paint an HTML dialog underneath an active
native webview and pretend it is usable.

### PR A — Consumer shell correction

Frontend-focused:

- remove Library and Chat from Workspace views;
- compact the expanded Browser composer;
- combine Settings and icon-only Help in the footer;
- shrink the session menu and remove Continue/Rewind controls;
- move archived-chat access into Settings;
- prevent Cmd+K from opening Search underneath another modal and restore focus
  when Search closes;
- apply the selected appearance before first paint and cover idle/open-project
  surfaces as well as the project shell;
- update the Handbook and UI/status documentation.

### PR B — Native project chooser and drop target

Small Tauri/frontend boundary:

- add the reviewed native directory-picker capability;
- reuse folder-drop interpretation in the open-project modal;
- keep manual path entry behind Enter path instead;
- verify trust, cancellation, non-directory, and stale-request behavior.

### Later design — Tracked work Undo/Redo

Do not bundle this with PR A or PR B. First audit existing patch checkpoints and
define the transaction ledger, artifact size/cap policy, redo invalidation,
crash recovery, project ownership, and reversible action kinds.

## Verification

Each implementation PR requires:

- focused tests written before behavior changes;
- TypeScript and relevant Rust tests;
- `PLUME_FULL_VERIFY=1 ./scripts/verify.sh`;
- exact-head GitHub verify and gitleaks;
- packaged macOS smoke for the surfaces it changes;
- visual comparison against the user-provided Codex and Plume screenshots;
- findings-only independent exact-head review.

PR A smoke must cover drawer contents, sidebar return-to-chat, composer draft
survival, Help focus/tooltip, compact menu keyboard behavior, and local/project
archives in Settings. PR B smoke must cover native choose, cancel, Finder drop,
manual path fallback, trust review, and rejection of a non-directory input.

## Audit Gate

Claude/Fable audited integrated
`main@337f7de54bcfb53f7d7b2a71423657056bb0ab0e`; Codex independently verified
the reported mechanics against the same tree. There were no Critical or
Important findings and no trust, scope, persistence, or capability-honesty
failure.

Disposition:

- the two Medium findings are one Browser readiness/overlay-recovery boundary
  and become PR 0 above;
- the Cmd+K modal collision and incomplete dark appearance belong in PR A;
- Browser split-width normalization and stale capture-error guards are cheap
  Browser hardening and belong in PR 0;
- the older `browser_sandbox_*` IPC family stays as the deliberate capability-
  isolation proof documented by the Phase A design; production task browsing
  uses `task_browser_*`. Add an explicit code comment instead of deleting the
  proof surface casually;
- pre-trust marker-file and package-manager existence metadata remains an
  explicit, low-sensitivity product decision because the user selected the
  folder and file contents, git state, and project capabilities remain blocked
  until trust. Record the boundary in Safety documentation;
- Library filename-only topic search, dead helper exports, and projection
  memoization are cleanup candidates for the Library polish pass. They do not
  block these shell corrections.

Unrelated roadmap work does not enter these PRs.

## Explicit Non-Goals

- no silent project access;
- no automatic memory retrieval or prompt authority from links;
- no generalized agent/computer-use authority;
- no claim that arbitrary external actions can be undone;
- no full Codex Settings clone;
- no deletion or migration of historical fork/rewind metadata;
- no new roadmap slice number invented by this design.
