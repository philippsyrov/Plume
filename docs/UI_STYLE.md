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

## Typography

- Editor + diffs + terminal: monospace (`--font-code`). Crisp. No texture
  behind the glyphs.
- Prose UI (chat messages, headers, empty states): serif (`--font-prose`).
- UI controls (buttons, menus, status strip): sans-serif (`--font-ui`).

## Layout

The trusted-project shell as it ships today is the **Developer
Mode** render (see `docs/ARCHITECTURE.md` § Trusted-project
workspace shell for the React-side specifics). The Simple Mode
render of the same shell is described under "Simple Mode vs
Developer Mode" below.

```
+--------------------------------------------------------------+
| ProjectStatusStrip  name | path | trust | git | pm | Close   |
+----------+----------------------------+----------------------+
| Navigator|                            |                      |
|  (file   |        AgentWorkspace      |    FileInspector     |
|   tree)  |        (placeholder)       |   (CodeMirror /      |
|          |                            |    blocked / empty)  |
+----------+                            |                      |
| Providers|                            |                      |
+----------+----------------------------+----------------------+
```

The eventual layout adds a terminal/verify pane below the workspace and
expands the status strip with provider, model, context, and memory
fields. None of that has shipped yet — the current strip carries
project name, path, trust badge, git state, package managers, and the
Close button. Mode + memory + context belong on the strip once they
have honest values to display.

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

## Simple Mode vs Developer Mode

Plume renders the trusted-project shell in one of two modes; the
shell above describes Developer Mode. Simple Mode is the default
a brand-new user lands in. The product axis is described in
`docs/PLUME_PROJECT_SPEC.md § 7.7`; this section pins the visual
rules. No new tokens — both modes use `--paper`, `--ink`,
`--ink-soft`, `--pencil`, `--radius-soft`, `--radius-small`, and
the `--good` / `--warn` / `--bad` accents already documented
above.

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

The mode is per-project. A user can flip modes mid-session —
the IPC underneath does not change, only the renderer does, so
an in-flight streaming reply continues uninterrupted.
Persistence ships with the IPC graduation described in
`docs/IPC_ROADMAP.md § Session mode and policy`; until then,
mode resets to Simple on every project open.

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
shelf is sticky for the project session, so successful Send does not animate or
clear it. Accepted user turns render a separate immutable compact manifest:
that evidence describes what reached that historical prompt and has no remove
control.

## Status strip

Always visible above the workspace shell. The shipped strip and the
target strip are deliberately different — the target items only land
on the strip once they have honest values to display.

Today (D1.5) the strip carries:

`name · path · trust · git · package managers · Close`

Target shape, in order:

`provider · model · context · memory · branch · dirty · mode · network`

The new fields slot in as their backends land — `provider` and `mode`
arrive with the chat slice, `model` and `context` with the model-load
slice, `memory` with the resource-honesty slice. None of them belong on
the strip until a real value can be displayed; an "unknown" badge for
every field would teach users the strip is decorative.

The memory color follows the runtime estimate:

- Green: comfortable.
- Amber: watch it.
- Red: likely to hurt performance.

The target strip above is the **Developer Mode** strip. The
**Simple Mode** strip shows only model/memory telemetry; trust,
Close, and the mode toggle stay visible as persistent project
controls in both modes (they are not toggleable). The other
telemetry fields (provider, context, branch, dirty, network)
stay reachable by flipping the mode toggle to Developer. The
mode toggle itself lives on the right end of the strip — see
"Simple Mode vs Developer Mode" below.

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
