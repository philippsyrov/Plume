# Calm Plume Consumer UI Cleanup

**Date:** 2026-07-18

**Status:** Approved design, awaiting written-spec review

**Base:** `origin/main@eb8024df0980fd34d45758f71acc217da80e6ac2`

## Goal

Make the first-run chat, model chooser, transcript, and projectless Library feel
calm and obvious without changing Plume's authority model, provider behavior,
memory boundaries, or hand-drawn coding-cafe identity.

The cleanup removes repeated copy and unnecessary frames. It does not add a
new design system, a new navigation model, or a new product feature.

## Design direction

Keep the warm paper, ink, restrained pencil shading, serif prose, and existing
accent semantics. Use hierarchy and whitespace instead of drawing a border
around every group. The macOS window remains the outer frame; internal
hairlines separate major regions, while small controls and true interactive
surfaces keep the existing imperfect outline treatment.

The result should read as one quiet workspace:

- one clear next action when no model is selected;
- one compact model list instead of cards inside a popover frame;
- one readable conversation column with secondary metadata kept quiet;
- one useful Library overview instead of a large empty status canvas.

## Scope

### 1. Empty chat and composer

The empty state keeps the question **What can I help you with?** and a single
short supporting line. When no model is selected, the centered **Choose a
model** button is the only primary model action.

The disabled composer remains visible so the layout does not jump after model
selection, but it stops repeating the same state in both its placeholder and a
separate status line. Its placeholder should say **Choose a model to start**;
the separate **No model selected** sentence is removed. The disabled Send
button and its accessible name remain.

Project chat must preserve the honest context copy: bounded ambient trusted-
project memory/topics may be included, while sources the user adds are pinned
exactly. Local chat must not imply project context.

### 2. Model chooser

Keep the top-bar trigger's stable accessible name **Model**, current selection
value, `aria-expanded`, and dialog relationship.

Inside the chooser, render Apple On-Device and Qwen Coder 1.5B as compact rows:

- name and one plain-language suitability line on the left;
- one state-aware action or **Selected** state on the right;
- download, verification, startup, and failure status directly below the
  affected row only;
- source, license, and diagnostic errors remain under **Details**.

Remove the large outlined card around each provider. Keep one restrained
popover boundary, one divider between rows, and a maximum width that does not
cover most of the conversation.

The dialog must contain keyboard focus while open, close with Escape or an
outside pointer press, and return focus to the trigger. Selecting a usable
model closes the chooser as it does today. Downloading and retry states remain
open so progress and errors stay visible.

### 3. Conversation transcript

Keep user and assistant messages visually distinct without giving every turn a
large competing card. Use a comfortable readable measure and tighter vertical
rhythm. Accessible message labels remain **user message** and **assistant
message**; visible role labels become quiet **You** and **Plume** labels rather
than loud all-caps decoration.

Assistant runtime metadata such as model name, duration, and token speed stays
available but visually secondary. Exact context-manifest chips, attachment
provenance, diff previews, cancellation state, errors, copy controls, and
streaming state retain their existing semantics and ordering.

The existing **Clear chat transcript** action remains available only when the
transcript has entries and remains disabled while streaming. It becomes a
quiet secondary control aligned with the conversation header rather than a
floating visual focal point.

### 4. Library overview

Keep the existing source-tree and reading-canvas architecture. Do not turn
Library into a dashboard or graph.

The projectless overview becomes two calm summary rows inside the reading
canvas:

- **About you** shows the app-private memory count, its Mac-local boundary,
  and an ordinary action that opens that source;
- **This project** explains that project memory and topics require an open,
  trusted project, with an ordinary **Open project** action routed through the
  existing shell callback when available.

When a trusted project is open, the second row shows project-memory and topic
counts with separate actions for **This project** and **Topics**. Those actions
select the existing Library sources without changing storage or retrieval
behavior. Connections remain organization metadata only and do not gain
automatic retrieval authority.

The header keeps **Refresh Library**, but the control is visually secondary.
Disabled source rows remain truthful and accessible; unavailable project data
must never look like empty user memory.

## Component boundaries

- `ModelChooser` continues to own the trigger, popover lifecycle, catalog
  states, and selection actions. A small internal row component may replace
  the duplicated card layout.
- `ChatPanel` continues to own composer state and transcript controls.
  `ChatEntryRow` continues to own per-turn rendering and metadata.
- `LibraryPanel` continues to own source selection and project-generation
  fences. `LibraryOverview` receives callbacks needed to enter a source or ask
  the shell to open a project; no storage or IPC moves into the frontend.
- Existing token files and layout styles remain the visual source of truth.
  New one-off colors, gradients, handcrafted icons, and inline styles are out
  of scope.

## State and error behavior

All current provider states remain represented: checking, unavailable,
downloading, verifying, starting, failed, start-failed, running, and selected.
The cleanup may shorten visible copy but must not hide the reason an action is
disabled or remove Retry/Cancel.

Library source failures stay isolated. A failed project source must not block
About you. Project switches must continue clearing old project selection and
late handoff notices before painting the next project.

No new persistence, network call, model download, provider, IPC command,
filesystem authority, prompt authority, or runtime behavior is introduced.

## Accessibility

- Preserve stable accessible names for Model, Send message, Clear chat
  transcript, Refresh Library, source navigation, and context removal.
- The model chooser traps Tab and Shift+Tab within the open dialog, supports
  Escape and outside dismissal, and restores trigger focus.
- Visible focus treatment, keyboard activation, status/live-region semantics,
  disabled explanations, and reduced-motion behavior remain intact.
- Visual simplification must not remove exact provenance or scope language
  required to understand what reaches a prompt.

## Tests and verification

Start with failing frontend regressions for:

1. one empty-chat model action and no duplicate no-model status;
2. compact model rows plus forward/backward focus containment and focus return;
3. provider progress, retry, unavailable, selected, and download states;
4. quiet transcript structure without losing metadata, manifests, errors,
   copy, Clear, or streaming semantics;
5. projectless and trusted-project Library overview actions and honest scope
   copy;
6. source isolation and project-switch generation fences remaining unchanged;
7. CSS guardrails for readable measure, reduced border nesting, narrow-window
   containment, and existing token use.

Then run focused component suites, TypeScript, the full frontend suite,
`PLUME_FULL_VERIFY=1 ./scripts/verify.sh`, pre-commit/gitleaks, and GitHub CI.
Because this is user-facing UI work, build the packaged app and perform
Computer Use smoke at the exact implementation head for empty chat, both model
paths, transcript, Library with and without a project, keyboard dismissal and
focus return, Settings/Help/workspace overlays, and quit/relaunch.

Compare exact-viewport before and after screenshots together. A screenshot is
evidence only after the visible controls and relevant accessibility paths have
also been exercised.

## Non-goals

- Agentic tool execution, multi-step coding loops, or broader patch authority.
- Semantic memory retrieval, automatic topic generation, dreaming, or memory
  links affecting prompt selection.
- New providers, Apple/Qwen runtime changes, bundled model-weight changes, or
  Ollama onboarding work.
- Browser authority, Plume-emitted computer actions, or macOS host control.
- A wholesale sidebar, Settings, Help, Browser, Files, or project-shell
  redesign.
- Final submission artifact creation or public upload.

## Completion criteria

The slice is complete when a first-run user can understand how to select a
model and start chatting without repeated instructions, both Apple and Qwen
states remain fully operable, conversation evidence stays exact but visually
quiet, and Library explains its two trust scopes without presenting a mostly
empty canvas. All capability boundaries and full verification gates must
remain green at the exact PR head.
