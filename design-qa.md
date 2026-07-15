# Design QA — Appearance settings

## Comparison

- Reference: user-supplied Codex Appearance screenshot, 2026-07-15 13:27.
- Implementation: packaged Plume Appearance settings, 2026-07-15 14:01.
- Combined comparison inspected at `/private/tmp/plume-appearance-comparison.png`.

## Result

Passed. No P0, P1, or P2 visual issues remain.

- The Settings hierarchy matches the reference: Appearance heading, Theme label,
  and one clear row of System / Light / Dark choices.
- Plume keeps its own warm paper-and-ink design tokens, spacing, typography, and
  restrained borders instead of copying Codex artwork.
- Selected state, focus, labels, and the three working radio controls remain
  explicit and accessible.
- Light is the first-run default; Dark and System were checked in the packaged
  app. Modal copy follows the active ink token in both themes.
- The panel stays readable at the packaged window size and stacks its options at
  the existing compact breakpoint.

Custom foreground and background colors remain deliberately planned rather than
being represented by non-functional controls.

## Packaged Browser follow-up

The post-review packaged smoke was repeated after the native-webview suspension
fix. Expanded Browser now reaches the bottom edge when chat is hidden; Show chat
reveals the compact centered composer and Hide chat returns to the full canvas.
The first smoke exposed an author-CSS override that left the hidden composer
painted despite its `hidden` attribute. A dedicated `[hidden]` rule plus a style
regression fixed it before handoff.

- Full Browser canvas: `Plume Smoke Screenshot 2026-07-15 at 14.49.39.jpeg`.
- Raised chat sheet: `Plume Smoke Screenshot 2026-07-15 at 14.49.57.jpeg`.
- Settings above a suspended Browser: `Plume Smoke Screenshot 2026-07-15 at 14.50.27.jpeg`.
- Resumed Browser with the unsent page input preserved: `Plume Smoke Screenshot 2026-07-15 at 14.50.40.jpeg`.
- Dark Appearance setting: `Plume Smoke Screenshot 2026-07-15 at 14.52.11.jpeg`.

No P0, P1, or P2 visual issues remain in the inspected Browser, overlay, or
Appearance states.
