# Human Browser Workspace Implementation Plan

> Execute with test-driven development and stop this slice at human navigation.

**Goal:** Ship a global, simple Browser control workspace over the isolated
`browser-sandbox` window with exact-origin localhost approval.

**Design:** `docs/superpowers/specs/2026-07-14-human-browser-workspace-design.md`

## 1. Pin the localhost session policy

- Add failing Rust tests for normalized loopback origins, exact port matching,
  session allowlist lifetime, public-target rejection of approval fields, and
  page-authored unapproved loopback denial.
- Extend `BrowserSandboxStore` with an internal loopback-origin set cleared by
  close/destroy, never serialized.
- Extend open payload with a serde-default optional approval origin and enforce
  it before creating/navigating a window.
- Re-run Browser tests and clippy.

## 2. Add fixed human navigation commands

- Add failing tests for main-webview-only Focus/Back/Forward/Reload commands and
  exact app-command/capability parity.
- Implement fixed-purpose commands; no arbitrary expression or URL bypass.
- Register them through the shared `APP_COMMANDS` registry and capability.
- Re-run command-registry, Browser, and authority tests.

## 3. Add the typed frontend API and hook

- Add `src/lib/api/browser.ts` with exact wire types and wrappers.
- Add hook tests first: initial state, polling cadence, stale-response guard,
  cleanup, action refresh, and stable product error mapping.
- Implement `useBrowserWorkspace` as the only stateful frontend owner.

## 4. Build the sparse Browser panel

- Add component tests first for public Go, localhost prompt/cancel/confirm,
  fixed controls, loading/error state, and absence of agent/evidence controls.
- Implement one compact toolbar and one quiet empty/state region.
- Add focused CSS using existing paper/ink tokens, radius, and reduced-motion
  conventions; no gradients, shadows, glass, or decorative dashboard cards.

## 5. Wire Browser globally

- Extend `ProjectWorkspaceView` with `browser`.
- Enable Browser in `ToolDrawer` for project and no-project shells.
- Add no-project view state/tool drawer without changing local-chat semantics.
- Add App/ToolDrawer tests proving Browser is global while project-only views
  keep their existing gates.

## 6. Docs and smoke contract

- Update IPC contract, safety, roadmap, operability, UI style, feature inventory,
  and smoke checklist with exact shipped/non-shipped status.
- Keep title null and subresource filtering limitations explicit.

## 7. Final gates and publication

- Run focused Rust and frontend tests.
- Run `cargo test`, all-target clippy, and `PLUME_FULL_VERIFY=1 ./scripts/verify.sh`.
- Build `Plume Smoke.app` and run the packaged localhost flow using only a
  `/private/tmp` fixture.
- Publish one focused PR, wait for GitHub verify + gitleaks, commission exact-head
  independent review, fix real findings, then squash-merge.
