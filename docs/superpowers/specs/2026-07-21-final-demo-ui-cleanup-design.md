# Final Demo UI Cleanup Design

**Status:** approved directly in the user's 2026-07-21 screenshot review.

## Goal

Make the recorded Plume flow read as one quiet Apple-style conversation: the
model workspace contains no redundant introduction, attached Browser evidence
looks like compact chat-native objects, the composer speaks in ordinary
language, and the Browser behaves like a neighboring surface rather than a
panel that pushes or separates the conversation.

## Chosen behavior

- Remove the visible `Choose a model`, `Models run locally on this Mac.`, and
  `Back` band. The surrounding Plume workspace already owns navigation and the
  model region keeps its accessible name.
- Remove visible `You` and `Plume` labels from ordinary transcript turns. Role
  semantics remain available through list-item accessible names.
- Render attached Browser page text and screenshots with the short visible
  labels `Website` and `Screenshot`. Long titles, URLs, dimensions, byte counts,
  and provenance remain available only in existing accessible/detail surfaces.
- Replace model-id composer placeholder copy with `Message Plume` while keeping
  actual selected-model state in the compact status line.
- Add restrained internal breathing room to the chat column at split-window
  sizes without sacrificing the center column's usable width.
- Position the Browser Attach menu as an anchored overlay above the webview. It
  must not add a grid row, resize, or push the page.
- Remove the decorative gray gutter between chat and Browser; retain the narrow
  functional resize hit target with a quiet dividing line.
- Use the supplied `Plume_Icon.png` as the canonical source for every packaged
  Tauri/macOS icon size. No network assets or new dependencies are introduced.

## Boundaries

This cleanup changes presentation only. It does not grant Browser authority,
change screenshot provenance, add external Finder image import, change model
routing, or expand research/tool behavior. The demo remains Qwen2-VL for
ordinary screenshot understanding and Qwen Coder for strict research/export.

## Verification

Each behavior change starts with a focused failing test. After focused suites
and the full verifier pass, build one release app, keep only one instance open,
capture the same states as the supplied screenshots, and compare them together.
The latest `design-qa.md` must end with `final result: passed` before merge.

