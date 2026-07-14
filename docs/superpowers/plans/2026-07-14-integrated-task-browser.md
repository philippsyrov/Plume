# Integrated Task Browser Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Put a five-tab WebKit Browser inside each selected chat task, split beside chat by default and expandable to the task canvas with a compact composer.

**Architecture:** Replace the global separate Browser window with up to five capability-isolated Tauri child WebViews attached to the main window for the selected session. A Rust runtime manager owns child labels, geometry, navigation generations, and lifecycle; React owns visible task layout and persists descriptors through PR 1's workspace API. Exact identity checks guard every async navigation/capture completion.

**Tech Stack:** Tauri 2 child WebViews, macOS WebKit, Rust, TypeScript, React 19, ResizeObserver, Vitest, packaged-app smoke.

## Global Constraints

- No iframe fallback, popup Browser, Chromium, DevTools, extensions, agent navigation, or automatic attachment.
- Child WebViews match no application capability and cannot invoke Plume commands.
- Child WebViews use WebKit's ordinary app-owned persistent website-data store. Do not set `incognito(true)` and do not add cookie read/export APIs.
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

**Interfaces:**
- `BrowserRuntimeIdentity { scope, session_id }` and `LiveTabIdentity { workspace, tab_id, generation }`.
- `BrowserChildPlan { label, position, size, visible, persistent_data_store: true }`.
- `BrowserRuntimePort` trait isolates `add_child`, geometry, show/hide, eval, reload, and close for unit tests.

```rust
pub(crate) trait BrowserRuntimePort {
    fn add_child(&self, plan: &BrowserChildPlan) -> Result<(), BrowserRuntimeError>;
    fn set_bounds(&self, label: &str, bounds: BrowserBounds) -> Result<(), BrowserRuntimeError>;
    fn set_visible(&self, label: &str, visible: bool) -> Result<(), BrowserRuntimeError>;
    fn close(&self, label: &str) -> Result<(), BrowserRuntimeError>;
}
```

- [ ] Add compile-level/unit tests for deterministic child labels, main-window-only parent, bounded geometry, inactive-tab visibility, persistent profile mode, and absence of any cookie/session serialization field.
- [ ] Run `cd src-tauri && cargo test browser::runtime_tests -- --nocapture`; expected RED is unresolved `BrowserRuntimeManager`/`BrowserRuntimePort`.
- [ ] Implement a minimal `BrowserRuntimeManager` around `Window::add_child(WebviewBuilder, position, size)` with lifecycle behind a trait so pure planning is testable.
- [ ] Preserve extension/autofill/devtools and popup/download denial policy from the current sandbox builder, but deliberately remove `.incognito(true)` so all task tabs use one app-owned persistent WebKit profile.
- [ ] Run `cd src-tauri && cargo test browser::runtime_tests -- --nocapture` and `cd src-tauri && cargo check`; expected GREEN includes compilation against the real pinned Tauri API. Do not continue if the native child seam cannot compile.
- [ ] Commit: `feat: prove embedded webkit runtime`.

### Task 2: Session activation, tabs, navigation, and geometry commands

**Files:**
- Modify: `src-tauri/src/browser/runtime.rs`
- Create: `src-tauri/src/commands/task_browser.rs`
- Create: `src-tauri/src/commands/task_browser_tests.rs`
- Modify: `src-tauri/src/commands/mod.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/capabilities/default.json`

**Interfaces:**
- `task_browser_activate({ identity, tabs, activeTabId })`.
- `task_browser_deactivate({ identity })`.
- `task_browser_open_tab`, `task_browser_close_tab`, `task_browser_select_tab`.
- `task_browser_navigate({ identity, tabId, url, approvedLoopbackOrigin? })`.
- `task_browser_back|forward|reload({ identity, tabId })`.
- `task_browser_set_geometry({ identity, host: { x, y, width, height, scaleFactor } })`.

Every mutating handler follows the same identity guard:

```rust
let requested = SessionIdentity::validate(req.payload.identity)?;
let selected = runtime.selected_identity().ok_or_else(not_found)?;
if requested != selected { return Err(IpcError::NotFound("browser.task".into())); }
```

- [ ] Write failing command tests for activate/deactivate, open/close/select tab, navigate/back/forward/reload, five-tab cap, stale session/tab ids, geometry bounds, and command registry parity.
- [ ] Run `cd src-tauri && cargo test commands::task_browser_tests -- --nocapture`; expected RED is missing commands/registry entries.
- [ ] Implement commands keyed by `{scope, sessionId}` plus tab id where needed; resolve scope server-side and recheck selected identity before mutation.
- [ ] Serialize runtime operations; destroy all child WebViews on selected-session deletion, project close/switch, app teardown, or runtime reset.
- [ ] Make page callbacks append only admitted top-level URLs through PR 1's store and discard stale generations.
- [ ] Re-run command/runtime tests and clippy.
- [ ] Commit: `feat: bind browser runtime to sessions`.

### Task 3: Casual and project evidence capture

**Files:**
- Modify: `src-tauri/src/commands/task_browser.rs`
- Modify: `src-tauri/src/browser/evidence.rs`
- Modify: `src-tauri/src/browser/screenshot_evidence.rs`
- Modify: `src-tauri/src/browser/native_snapshot.rs`
- Modify: `src-tauri/src/prompts/explicit_context.rs`
- Modify: `src-tauri/src/commands/chat/context.rs`
- Modify: `src-tauri/src/commands/chat/send.rs`
- Modify: `src-tauri/src/sessions/validation.rs`
- Modify: `src-tauri/src/sessions/mod.rs`
- Modify: `src-tauri/src/commands/sessions.rs`
- Modify: `src/lib/api/chat.ts`
- Modify: `src/lib/api/sessions.ts`
- Modify: `src/features/chat/useChat.ts`
- Modify: `src/features/chat/useChat.test.tsx`
- Modify: `src/features/sessions/usePersistedChat.ts`
- Modify: `src/features/sessions/usePersistedChat.test.tsx`
- Test: existing Browser evidence, prompt, and chat command suites.

**Interfaces:**
- `ContextOwner = { scope: 'local' | 'project'; sessionId: string }` travels with `chat.context` and `chat.send` only when explicit session-owned context exists.
- Browser refs remain opaque ids; Browser manifest variants gain `ownerScope` and `ownerSessionId` for persisted boundary validation.
- A local shelf may contain only Browser text/screenshot refs owned by that exact local session.
- A project shelf retains existing file/memory/topic/Browser refs and trust requirements.
- `useChat` must not erase local Browser refs merely because `includeProjectContext === false`; it sends them with exact `contextOwner`.

```ts
export type ContextOwner = SessionIdentity;

export type ChatSendPayload = ExistingChatSendPayload & {
  contextOwner?: ContextOwner;
  contextSources?: ContextSourceRef[];
};
```

```rust
match (owner.scope, source) {
    (SessionScope::Local, ContextSourceRef::BrowserText { .. }
      | ContextSourceRef::BrowserScreenshot { .. }) => resolve_local_owned(owner, source),
    (SessionScope::Local, _) => Err(IpcError::NeedsApproval),
    (SessionScope::Project, _) => resolve_trusted_project_owned(owner, source),
}
```

- [ ] Add failing tests proving local captures land only in the session-private store and project captures only in the trusted project store.
- [ ] Add failing tests for local preview/send/persist/relaunch, foreign-local-id `NotFound`, project-ref `NeedsApproval`, project behavior unchanged, and legacy local rows unchanged.
- [ ] Run `cd src-tauri && cargo test commands::chat -- --nocapture`, `cd src-tauri && cargo test sessions::context_tests -- --nocapture`, and `npm run test -- src/features/chat/useChat.test.tsx src/features/sessions/usePersistedChat.test.tsx`; expected RED is the current trust gate/local-source rejection.
- [ ] Extend Browser evidence refs/manifests with owning scope/session metadata without exposing paths; old project records remain readable.
- [ ] Bind capture tickets to `{scope, sessionId, tabId, pageGeneration, currentUrl}` and recheck identity after callback/image encoding.
- [ ] Delete a casual chat's evidence on session delete; never delete evidence still referenced by that session's historical accepted-turn manifests before the session itself is deleted.
- [ ] Change command validation from “any explicit source requires trust” to a tagged rule: exact-owner local Browser refs are allowed without trust; every project file/memory/topic/project-Browser ref still requires the trusted matching project.
- [ ] Change session validation from “local manifests/shelves always empty” to “local shelves/manifests contain only exact-owner Browser variants”; reject project kinds, foreign local session ids, and mixed ownership as corrupt/bad input.
- [ ] Ensure `usePersistedChat` creates/commits a local session before Browser activation, so no evidence is ever owned by a draft/null identity.
- [ ] Re-run `cd src-tauri && cargo test commands::chat -- --nocapture`, `cd src-tauri && cargo test sessions -- --nocapture`, and `npm run test -- src/features/chat/useChat.test.tsx src/features/sessions/usePersistedChat.test.tsx`; expected GREEN proves the complete local/project path.
- [ ] Commit: `feat: allow session-owned browser context`.

### Task 4: Frontend task Browser state hook

**Files:**
- Replace: `src/features/browser/useBrowserWorkspace.ts`
- Modify: `src/features/browser/useBrowserWorkspace.test.tsx`
- Modify: `src/lib/api/browser.ts`
- Consume: `src/lib/api/browserWorkspace.ts`

**Interfaces:**
- `useBrowserWorkspace(identity: SessionIdentity | null): TaskBrowserApi`.
- `TaskBrowserApi` exposes `workspace`, `runtime`, `openTab`, `closeTab`, `selectTab`, `navigate`, `back`, `forward`, `reload`, `setLayout`, `setSplitWidth`, `setGeometry`, `capture`, and `notice`.
- `usePersistedChat.surfaceIdentity(): SessionIdentity | null` replaces the current anonymous return type and returns null until a persisted session exists.

The stale-response test must switch identity while `browserWorkspaceLoad` is pending and prove the old completion is ignored:

```ts
expect(result.current.workspace).toBeNull();
oldLoad.resolve(workspaceFor('old'));
await flushPromises();
expect(result.current.identity).toEqual(newIdentity);
expect(result.current.workspace).not.toEqual(workspaceFor('old'));
```

- [ ] Write failing hook tests for initial activation, five tabs, selection, close fallback, history, relaunch restore, split width/layout, corrupt reset notice, stale task switch, and unmount cleanup.
- [ ] Run `npm run test -- src/features/browser/useBrowserWorkspace.test.tsx`; expected RED is the global/no-identity hook contract.
- [ ] Implement one hook keyed by exact `SessionIdentity`; clear old visible state immediately on identity change.
- [ ] Queue descriptor saves and compare identity again after every awaited IPC call.
- [ ] Model restore honestly: descriptors return immediately; runtime reports reloading/manual reopen/failure separately.
- [ ] Run `npm run test -- src/features/browser/useBrowserWorkspace.test.tsx` and `npm run typecheck`; expected GREEN includes every identity/race/restoration case.
- [ ] Commit: `feat: own browser state per task`.

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

**Interfaces:**
- `TaskBrowserWorkspace({ identity, chat, browser, projectAvailable })` owns split/expanded composition.
- `BrowserToolbar` emits typed actions only; it never reads WebView contents.
- `BrowserHostRect` is measured in CSS pixels and paired with `window.devicePixelRatio` for Rust geometry conversion.

- [ ] Add failing UI tests for tabs, address/search, controls, Attach menu, split handle, Expand/Return, compact composer, restoration notice, keyboard names, and streaming behavior.
- [ ] Run `npm run test -- src/features/browser/TaskBrowserWorkspace.test.tsx src/features/browser/BrowserPanel.test.tsx`; expected RED is missing split/expanded controls and task-owned props.
- [ ] Render chat and Browser together for the selected task; Browser is no longer a mutually exclusive `activeView` destination.
- [ ] Measure the Browser host rectangle with `ResizeObserver` and send bounded geometry to Rust; hide child WebViews before layout transitions to prevent native-view bleed.
- [ ] In expanded mode keep the existing chat API/composer, transcript collapsed, and shelf/model state unchanged.
- [ ] Attach menu exposes only currently supported selection/page/screenshot actions and returns an explicit result notice.
- [ ] Re-run `npm run test -- src/features/browser/TaskBrowserWorkspace.test.tsx src/features/browser/BrowserPanel.test.tsx src/features/browser/useBrowserWorkspace.test.tsx src/App.test.tsx src/features/chat/ChatPanel.test.tsx`; expected GREEN includes CSS restraint/reduced-motion assertions.
- [ ] Commit: `feat: integrate browser into task canvas`.

### Task 6: Packaged WebKit proof and publication

- [ ] Update Browser IPC/safety/UI/feature docs and replace old separate-window smoke steps.
- [ ] Build `Plume Smoke.app`; prove two tasks keep different tabs/history, switching tears down stale views, split/expanded share the same workspace, casual public browsing works, project localhost requires exact-origin approval, and all three capture kinds attach once.
- [ ] Prove delete/fork/rewind/relaunch and Browser use during streaming.
- [ ] Run `cd src-tauri && cargo test`, `npm run test -- src/features/browser/useBrowserWorkspace.test.tsx src/features/browser/BrowserPanel.test.tsx src/features/browser/TaskBrowserWorkspace.test.tsx src/features/chat/useChat.test.tsx src/features/sessions/usePersistedChat.test.tsx`, `npm run typecheck`, and `PLUME_FULL_VERIFY=1 ./scripts/verify.sh`; require zero failures.
- [ ] Publish the PR, wait for GitHub verify and gitleaks, and require findings-only exact-head review before merge.
