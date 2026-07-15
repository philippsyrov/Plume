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
