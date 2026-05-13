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

The trusted-project shell as it ships today (see `docs/ARCHITECTURE.md`
§ Trusted-project workspace shell for the React-side specifics):

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

Resizable panes via simple drag dividers will land later. Today the
column widths are fixed (260 px / center fr / 340 px) so the center
keeps a useful gutter at the configured 900 px window minimum; the
mode-card grid inside `AgentWorkspace` collapses to a single column
at narrow widths.

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

### Inspector gutter (CodeMirror, D13)

The read-only editor's `.cm-gutters` paints `var(--paper)` and
holds a `min-width: 40px`. CM6 keeps the gutter `sticky` to the
left edge of the scroller, and a transparent gutter lets
horizontally-scrolled content paint visually under the line
numbers — the paper background occludes it cleanly. Any future
extension to the editor (line annotations, breakpoints, gutter
icons) MUST keep the solid background so the same overlap
doesn't reappear.

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
