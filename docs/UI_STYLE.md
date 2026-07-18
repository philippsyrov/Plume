# UI Style

The Plume UI is a quiet hand-drawn coding cafe. Paper white surfaces, ink
black outlines, thin imperfect borders, the occasional pencil shading,
restrained accent colors only for state.

Look at `reference/visual/` for the inspiration images. The aesthetic is
notebook + cafe wall. Not glossy SaaS, not dark dev dashboard, not
generative-AI gradient.

## Visual rules

- Mostly white background (`--paper`).
- Black or near-black outlines (`--ink`, `--ink-soft`).
- Hand-drawn 1.5 px borders for panels, tabs, badges, model cards. Never
  around the editor text itself.
- Occasional gray pencil shading for empty states and secondary text.
- Three accent colors only:
  - `--good` muted green for passing
  - `--warn` warm amber for memory pressure / approvals waiting
  - `--bad` muted red for destructive / failing
- No gradients. No glass. No purple-blue AI blobs. No fake chrome.

## Appearance

Warm paper and black ink are the first-run default. Settings exposes explicit
**System**, **Light**, and **Dark** choices; System alone follows the current
macOS appearance. The choice persists locally. Custom foreground and background
colors remain planned and must preserve contrast, focus, and state semantics.

## Typography

- Editor + diffs + terminal: monospace (`--font-code`). Crisp. No texture
  behind the glyphs.
- Prose UI (chat messages, headers, empty states): serif (`--font-prose`).
- UI controls (buttons, menus, status strip): sans-serif (`--font-ui`).

## Layout

The shipped consumer shell is one collapsible sidebar beside one active
workspace. Chat and Project conversations use the same calm conversation
surface; Files, Browser, Library, and Benchmarks are explicit workspace views.
The top bar carries the current title, model picker, project switch, and quiet
Workspace views control. Settings owns providers, local-model controls, Library
editing, and a closed **Advanced project tools** disclosure.

```
+--------------------------------------------------------------+
| Sidebar  | Current title        Model      Workspace views   |
+----------+---------------------------------------------------+
| Tasks    |                                                   |
| Projects |              one active workspace                 |
|          |       Chat / Files / Browser / Library            |
| Settings |                                                   |
| Help     |                                                   |
+----------+---------------------------------------------------+
```

The current consumer shell keeps model selection in the top bar and moves
technical project, provider, context, and memory facts into Settings or compact
Details disclosures. A future terminal/verify pane or additional status field
lands only when it has an honest value and a clear user action; empty badges do
not ship as decoration.

### Calm consumer hierarchy

The consumer chat uses one primary action in an empty state, divider-separated
model rows inside one popover, a readable border-light transcript, and quiet
runtime metadata. Library keeps its source tree and reading canvas; its
overview uses two scope summaries rather than dashboard cards. Borders frame
controls and major regions, not every nested group.

The following D30–D32 pane notes describe the earlier three-zone workspace and
are retained as implementation history, not as the current consumer navigation
contract. The integrated Browser has its own current resizable split.

D30 shipped resizable panes plus per-side show/hide. The hook
`useWorkspaceLayout` (in `src/features/workspace-layout/`) owns
the four-field shape (left width, right width, left visible,
right visible), enforces min/max clamps (left 200–480 px, right
260–640 px static absolutes), and persists the values to
`localStorage` under `plume:workspace-layout-v1`. The defaults
match the pre-D30 fixed widths (260 px / center fr / 340 px) so
the center keeps a useful gutter at the configured 900 px window
minimum.

The static maxes are absolute upper bounds, NOT the values the
drag handles accept on any given window. The effective max each
side accepts at drag time is computed live by `dynamicMaxFor`:
the live viewport minus the shell horizontal padding (48 px),
minus a reserved **center minimum of 280 px**, minus the other
side's current width, minus the visible handles (8 px each).

A rebalance pass shrinks both sides when their total
over-subscribes the available room. It fires on three triggers:
viewport resize, a hidden-then-shown visibility flip, and a saved
value from a wider window rehydrating at mount. The algorithm is
slack-based, not naive-proportional: each side gives up part of
its `currentWidth - staticMin` slack in proportion to how much
slack it has, so a side already at its min contributes zero to
the shrink and the OTHER side absorbs the full excess. When
combined slack ≥ excess the math fits exactly — center stays at
exactly 280 px. When combined slack < excess (both sides at or
near their mins), both sides go to their min and the center
squeezes below the reservation; that's the honest failure mode
for windows smaller than the Tauri minimum, not a layout glitch.

The combination of the 280 px center reservation, the dynamic
per-side max, and the slack-based rebalance is the load-bearing
rule that prevents maxing both sides — or toggling a hidden
panel back on after dragging the other one wider — from
collapsing or overflowing the center on any sane window size.

Drag handles sit between adjacent visible columns. Each handle is
8 px wide for hit-area but renders as a 2 px pencil-coloured
stripe that thickens on hover or focus. Keyboard-only users can
nudge a handle with Arrow Left / Arrow Right (8 px step; 32 px
with Shift); Home / End jump to the min / max clamp. The handle
advertises itself as `role="separator"` with
`aria-orientation="vertical"` and live `aria-valuenow`, so a
screen reader narrates the current width.

Show/hide is exposed two ways: small chevron buttons (`PanelToggle`)
in the status strip next to Close, and the keyboard shortcuts
**Cmd+Shift+[** (toggle left) and **Cmd+Shift+]** (toggle right).
The chevron points toward the edge the panel will collapse to
when visible, and away from the edge (toward the centre) when
hidden, so the glyph alone reads as the next action. Hiding a
side panel removes its column AND its resize handle from the
grid; the 1fr centre track absorbs the freed space.

### Inner-panel chip strip (D32)

Inside each visible side column, a compact chip strip at the top
exposes the column's individual panels as a row of pill buttons —
left column: **Files**, **Providers**, **Local models**; right
column: **Inspector** (with Diff / Preview slots reserved for
later slices). The chip strip is intentionally light: no
border around the row, ~11 px Inter labels, 4 px corner radius.

Two visual states per pill:

- **Visible** — filled, ink-soft background with paper-coloured
  label and an ink-soft border. Reads "on" at a glance, matching
  the trust badge convention.
- **Hidden** — outlined, paper-deep background with pencil-grey
  label and pencil-grey border. Lower contrast so the off state
  is obvious without being illegible. Both states keep the same
  hit area; only colours change.

`aria-pressed` follows the canonical "is this control currently
on?" semantic — pressed means visible. Each pill's `title` spells
out the action ("Hide Files" / "Show Providers" etc.) so the
behaviour is discoverable on hover.

The chip strip stays rendered whenever the column itself is
visible, even if every inner panel has been hidden. That's the
recovery affordance: a user who toggled all three left panels
off can still bring one back by clicking a pill. A small
`EmptyColumn` placeholder renders below the chips in that
all-hidden case, explaining the path back. Persistence:
`localStorage['plume:inner-panels-v1']`, independent from the
outer D30 layout key so changing one doesn't churn the other.

Drag-anywhere panel rearrangement (free-form docking, splitting a
zone into two stacks, dropping panels into a different region)
is still roadmap — a future slice owns that layout-tree
machinery.

The center `AgentWorkspace` stays a simple vertical stack —
orientation line, selected-model banner, chat panel — that flexes
with the workspace column widths. (D87 removed the old descriptive
mode-card grid; the mode controls now live in the chat header and the
left-column Agent settings card.)

### Workspace shell scrolling rule (D13)

The whole window is a fixed canvas. `html`, `body`, `#root`, and
`.plume-shell` all set `overflow: hidden`. Page-level scrolling
is forbidden — if a pane grows past the window edge it MUST own
its own internal scroll (the file listing, the chat transcript,
the inspector editor's CodeMirror scroller). A new pane that
inherits this rule needs `min-height: 0` on its flex/grid
container chain and an explicit `overflow: auto` on the body
that actually scrolls. The trusted-view variant of the shell
(`.plume-shell-compact`) drops the global Plume hero so the
compact status strip is the top-of-window identity; the open
form keeps the hero because there's no project context strip
yet at that point.

D98 makes the **left column itself** one of those scroll bodies:
`.plume-workspace-left` is `overflow-y: auto` and its panels keep
their natural height (`> * { flex-shrink: 0 }`), so the stack of
panels (file tree, providers, local models, memory, and the three
agent cards) scrolls as a unit instead of overflowing past the
window edge where the shell's `overflow: hidden` would clip it.
The inner-panel toggle strip is `position: sticky; top: 0` so the
panel chips (and the recovery affordance when everything is hidden)
stay reachable mid-scroll. The file navigator no longer `flex: 1`-
fills the column — that collapses to nothing once siblings overflow
a scroll container — it caps at `--nav-max-height` (50vh default)
and scrolls its own listing. The center scrolls via
`.plume-agent-workspace`; the right column holds a single
fill-and-scroll inspector, so only the left column carries the
column-level scroll.

### Window-fill unified shell (D64)

The unified chat workspace (`.plume-project-codex`, both the
trusted-project and no-project shells) fills the window
edge-to-edge, Codex-style. The compact shell
(`.plume-shell-compact`) has **no outer padding or gap**, and the
codex root carries **no border, border-radius, or window shadow**
— the OS window chrome is the frame. What separates the full-bleed
panes is the internal hairlines: the sidebar's `border-right` and
the topbar's `border-bottom`. Internal floating surfaces (tool
drawer, settings window, session dialogs) keep their rounded
corners (`--plume-chrome-radius-window`) and shadows — the warm
palette, typography, and control styling are unchanged. The hero
views (open form, trust gate) keep the padded card layout of the
base `.plume-shell`. Contract pinned by
`src/features/project-shell/windowFill.test.ts`.

### Inspector gutter (CodeMirror, D13)

The read-only editor's `.cm-gutters` paints `var(--paper)` and
holds a `min-width: 40px`. CM6 keeps the gutter `sticky` to the
left edge of the scroller, and a transparent gutter lets
horizontally-scrolled content paint visually under the line
numbers — the paper background occludes it cleanly. Any future
extension to the editor (line annotations, breakpoints, gutter
icons) MUST keep the solid background so the same overlap
doesn't reappear.

## Historical Simple Mode vs Developer Mode proposal

An earlier product proposal described a user-switchable Simple/Developer pair.
That toggle is **not shipped**. The current consumer shell uses one interface
with progressive disclosure: ordinary Chat and Project views stay calm, while
technical controls live in Settings, **Advanced project tools**, workspace
views, or local **Details** disclosures. The internal `ChatPanel` `simple`
variant is a presentation style, not a user mode or authority boundary.

The historical proposal below is retained only as design context. It must not
be used as a current manual-test contract or capability claim:

The visual contract differs along four axes:

1. **Panel count.** Simple Mode renders one panel at a time
   (chat fills the workspace). Developer Mode renders the full
   three-zone shell (navigator + workspace + inspector). The
   `.plume-shell` grid is the same in both — Simple just hides
   the left and right zones via `display: none`, keeping the
   center column at its full width plus the side gutters'
   whitespace.
2. **Whitespace.** Simple Mode uses larger side margins on the
   chat surface so the prose stays in a comfortable measure
   (roughly 60-80 characters per line for the serif). Developer
   Mode keeps the existing edge-to-edge density. The token
   vocabulary is the same — Simple just picks higher `--space-*`
   values for its outer padding.
3. **Status strip.** Simple Mode renders only model/memory
   telemetry (model name + memory pressure with the same
   green / amber / red rule above); Developer Mode renders the
   full target strip from the section above. Trust badge,
   Close button, and the mode toggle stay visible in both —
   they are persistent project controls, not mode-toggleable
   telemetry, and a user in Simple Mode never loses access to
   them.
4. **Disclosures.** Simple Mode hides the provider strip, the
   file tree, the file inspector, the mode-card grid, the
   propose-diff toggle, the AGENTS.md badge, the context
   preview row, and the per-reply telemetry footer. None of
   them are removed — they sit behind a `Show developer
   controls` affordance (described in
   `docs/AGENT_OPERABILITY.md`) that flips to Developer Mode
   when activated. Developer Mode renders all of them inline.

What Simple Mode does NOT change:

- The hand-drawn cafe identity. Same paper white, same ink
  black, same sketched borders on the chat panel and the
  status strip. The aesthetic does not get more "consumer";
  it gets more spacious.
- The empty-state convention. A Simple-Mode session with no
  selected model still renders a paper card with one sentence
  and one obvious next action — the same convention as the
  Developer-Mode mode-cards grid uses for unshipped stages.
- Editor + diff readability. If Simple Mode ever surfaces a
  diff (via `Propose diff` reached through the disclosure), the
  diff panel uses the same coloring and shape Developer Mode
  uses. Readability rules win across modes.
- Accessibility. Both modes meet the same contrast, focus, and
  accessible-name requirements. See `docs/AGENT_OPERABILITY.md
  § Mode toggle` for the toggle's accessibility contract.

No product-mode preference or mode-toggle persistence exists today. Any future
appearance or density choice must preserve the same scope, trust, and
accessibility contract rather than creating a second authority model.

## Tokens

Single source: `src/styles/tokens.css`. Components reference variables;
never inline a color or size. Adding a new token is a deliberate decision
that should be documented here.

## Component vocabulary

The shared primitives will live in `src/app/ink/`:

- `InkButton`, `InkIconButton`
- `InkPanel`, `InkTab`
- `InkBadge` — model name, mode, status
- `InkInput`, `InkSelect`, `InkToggle`, `InkSlider`
- `InkDivider`
- `InkTooltip`, `InkModal`

Every component must support keyboard nav, visible focus, disabled state,
and predictable sizing. Hand-drawn does not mean sloppy — it means
deliberately imperfect borders around a strict UI.

Session controls (D63B) follow the same system: the sidebar session
row reuses the D62 action-row classes with a small `…` popover menu
(Escape and outside-press close it; items are `role="menuitem"`);
rename / delete-confirmation / archived-chats reuse the settings-modal
backdrop + window frame. Destructive actions (`Delete permanently`,
`Confirm delete`) are the only red-tinted text in the sidebar system,
and delete always takes an explicit second click — no native browser
dialogs anywhere (see Anti-patterns).

The chat-search overlay (D66) is the same modal system in a compact
palette shape: settings backdrop, `--plume-chrome-radius-window`
frame, muted input field, hit rows highlighted with the chrome-muted
fill. Snippet highlights use a soft warm `mark` tint — the only
highlight color in the sidebar system; the `archived` pill reuses the
muted chip treatment.

The explicit context shelf is an essential composer control in both Simple and
Developer modes, not developer telemetry. It renders one ordered compact row per
typed source with kind, exact provenance label, ready/checking/blocked state,
and an individually accessible remove button. A blocked source stays visible
and keeps its reason in the tooltip; ready neighbors do not disappear. The
shelf shows Browser captures as selection/page plus title/host, byte and
redaction/truncation state, and a short redacted preview so two captured pages
are distinguishable before Send. The
shelf is sticky for the project session, so successful Send does not animate or
clear it. Accepted user turns render a separate immutable compact manifest:
that evidence describes what reached that historical prompt and has no remove
control.

Library and Files add a progressive drag gesture over that same shelf. A
dedicated **Use in chat** action remains clickable and keyboard-accessible; when
dragged, the current workspace reveals one generous temporary chat drop tray
near the bottom edge. The tray is not a second shelf and is absent at rest. It
uses a soft state tint, dashed outline, and one restrained entry transition;
hover copy says the item will be added to chat. Success opens the canonical
shelf and briefly emphasizes the exact added or existing row.
`prefers-reduced-motion: reduce` removes both animations. No glow, confetti,
sound, or raw content appears in the drag payload.

## Library workspace

Library borrows Obsidian's calm source-tree, note-index, reading-canvas, and
backlink hierarchy without copying its plugin density or theme. The visible
sources are **Overview**, **About you**, **This project**, **Topics**, and
**Connections**. Scope is never inferred from visual position: About you is
app-private and available without a project; This project and Topics are shown
only for the currently trusted project.

The default canvas has a compact source tree, a searchable row index, a
readable detail pane, and Connections inside the selected detail. Use calm rows
and one selected state rather than dashboard cards. Human text and titles come
first; ids, stored paths, timestamps, bytes, and redaction counts sit under
**Details**. Search copy names the selected boundary, because Library does not
silently search other projects or all stores at once.

Links and backlinks are exact stored organization metadata. Connections says
plainly that it organizes information and does not choose what goes into chat.
There is no graph, semantic similarity, automatic retrieval, distillation, or
dreaming surface in Library. Mutations stay in **Settings → Library**:
**About you** offers app-private CRUD, while **This project** reuses the trusted
project memory/topic controls.

Each eligible memory or canonical topic keeps an ordinary **Use in chat**
button plus the typed drag gesture. The action sends only an opaque ref; Rust
re-resolves the owning store at preview/send. User memory can go to local or
project chat, while project memory/topics can go only to project chat. Core
topic files that are ambient project context do not gain an explicit action.
Duplicate/full/unavailable outcomes stay visible and never imply the source was
attached. Project switches clear old project rows before the next load so stale
content cannot remain visible beneath a new project title.

## Unified top bar

The unified top bar is always visible above the workspace. It shows one current
surface title, one model picker, the quiet workspace-view control when useful,
and the project switch action. Project trust, paths, package managers, context
manifests, and memory details do not form a permanent diagnostic strip in the
consumer shell; they live in the owning surface or under **Details**.

Future status fields land only when they have honest values and a clear user
action. An "unknown" badge for every possible field would teach users that the
bar is decorative.

There is no separate Simple/Developer top bar and no diagnostic-strip mode
toggle. Runtime and memory facts belong in Settings or their owning Details
surface until a later design gives them one clear action.

## Empty states

When a pane has nothing to show, render a paper card with a single sentence
in serif explaining what the pane is for and one obvious next action. No
illustrations of robots. No marketing language.

## Accessibility

- Body text contrast >= 7:1 on `--paper`.
- All controls reachable by keyboard. Focus rings are visible and
  ink-colored.
- Interactive controls must have useful accessible names and roles. Treat
  those names as part of the UI contract because computer-use agents and
  screen readers depend on them.
- Loading, disabled, selected, error, and approval states must be visible
  on screen and reflected to accessibility where the platform supports it.
- No animation required to understand state. Respect
  `prefers-reduced-motion`.
- Code and terminal fonts default to 14 px and are user-adjustable.
- The hand-drawn texture must never reduce text clarity. If it does, drop
  the texture for that surface.

Agent-operable UI is the same requirement with a stricter workflow bar:
anything important a human can do should also be possible through visible
mouse, keyboard, and accessibility interactions. See
`docs/AGENT_OPERABILITY.md`.

## Human Browser workspace

Browser belongs to the current chat. The default is a calm split workspace:
the same task chat stays on the left and its page lives on the right. One
control expands the page into the full main canvas. A **Show chat** control
pulls up a compact centered bottom sheet in its own reserved row; **Hide chat**
returns to a genuinely full-height page, and the inverse layout control returns
to split view. The native child webview must never be covered by parent HTML,
so the open chat sheet reserves space rather than relying on z-index. Keep the
chrome sparse—tabs, address, Go, Back, Forward, Reload, Attach, and the layout
toggle. Do not render capability labels, DOM controls, or agent traces.

One quiet **Attach** menu offers **Selected text**, **Readable page text**, and
**Visible screenshot**. These are ordinary human actions, not agent controls.
Visible screenshot means the current Browser viewport, not the full page. It hands an
opaque immutable image ref to the owning chat shelf; the shelf shows
Image, title/host, dimensions, bytes, and any model-support block in plain
language. Local chat evidence remains app-private; project chat evidence stays
under that trusted project. Errors use short product copy and never expose
backend paths.

Browser layout, tab order, admitted history, and unfinished address draft restore
only with the owning persisted chat. A privacy-reduced saved URL never reopens
silently: Plume explains the reset and requires an explicit **Reopen page** action.
Before any parent-HTML overlay or native geometry change, Plume hides the child
webview; this prevents Settings/Help from appearing behind the page and prevents
stale browser rectangles during window resize or movement.

Localhost approval uses one small in-app confirmation card with the exact origin,
plain lifetime copy, and `Cancel` / `Open local site`. No native browser prompt,
remember checkbox, blanket local-network permission, gradient, shadow, or modal
maze. Loading and failure remain text-visible and announced through the normal
status path.

## Computer-use trace area (post-MVP)

When the computer-use track lands (see
`docs/IPC_ROADMAP.md § Computer use`,
`docs/SAFETY.md § Computer-use sandbox`,
`docs/PLUME_PROJECT_SPEC.md § 13.5`), the chat panel grows a
visible trace surface. The visual treatment follows the existing
hand-drawn cafe language — no new design vocabulary:

- Trace rows render in the same `ink-panel` paper style as chat
  entries, one row per action, with a small sketched glyph for
  the action kind (click / type / scroll / drag / capture /
  observe). No emoji, no marketing icons.
- Status carries through the existing `--good` / `--bad` / pencil
  tokens. `executed` rows are pencil-grey, `rejected` rows pick
  up a `--bad` border-left, `pending-approval` rows pick up
  a `--warn` border-left while the approval is in flight.
- The live screenshot pane is bordered in pencil with a corner
  label showing the target name in the same serif used for empty
  states. No drop shadow, no glow.
- The Pause / Stop buttons share the `ink-button` shape and live
  on the session header — they are not floated, not glassy, not
  animated.
- The session approval dialog reuses the project-trust prompt's
  visual shape: paper card, plain prose explaining what the
  session is about to do, two visible buttons (Approve / Reject)
  with focus rings, no checkbox shortcuts.

The trace is also content under the accessibility rules above —
`role="log"` with `aria-live="polite"`, action kinds reflected as
text in the row's accessible name, status changes announced.
Visual brevity does not come at the expense of those names.

## Anti-patterns

Do not:

- Add giant gradients.
- Use purple/blue AI iconography.
- Add cute illustrations inside the editor or diff view.
- Animate panel borders on every state change.
- Use color as the only signal for failure.
- Wrap monospaced code in a serif "speech bubble".
