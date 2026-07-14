# Integrated Task Browser Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Put a five-tab WebKit Browser inside each selected chat task, split beside chat by default and expandable to the task canvas with a compact composer.

**Architecture:** Replace the global separate Browser window with up to five capability-isolated Tauri child WebViews attached to the main window for the selected session. A Rust runtime manager owns child labels, geometry, navigation generations, and lifecycle; React owns visible task layout and persists descriptors through PR 1's workspace API. Exact identity checks guard every async navigation/capture completion.

**Tech Stack:** Tauri 2 child WebViews, macOS WebKit, Rust, TypeScript, React 19, ResizeObserver, Vitest, packaged-app smoke.

## Global Constraints

- No iframe fallback, popup Browser, Chromium, DevTools, extensions, agent navigation, or automatic attachment.
- Child WebViews match no application capability and cannot invoke Plume commands.
- Browser works in casual Chat and Projects; only Projects gain localhost/project actions.
- One selected task workspace is live. Switching tasks destroys/hides its child WebViews before another task becomes authoritative.
- Browsing stays usable during model streaming; shelf mutation may remain blocked.

---

### Task 1: Prove and encapsulate the child-WebView seam

**Files:**
- Create: `src-tauri/src/browser/runtime.rs`
- Create: `src-tauri/src/browser/runtime_tests.rs`
- Modify: `src-tauri/src/browser/mod.rs`
- Modify: `src-tauri/Cargo.toml` only if Tauri's child-WebView API requires its documented feature.

- [ ] Add compile-level/unit tests for deterministic child labels, main-window-only parent, bounded geometry, and inactive-tab visibility planning.
- [ ] Implement a minimal `BrowserRuntimeManager` around `Window::add_child(WebviewBuilder, position, size)` with lifecycle behind a trait so pure planning is testable.
- [ ] Preserve incognito/browser-extension/autofill/devtools and popup/download denial policy from the current sandbox builder.
- [ ] Run `cd src-tauri && cargo test browser::runtime_tests -- --nocapture` and `cargo check`; do not continue if the native child seam cannot compile on the pinned Tauri version.

### Task 2: Session activation, tabs, navigation, and geometry commands

**Files:**
- Modify: `src-tauri/src/browser/runtime.rs`
- Create: `src-tauri/src/commands/task_browser.rs`
- Create: `src-tauri/src/commands/task_browser_tests.rs`
- Modify: `src-tauri/src/commands/mod.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/capabilities/default.json`

- [ ] Write failing command tests for activate/deactivate, open/close/select tab, navigate/back/forward/reload, five-tab cap, stale session/tab ids, geometry bounds, and command registry parity.
- [ ] Implement commands keyed by `{scope, sessionId}` plus tab id where needed; resolve scope server-side and recheck selected identity before mutation.
- [ ] Serialize runtime operations; destroy all child WebViews on selected-session deletion, project close/switch, app teardown, or runtime reset.
- [ ] Make page callbacks append only admitted top-level URLs through PR 1's store and discard stale generations.
- [ ] Re-run command/runtime tests and clippy.

### Task 3: Casual and project evidence capture

**Files:**
- Modify: `src-tauri/src/commands/task_browser.rs`
- Modify: `src-tauri/src/browser/evidence.rs`
- Modify: `src-tauri/src/browser/screenshot_evidence.rs`
- Modify: `src-tauri/src/browser/native_snapshot.rs`
- Modify: `src-tauri/src/prompts/explicit_context.rs`
- Modify: `src-tauri/src/commands/chat/context.rs`
- Modify: `src-tauri/src/commands/chat/send.rs`
- Test: existing Browser evidence, prompt, and chat command suites.

- [ ] Add failing tests proving local captures land only in the session-private store and project captures only in the trusted project store.
- [ ] Extend Browser evidence refs/manifests with owning scope/session metadata without exposing paths; old project records remain readable.
- [ ] Bind capture tickets to `{scope, sessionId, tabId, pageGeneration, currentUrl}` and recheck identity after callback/image encoding.
- [ ] Delete a casual chat's evidence on session delete; never delete evidence still referenced by that session's historical accepted-turn manifests before the session itself is deleted.
- [ ] Prove local chat can preview/send its own Browser evidence but cannot resolve project evidence or gain project context.

### Task 4: Frontend task Browser state hook

**Files:**
- Replace: `src/features/browser/useBrowserWorkspace.ts`
- Modify: `src/features/browser/useBrowserWorkspace.test.tsx`
- Modify: `src/lib/api/browser.ts`
- Consume: `src/lib/api/browserWorkspace.ts`

- [ ] Write failing hook tests for initial activation, five tabs, selection, close fallback, history, relaunch restore, split width/layout, corrupt reset notice, stale task switch, and unmount cleanup.
- [ ] Implement one hook keyed by exact `SurfaceIdentity`; clear old visible state immediately on identity change.
- [ ] Queue descriptor saves and compare identity again after every awaited IPC call.
- [ ] Model restore honestly: descriptors return immediately; runtime reports reloading/manual reopen/failure separately.
- [ ] Re-run the hook suite and typecheck.

### Task 5: Hybrid split/expanded Browser UI

**Files:**
- Replace: `src/features/browser/BrowserPanel.tsx`
- Modify: `src/features/browser/BrowserPanel.test.tsx`
- Create: `src/features/browser/BrowserTabs.tsx`
- Create: `src/features/browser/BrowserToolbar.tsx`
- Create: `src/features/browser/TaskBrowserWorkspace.tsx`
- Create: `src/features/browser/TaskBrowserWorkspace.test.tsx`
- Modify: `src/App.tsx`
- Modify: `src/features/chat/ChatPanel.tsx`
- Modify: `src/styles/layout/browser.css`

- [ ] Add failing UI tests for tabs, address/search, controls, Attach menu, split handle, Expand/Return, compact composer, restoration notice, keyboard names, and streaming behavior.
- [ ] Render chat and Browser together for the selected task; Browser is no longer a mutually exclusive `activeView` destination.
- [ ] Measure the Browser host rectangle with `ResizeObserver` and send bounded geometry to Rust; hide child WebViews before layout transitions to prevent native-view bleed.
- [ ] In expanded mode keep the existing chat API/composer, transcript collapsed, and shelf/model state unchanged.
- [ ] Attach menu exposes only currently supported selection/page/screenshot actions and returns an explicit result notice.
- [ ] Re-run Browser/Chat/App suites and CSS restraint/reduced-motion tests.

### Task 6: Packaged WebKit proof and publication

- [ ] Update Browser IPC/safety/UI/feature docs and replace old separate-window smoke steps.
- [ ] Build `Plume Smoke.app`; prove two tasks keep different tabs/history, switching tears down stale views, split/expanded share the same workspace, casual public browsing works, project localhost requires exact-origin approval, and all three capture kinds attach once.
- [ ] Prove delete/fork/rewind/relaunch and Browser use during streaming.
- [ ] Run complete verification, publish the PR, and require exact-head review before merge.
