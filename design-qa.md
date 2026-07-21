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

## Final demo UI cleanup — 2026-07-21

### Comparison

- References: the seven user-supplied screenshots from 2026-07-21 21:22–22:12,
  including `NSIRD_screencaptureui_GgxgK5`, `SvoOZZ`, `CwM3UW`, `qiIiP8`,
  `wXfULe`, `2yefQD`, and `tggC6o`.
- Implementation: packaged arm64 Plume at 1152×768, inspected in the persisted
  Qwen2-VL dinosaur chat, model chooser, Browser split, and native Attach menu.
- Packaged captures: `Plume Screenshot 2026-07-21 at 22.53.00.jpeg`,
  `22.53.41.jpeg`, and `22.54.45.jpeg` in the Computer Use capture directory.

### Result

- The model chooser no longer paints the redundant heading, subtitle, or Back
  band. Escape and the Model control still close it.
- Transcript and composer evidence are compact `Website` and `Screenshot`
  chips. Full provenance remains available to accessibility and Details.
- Visible `You` / `Plume` speaker labels and the full model-name placeholder are
  gone; the composer now says `Message Plume`.
- Split chat has breathing room, the native Browser seam has no gray gutter,
  and its resize target stays fully on the chat side.
- Attach is a native macOS popup above the child webview. A packaged smoke found
  and fixed an immediate resource-close bug; the final accessibility tree held
  all three visible menu items until dismissal.
- The packaged app uses the supplied feather artwork and passed deep strict
  code-sign verification as an ad-hoc arm64 0.1.0 bundle.

No P0, P1, or P2 visual issues remain in the inspected final-demo states.

final result: passed
