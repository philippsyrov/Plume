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
3. **Status strip.** Simple Mode renders only model name and
   memory pressure (with the same green / amber / red rule
   above). Developer Mode renders the full target strip from
   the section above. Trust badge and Close button stay
   visible in both — they are project-state, not mode-state.
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
**Simple Mode** strip renders model name and memory only; the
other fields stay reachable through the mode toggle. The mode
toggle itself lives on the right end of the strip in both
modes — see "Simple Mode vs Developer Mode" below.

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

## Anti-patterns

Do not:

- Add giant gradients.
- Use purple/blue AI iconography.
- Add cute illustrations inside the editor or diff view.
- Animate panel borders on every state change.
- Use color as the only signal for failure.
- Wrap monospaced code in a serif "speech bubble".
