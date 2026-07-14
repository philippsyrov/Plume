# Consumer workspace, task Browser, Library, and Handbook design

## Status

Approved product design for the next Plume implementation campaign. This
document does not describe shipped behavior until its slices merge and the
feature inventory records them as shipped.

## Goal

Make Plume understandable to someone who already knows ChatGPT or email while
preserving the exact authority, provenance, and local-first guarantees already
implemented.

The product model is:

- **Chat answers.** A casual chat can browse with the person and use explicit
  web evidence, but it has no project files or project actions.
- **Projects act.** A project chat has the same conversational Browser plus
  trusted local-app testing, project context, patching, and other explicitly
  approved project workflows.

The campaign also turns the current collection of panels into one restrained
consumer workspace, reorganizes Knowledge as a clear Library, and writes the
first human-readable Plume Handbook alongside the UI.

## Product principles

1. **Simple by default, exact on demand.** Friendly summaries are visible;
   manifests, byte counts, filenames, model capability evidence, and raw ids
   remain available under **Details**.
2. **No hidden authority.** Browser pages, memories, files, and topics enter a
   prompt only after an explicit human attachment or an already-documented
   project contract.
3. **One surface, one visual language.** Plume has one identity, one sidebar,
   one typography hierarchy, one icon family, and consistent controls.
4. **Native before heavyweight.** The first consumer Browser continues to use
   macOS WebKit. A Chromium fork is a later evidence-driven decision, not a
   prerequisite for browsing, quick research, localhost testing, or artifact
   preview.
5. **Borrow workflows, not pixels.** Codex informs task/Browser spatial behavior
   and Obsidian informs Library navigation. Plume keeps its own calm,
   Apple-like restraint and safety boundaries.

## Chosen workspace model

### Browser ownership

Every persisted chat session owns one logical Browser workspace. The workspace
contains at most five tabs, one active tab, a bounded app-recorded top-level URL
history per tab, the split-panel width, and the split/expanded layout choice.

The chosen interaction is a hybrid of the two strongest explored directions:

- **Split by default:** chat and Browser share the task canvas.
- **Browser canvas on demand:** Expand gives Browser the full task canvas while
  keeping a compact task composer available.
- **Same workspace in both states:** expanding never creates another browser,
  changes tab ownership, or loses history. Returning restores the prior split.

The rejected alternatives are a permanently browser-first canvas, which weakens
long conversations, and mutually exclusive Chat/Browser/Files modes, which add
constant context switching.

### Live-resource policy

Only the selected task's active Browser workspace is live. Inactive tasks keep
bounded descriptors rather than hidden running webviews. Returning to a task
recreates its tabs and reloads each tab's last restorable URL.

Restoration is intentionally honest: Plume restores recorded top-level URLs and
its own bounded history, not transient DOM state, form contents, scroll
position, JavaScript heap, or an exact WebKit back-forward list. The UI says
**Reload this page** when restoration cannot reproduce a page.

WebKit website data such as ordinary cookies belongs to one app-owned Plume
Browser profile, as in a normal browser profile. Cookies are never copied into
session databases, evidence records, manifests, or prompts. Per-task private
profiles and a Chromium backend remain later research candidates.

### URL persistence

Credential-bearing URLs remain rejected. Restorable URLs are local session
data, never prompt context. Before persistence, URL fields run through the
existing secret-shape checks. A URL containing unsafe query or fragment data is
stored only as its safe origin/path plus a `manualReopenRequired` marker; Plume
does not write a token-bearing URL merely to make restoration convenient.

Each tab history is capped at 20 admitted top-level URLs. Page-authored popup,
download, unsupported scheme, and unapproved loopback navigation policies stay
closed.

## Browser product surface

The task Browser exposes ordinary controls:

- Back, Forward, Reload;
- one address/search field;
- New tab and Close tab;
- Expand and Return to split;
- Attach.

**Attach** opens exactly three actions when the page supports them:

- selected text;
- readable page text;
- visible screenshot.

The composer remains usable in split and expanded states. Navigation does not
automatically attach, summarize, or send page content. Streaming chat may block
context-shelf mutation, but it does not freeze human browsing.

### Casual Chat

Casual-chat Browser evidence is stored in Plume's private application data and
is owned by that local session. It supports answers grounded in explicit page
text, selections, or screenshots, but grants no project file, localhost-project,
patch, shell, or agent authority.

Deleting a casual chat deletes its live Browser descriptors and its unshared
private evidence. Fork/rewind children retain historical accepted-turn
manifests, start with an empty live shelf, and do not inherit a running webview.

### Projects

Project captures retain the existing project-scoped immutable evidence store,
trust gates, exact manifests, model capability checks, and localhost approval.
The task identity is rechecked after asynchronous navigation and capture so a
project/chat switch cannot attach evidence to the wrong shelf.

Localhost approval is exact-origin and bounded to the relevant live Browser
session. There is no blanket local-network permission or remember-forever
toggle.

## Persistence model

Browser workspace persistence is additive and versioned. Existing local and
project session databases migrate with no Browser state.

A separate browser-workspace relation, rather than more nullable fields on the
session row, owns:

- session id and scope;
- layout mode and bounded split width;
- active tab id;
- ordered bounded tab descriptors;
- bounded admitted URL history;
- restoration status and timestamps.

Every load re-validates ids, counts, URLs, ordering, caps, and scope. Malformed
Browser state is treated as corrupt Browser state, not a corrupt transcript:
chat still opens with a reset Browser workspace and a visible recovery notice.

Browser state writes are serialized with session transitions. Chat selection,
project switching, deletion, fork, rewind, navigation callbacks, and capture
callbacks all compare the current `{scope, sessionId}` identity before
committing state.

## Unified consumer shell

### Navigation

The left sidebar has one Plume identity and one **New chat** action. It is
collapsible and avoids repeated `Plume`, `local chat`, `Simple chat`, and
`project chat` labels.

The default information architecture is:

- New chat;
- Search;
- Library;
- current tasks and projects;
- Settings / Help in the quiet footer.

**Scheduled** does not ship as a decorative destination. It appears only after
real schedule persistence, permissions, pause/resume, run status, and run
history exist.

### Chat

The default composer resembles a familiar consumer chat surface. Advanced
response modes no longer float as unexplained controls over an empty canvas.
Project instructions, attached context, and model state use ordinary labels and
one compact disclosure:

- `AGENTS.md` becomes **Project instructions**;
- raw filenames, paragraph symbols, byte counts, ids, and exact manifests move
  under **Details**;
- Propose diff is selected through an explained task/action control;
- empty model states say what to do next once, with one direct action.

Continue and Rewind remain high-value actions. Their menus get opaque surfaces,
consistent spacing, and short explanations of what will be copied and what will
stay unchanged.

### Visual system

The macOS titlebar visually merges with the application surface in light and
dark themes. The shell uses the system UI face for controls and prose, a single
monospace face for code/evidence, and a restrained display treatment only where
it creates real hierarchy.

One coherent icon family replaces unexplained glyphs. Menus and popovers are
opaque, correctly stacked, keyboard reachable, and bordered consistently.
Spacing, row heights, corner radii, focus rings, and control density come from
shared tokens rather than local panel inventions.

## Library design

Library replaces the unexplained standalone Knowledge workspace. Obsidian is
the navigation reference: a calm tree, quick search, tabs/detail reading, and
visible backlinks. Plume deliberately removes plugin density and exposes a
smaller opinionated structure:

- **Overview**;
- **User memory**;
- **Project memory**;
- **Topics**;
- **Connections**.

The default Library layout is:

1. a compact source/tree column;
2. a searchable list or note index;
3. a readable detail canvas;
4. an optional Connections/Provenance inspector.

User and project scope are always visible. Memory cards and topic notes show
human titles and summaries before ids or paths. Exact source, timestamps,
backlinks, redactions, and stored identifiers remain available under Details.

Existing memory-topic links remain organization metadata only. Backlinks and
Connections do not silently change prompt selection, semantic retrieval, or
agent authority. The current read-only search and exact context-shelf attachment
paths remain the only prompt-facing behavior unless a later retrieval design is
separately specified and approved.

Library objects may be clicked or dragged onto a chat. Scope checks, duplicate
handling, caps, provenance, and stale async protection reuse the existing typed
context shelf.

No graph view ships in this campaign. A graph is useful only after the link
model and navigation density justify it; it is not included because Obsidian
has one.

## Plume Handbook

The campaign writes a human guide alongside the product instead of treating
technical architecture documents as onboarding.

The Handbook covers:

- Chat versus Projects;
- choosing and starting a local model;
- Browser split/expanded use, tabs, localhost approval, and attachments;
- files, project instructions, context, memory, topics, and Library;
- Continue, Rewind, patch apply/revert, and current permission boundaries;
- simple worked examples with packaged-app screenshots;
- troubleshooting in ordinary language;
- an explicit **Available now / Planned** capability page.

The canonical first artifact is `docs/USER_GUIDE.md`, linked from the docs
spine and an in-app **Help** action. Screenshots are captured from the verified
packaged application and updated when the final shell changes materially.

## Error and recovery design

- A failed Browser restoration affects only that workspace, never the
  transcript or another tab.
- Missing pages, offline providers, unsupported screenshots, blocked
  attachments, and localhost approval use short actionable copy.
- Navigation/capture completions from an old task identity are discarded.
- Deleting a selected session tears down its live webview before deleting its
  descriptors and private evidence.
- Store capacity never silently evicts evidence referenced by a live shelf.
- Browser and Library async loaders use generation/identity checks and clear
  stale visible state during transitions.
- Safety-critical provenance is progressively disclosed, not removed.

## Accessibility and operability

Every important action remains available through visible controls with stable
accessible names. Split handles, tab controls, Expand/Return, Attach, Library
navigation, menus, Continue, and Rewind are keyboard reachable. Loading,
selected, approval, error, and disabled states are visible and accessible.

No animation is required to understand state, and reduced-motion preferences
are respected. The same visible UI remains operable by external computer-use
agents; this does not grant Plume's own model computer-use authority.

## Implementation campaign

The work is one autonomous goal delivered as coherent independently reviewed
PRs:

1. **Session Browser foundation:** versioned workspace persistence, bounded tabs
   and history, local evidence store, deletion/migration/scope races.
2. **Integrated Browser workspace:** split/expanded canvas, tab and navigation
   controls, local/project attachment handoff, restoration and recovery UI.
3. **Unified consumer shell:** titlebar, sidebar, typography, icons, tokens,
   opaque menus, composer and progressive-disclosure cleanup.
4. **Library:** rehome Knowledge into the Obsidian-informed, Plume-restrained
   information architecture without changing retrieval authority.
5. **Handbook and end-to-end polish:** user guide, screenshots, Help entry,
   packaged walkthrough, cross-slice fixes.

Each PR branches from clean verified main, includes focused tests and relevant
docs, passes the full verifier and GitHub checks, receives packaged-app smoke
where visual behavior changes, and is independently reviewed at the exact head
before squash merge.

## Verification matrix

The campaign must directly cover:

- old-database migration with empty Browser state;
- local/project session separation;
- five-tab, history, URL, evidence, and storage caps;
- tab close, chat delete, fork, rewind, relaunch, and project-switch behavior;
- stale navigation/capture callbacks after task switches;
- unsafe URL persistence and loopback approval lifetime;
- local evidence integrity, link/path safety, deletion, and capacity;
- split/expanded restoration and responsive minimum sizes;
- Browser use during chat streaming;
- Library search, scope, backlinks, drag/click attachment, and stale loaders;
- titlebar/theme, menu opacity, typography, icons, keyboard, and reduced motion;
- Handbook links, screenshots, and available/planned truth.

Final proof includes focused Rust/frontend suites, the complete Rust suite,
`PLUME_FULL_VERIFY=1 ./scripts/verify.sh`, GitHub verify, gitleaks, packaged
WebKit smoke, exact-head review, and a non-technical Handbook walkthrough.

## Honest exclusions

This campaign does not add agent-driven browsing, hidden retrieval, automatic
page ingestion, DOM/tool authority for models, computer use, macOS host control,
Chromium, extensions, DevTools, cloud sync, private profiles, a knowledge graph,
semantic memory retrieval, dreaming, scheduled automation, or a claim that
every local model can create high-quality artifacts.

Slides, documents, spreadsheets, and PDFs are the next product track after the
consumer workspace. They will use typed artifact specifications, deterministic
renderers, templates, validation, and Browser-based preview/visual QA. A small
specialist model is a later optimization, not the foundation.

## Success criteria

A non-technical user can:

1. understand whether they are asking a Chat or working in a Project;
2. browse beside a conversation, expand the page, switch tasks, and return
   without losing the task's Browser state;
3. explicitly attach web evidence and understand what was attached;
4. find user memory, project memory, topics, and connections in Library;
5. use Continue and Rewind without knowing harness terminology;
6. recover from missing models, stale pages, blocked evidence, and approvals;
7. learn the current product from the Handbook without reading developer docs.

The implementation is complete only when these behaviors are proven in the
packaged app and the capability inventory states them honestly.
