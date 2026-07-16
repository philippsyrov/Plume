# Browser Overlay Recovery Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make task-owned Browser startup and HTML overlays recover safely from failed or hung native suspension IPC without weakening native-webview isolation.

**Architecture:** `useTaskBrowser` becomes the owner of a small native-runtime readiness state. Suspension calls receive a bounded acknowledgement deadline; failure deactivates the native Browser before reporting the HTML layer safe, and a visible retry restarts the same task-owned runtime. `App` gates overlays on this broader “native layer is safe” signal rather than only the happy-path suspended echo.

**Tech Stack:** React 19, TypeScript, Vitest, Testing Library, Tauri 2 IPC, existing task-browser Rust commands.

## Global Constraints

- Keep Browser state owned by the exact local/project session identity.
- Never paint an HTML overlay while an active native Browser might still cover it.
- Do not add Browser authority, network policy, or new Rust IPC commands.
- Preserve the current task Browser workspace, tabs, history, and evidence semantics.
- Use focused tests before implementation and run the full project verifier before git publication.
- Keep the older `browser_sandbox_*` family as the documented Phase A capability-isolation proof; production task browsing continues through `task_browser_*`.

---

### Task 1: Fail-closed native runtime recovery

**Files:**
- Modify: `src/features/browser/useTaskBrowser.ts`
- Test: `src/features/browser/useTaskBrowser.test.tsx`

**Interfaces:**
- Consumes: existing `activateTaskBrowser`, `deactivateTaskBrowser`, `setTaskBrowserSuspended`, `SessionIdentity`.
- Produces: `TaskBrowserApi.overlaySafe: boolean`, `TaskBrowserApi.runtimeReady: boolean`, and `TaskBrowserApi.retryRuntime(): void`.

- [ ] **Step 1: Write failing mount and hung-suspension tests**

Add tests that use a deferred promise and fake timers:

```tsx
it('deactivates after mount-time suspension failure and can retry without remounting', async () => {
  mocks.suspended.mockRejectedValueOnce(new Error('native bridge unavailable'));
  const { result } = renderHook(() => useTaskBrowser(identity));

  await vi.waitFor(() => expect(mocks.deactivate).toHaveBeenCalledWith({ identity }));
  expect(result.current.runtimeReady).toBe(false);
  expect(result.current.overlaySafe).toBe(true);

  act(() => result.current.retryRuntime());
  await vi.waitFor(() => expect(mocks.activate).toHaveBeenCalledTimes(2));
  expect(result.current.runtimeReady).toBe(true);
});

it('times out a hung suspend and reports overlays safe only after deactivation', async () => {
  vi.useFakeTimers();
  const never = new Promise<void>(() => undefined);
  mocks.suspended.mockResolvedValueOnce(undefined).mockReturnValueOnce(never);
  const { result, rerender } = renderHook(
    ({ suspended }) => useTaskBrowser(identity, suspended),
    { initialProps: { suspended: false } },
  );
  await act(async () => Promise.resolve());

  rerender({ suspended: true });
  expect(result.current.overlaySafe).toBe(false);
  await act(async () => vi.advanceTimersByTimeAsync(SUSPENSION_ACK_TIMEOUT_MS));
  expect(mocks.deactivate).toHaveBeenCalledWith({ identity });
  expect(result.current.overlaySafe).toBe(true);
});
```

Export `SUSPENSION_ACK_TIMEOUT_MS` for the test. Add one rejection test where `deactivateTaskBrowser` also rejects and assert `overlaySafe` remains `false` because native safety was not confirmed.

- [ ] **Step 2: Run the focused tests and confirm the missing API fails**

Run:

```bash
npx vitest run src/features/browser/useTaskBrowser.test.tsx
```

Expected: FAIL because `overlaySafe`, `runtimeReady`, `retryRuntime`, and `SUSPENSION_ACK_TIMEOUT_MS` do not exist.

- [ ] **Step 3: Add the bounded runtime state machine**

Add the public fields and bounded acknowledgement helper:

```ts
export const SUSPENSION_ACK_TIMEOUT_MS = 1_500;

type RuntimeState = 'starting' | 'ready' | 'inactive' | 'unknown';

function withDeadline<T>(promise: Promise<T>, timeoutMs: number): Promise<T> {
  return new Promise<T>((resolve, reject) => {
    const handle = window.setTimeout(
      () => reject(new Error('Browser suspension acknowledgement timed out.')),
      timeoutMs,
    );
    promise.then(
      (value) => { window.clearTimeout(handle); resolve(value); },
      (error) => { window.clearTimeout(handle); reject(error); },
    );
  });
}
```

Extend `TaskBrowserApi`:

```ts
runtimeReady: boolean;
overlaySafe: boolean;
retryRuntime: () => void;
```

Inside the hook, hold `runtimeState` and a retry revision:

```ts
const [runtimeState, setRuntimeState] = useState<RuntimeState>('starting');
const [runtimeRetryRevision, setRuntimeRetryRevision] = useState(0);
```

Wrap every `setTaskBrowserSuspended` call with `withDeadline`. On successful activation plus suspension sync, set both `runtimeReadyRef.current = true` and `runtimeState = 'ready'`. On sync failure, set readiness false, await bounded `deactivateTaskBrowser({ identity })`, and set `runtimeState = 'inactive'` only after that acknowledgement. If deactivation also fails or times out, use `runtimeState = 'unknown'`.

Return:

```ts
runtimeReady: runtimeState === 'ready',
overlaySafe: suspended || runtimeState === 'inactive',
retryRuntime: () => setRuntimeRetryRevision((revision) => revision + 1),
```

Include `runtimeRetryRevision` in the mount effect dependency list so retry performs the same load → activate → suspend-sync → geometry sequence without changing task identity. Do not automatically loop after repeated failures.

- [ ] **Step 4: Run the focused hook tests**

Run:

```bash
npx vitest run src/features/browser/useTaskBrowser.test.tsx
```

Expected: all `useTaskBrowser` tests PASS, including the three new recovery cases.

- [ ] **Step 5: Commit the runtime recovery unit**

```bash
git add src/features/browser/useTaskBrowser.ts src/features/browser/useTaskBrowser.test.tsx
git commit -m "fix: recover task browser suspension failures"
```

### Task 2: Gate overlays on confirmed native safety

**Files:**
- Modify: `src/features/browser/BrowserPanel.tsx`
- Modify: `src/features/browser/TaskBrowserWorkspace.tsx`
- Modify: `src/App.tsx`
- Test: `src/features/browser/BrowserPanel.test.tsx`
- Test: `src/App.test.tsx`

**Interfaces:**
- Consumes: `TaskBrowserApi.overlaySafe`, `TaskBrowserApi.runtimeReady`, `TaskBrowserApi.retryRuntime()` from Task 1.
- Produces: `TaskBrowserWorkspace.onOverlaySafeChange?: (safe: boolean) => void`; `App` renders pending overlays after suspension or confirmed fail-closed deactivation.

- [ ] **Step 1: Write failing Browser and App recovery tests**

Update the App Browser stub to expose the broader callback:

```tsx
<button
  type="button"
  onClick={() =>
    (props.onOverlaySafeChange as ((safe: boolean) => void) | undefined)?.(true)
  }
>
  Confirm native Browser is safe
</button>
```

Add an App regression that opens Settings from Browser, confirms it is initially absent, sends `onOverlaySafeChange(true)`, and then sees Settings. Add a second assertion that `false` hides no already-open dialog until the next overlay request; overlay readiness is a handshake for the current request, not a global permission.

Add a BrowserPanel test with this fixture:

```tsx
mocks.browser = fixture({
  runtimeReady: false,
  overlaySafe: true,
  errorMessage: 'Browser paused after a native connection problem.',
  retryRuntime,
});
```

Assert a `Try Browser again` button is visible and calls `retryRuntime`.

- [ ] **Step 2: Run the focused tests and confirm they fail**

Run:

```bash
npx vitest run src/features/browser/BrowserPanel.test.tsx src/App.test.tsx
```

Expected: FAIL because `onOverlaySafeChange` and the retry control are absent.

- [ ] **Step 3: Wire the safe-overlay handshake and visible retry**

Rename the callback through `TaskBrowserWorkspace` and `BrowserPanel`:

```ts
onOverlaySafeChange?: ((safe: boolean) => void) | undefined;
```

Report the hook value:

```ts
useEffect(() => {
  onOverlaySafeChange?.(browser.overlaySafe);
}, [browser.overlaySafe, onOverlaySafeChange]);
```

In `App.tsx`, replace `browserSuspended` with `browserOverlaySafe`, keep the overlay-request boolean, and derive:

```ts
const htmlOverlayReady = !browserActive || browserOverlaySafe;
```

Reset `browserOverlaySafe` to `false` when entering a Browser view or changing the Browser session key so a prior task's acknowledgement cannot authorize a new task's overlay.

When `browser.runtimeReady` is false and its native layer is confirmed inactive, render this action in the existing Browser notice row:

```tsx
<button type="button" onClick={browser.retryRuntime}>
  Try Browser again
</button>
```

Keep ordinary navigation, capture, and tab actions disabled until `runtimeReady` is true.

- [ ] **Step 4: Run Browser and App tests**

Run:

```bash
npx vitest run src/features/browser/BrowserPanel.test.tsx src/App.test.tsx
```

Expected: PASS with the existing happy suspension handshake and new fail-closed recovery both covered.

- [ ] **Step 5: Commit the overlay recovery unit**

```bash
git add src/features/browser/BrowserPanel.tsx src/features/browser/TaskBrowserWorkspace.tsx src/App.tsx src/features/browser/BrowserPanel.test.tsx src/App.test.tsx
git commit -m "fix: recover browser overlays safely"
```

### Task 3: Normalize restored geometry and stale capture errors

**Files:**
- Modify: `src/features/browser/BrowserPanel.tsx`
- Modify: `src/features/browser/useTaskBrowser.ts`
- Test: `src/features/browser/BrowserPanel.test.tsx`
- Test: `src/features/browser/useTaskBrowser.test.tsx`

**Interfaces:**
- Consumes: existing `BrowserWorkspace.splitWidthPx`, `TaskBrowserApi.setSplitWidth`, and hook generation counter.
- Produces: container-normalized persisted split widths and identity-safe capture error reporting.

- [ ] **Step 1: Extend the existing width test and add stale-capture tests**

Change `clamps a large restored split width` to use a spy and wait for persistence:

```tsx
const setSplitWidth = vi.fn().mockResolvedValue(true);
mocks.browser = fixture({ workspace, setSplitWidth });
render(<BrowserPanel identity={identity} chatPane={null} onUseInChat={vi.fn()} />);
expect(screen.getByLabelText('Browser')).toHaveStyle('--plume-browser-split-width: 532px');
await vi.waitFor(() => expect(setSplitWidth).toHaveBeenCalledWith(532));
```

Add one hook test for text and one for screenshot capture: start a deferred rejected capture, rerender with a new identity, reject the old promise, and assert the new mount's `errorMessage` remains unchanged.

- [ ] **Step 2: Run the focused tests and confirm failure**

Run:

```bash
npx vitest run src/features/browser/BrowserPanel.test.tsx src/features/browser/useTaskBrowser.test.tsx
```

Expected: width persistence and stale-capture assertions FAIL.

- [ ] **Step 3: Measure before paint and persist only a real clamp**

Use `useLayoutEffect` for the initial root measurement so the restored descriptor is clamped before paint:

```ts
useLayoutEffect(() => {
  const root = rootRef.current;
  if (!root) return;
  const report = () => {
    const width = root.getBoundingClientRect().width;
    if (width > 0) setContainerWidth(width);
  };
  report();
  const observer = new ResizeObserver(report);
  observer.observe(root);
  return () => observer.disconnect();
}, []);
```

Add a guarded persistence effect:

```ts
useEffect(() => {
  const stored = browser.workspace?.splitWidthPx;
  if (expanded || dragWidth !== null || containerWidth === null || stored === undefined) return;
  if (stored === splitWidth) return;
  void browser.setSplitWidth(splitWidth);
}, [browser.workspace?.splitWidthPx, browser.setSplitWidth, containerWidth, dragWidth, expanded, splitWidth]);
```

Because the successful save updates `workspace.splitWidthPx`, the equality guard stops repeats.

- [ ] **Step 4: Generation-guard capture failures**

Capture the current generation before each request and guard both success and failure:

```ts
const generation = generationRef.current;
try {
  const captured = await captureTaskBrowserText({ identity, tabId, captureKind });
  if (generation !== generationRef.current) return { kind: 'failed' } as const;
  return { kind: 'captured', ...captured } as const;
} catch (error) {
  if (generation === generationRef.current) setErrorMessage(productError(error));
  return { kind: 'failed' } as const;
}
```

Apply the same shape to screenshot capture.

- [ ] **Step 5: Run the focused Browser tests**

Run:

```bash
npx vitest run src/features/browser/BrowserPanel.test.tsx src/features/browser/useTaskBrowser.test.tsx
```

Expected: PASS.

- [ ] **Step 6: Commit geometry and stale-state hardening**

```bash
git add src/features/browser/BrowserPanel.tsx src/features/browser/BrowserPanel.test.tsx src/features/browser/useTaskBrowser.ts src/features/browser/useTaskBrowser.test.tsx
git commit -m "fix: normalize restored browser state"
```

### Task 4: Document deliberate boundaries and verify the PR

**Files:**
- Modify: `src-tauri/src/app_commands.rs`
- Modify: `docs/SAFETY.md`
- Modify: `docs/FEATURE_INVENTORY.md`
- Modify: `docs/SMOKE_TESTING.md`

**Interfaces:**
- Consumes: the shipped Phase A isolation proof and current untrusted-project metadata behavior.
- Produces: explicit maintenance intent and packaged-smoke recovery steps.

- [ ] **Step 1: Add the dead-surface intent comment**

Immediately above the `browser_sandbox_*` entries in `APP_COMMANDS`, add:

```rust
// Deliberately retained Phase A capability-isolation proof. The consumer
// task-owned Browser uses `task_browser_*`; these commands remain registered
// for the zero-authority sandbox contract and must match no capability file.
```

Do not add a capability grant or production frontend caller.

- [ ] **Step 2: Record the pre-trust metadata decision**

In `docs/SAFETY.md`, state that after a user explicitly selects a folder, Plume may report only marker-file existence (`AGENTS.md`, `CLAUDE.md`) and package-manager signal files before trust. File contents, git state, recursive enumeration, prompt context, and project tools remain blocked until explicit trust.

- [ ] **Step 3: Update shipped behavior and smoke steps**

Update the Browser inventory record with fail-closed suspension recovery and add packaged smoke steps that:

1. open Browser for a task;
2. open Settings, Help, Workspace views, and a session dialog and confirm each
   waits for native suspension, appears above the Browser, and preserves tabs
   after close;
3. relaunch at a narrower width and confirm the split descriptor stays
   normalized.

The rejection and never-resolving IPC paths stay deterministic unit tests. Do
not ship a production fault-injection flag merely to make packaged smoke force
those states.

- [ ] **Step 4: Run focused and full verification**

Run:

```bash
npx vitest run src/features/browser/useTaskBrowser.test.tsx src/features/browser/BrowserPanel.test.tsx src/App.test.tsx
PLUME_FULL_VERIFY=1 ./scripts/verify.sh
```

Expected: focused tests PASS; full verifier reports `36 pass / 2 existing documentation soft-cap warnings / 0 fail`.

- [ ] **Step 5: Perform packaged macOS smoke and exact-head review**

Build and run the packaged smoke app. Exercise Settings, Help, Workspace views,
and session dialogs from an active Browser. Record exact screenshots and
outcomes in the PR; do not treat unit tests as proof of normal native layer
ordering, and do not claim packaged proof of the injected failure paths.

- [ ] **Step 6: Commit documentation**

```bash
git add src-tauri/src/app_commands.rs docs/SAFETY.md docs/FEATURE_INVENTORY.md docs/SMOKE_TESTING.md
git commit -m "docs: record browser recovery boundaries"
```

The PR is ready only after the branch head, GitHub verify, gitleaks, packaged smoke, and an independent findings-only review all agree.
