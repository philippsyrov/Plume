import { act, renderHook } from '@testing-library/react';
import { StrictMode, type ReactNode } from 'react';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import type { BrowserWorkspace } from '../../lib/api/browserWorkspace';
import type { SessionIdentity } from '../../lib/api/sessions';
import { SUSPENSION_ACK_TIMEOUT_MS, useTaskBrowser } from './useTaskBrowser';

const mocks = vi.hoisted(() => ({
  load: vi.fn(),
  reset: vi.fn(),
  save: vi.fn(),
  activate: vi.fn(),
  deactivate: vi.fn(),
  navigate: vi.fn(),
  back: vi.fn(),
  forward: vi.fn(),
  reload: vi.fn(),
  openTab: vi.fn(),
  closeTab: vi.fn(),
  selectTab: vi.fn(),
  geometry: vi.fn(),
  suspended: vi.fn(),
  captureText: vi.fn(),
  captureScreenshot: vi.fn(),
}));

vi.mock('../../lib/api/browserWorkspace', async (importOriginal) => ({
  ...(await importOriginal<typeof import('../../lib/api/browserWorkspace')>()),
  loadBrowserWorkspace: mocks.load,
  resetBrowserWorkspace: mocks.reset,
  saveBrowserWorkspace: mocks.save,
  activateTaskBrowser: mocks.activate,
  deactivateTaskBrowser: mocks.deactivate,
  navigateTaskBrowser: mocks.navigate,
  backTaskBrowser: mocks.back,
  forwardTaskBrowser: mocks.forward,
  reloadTaskBrowser: mocks.reload,
  openTaskBrowserTab: mocks.openTab,
  closeTaskBrowserTab: mocks.closeTab,
  selectTaskBrowserTab: mocks.selectTab,
  setTaskBrowserGeometry: mocks.geometry,
  setTaskBrowserSuspended: mocks.suspended,
  captureTaskBrowserText: mocks.captureText,
  captureTaskBrowserScreenshot: mocks.captureScreenshot,
}));

const identity = { scope: 'project' as const, sessionId: `s_${'a'.repeat(32)}` };

describe('useTaskBrowser', () => {
  beforeEach(() => {
    vi.useRealTimers();
    vi.clearAllMocks();
    const workspace = fixture();
    mocks.load.mockResolvedValue({ workspace, recoveryNotice: null });
    mocks.reset.mockResolvedValue({ workspace });
    mocks.save.mockImplementation(async ({ workspace: next }) => ({ workspace: next }));
    for (const mock of [
      mocks.activate,
      mocks.deactivate,
      mocks.navigate,
      mocks.back,
      mocks.forward,
      mocks.reload,
      mocks.openTab,
      mocks.closeTab,
      mocks.selectTab,
      mocks.geometry,
      mocks.suspended,
    ]) {
      mock.mockResolvedValue(undefined);
    }
  });

  it('suspends and resumes the live native Browser without deactivating it', async () => {
    const { result, rerender } = renderHook(
      ({ suspended }) => useTaskBrowser(identity, suspended),
      { initialProps: { suspended: false } },
    );
    await act(async () => Promise.resolve());

    rerender({ suspended: true });
    await act(async () => Promise.resolve());
    expect(mocks.suspended).toHaveBeenLastCalledWith({ identity, suspended: true });
    expect(result.current.suspended).toBe(true);
    expect(mocks.deactivate).not.toHaveBeenCalled();

    rerender({ suspended: false });
    await act(async () => Promise.resolve());
    expect(mocks.suspended).toHaveBeenLastCalledWith({ identity, suspended: false });
    expect(result.current.suspended).toBe(false);
    expect(mocks.deactivate).not.toHaveBeenCalled();
  });

  it('resumes when suspension is cancelled while the initial suspend is in flight', async () => {
    let finishSuspend!: () => void;
    mocks.suspended.mockReturnValueOnce(new Promise<void>((resolve) => {
      finishSuspend = resolve;
    }));

    const { result, rerender } = renderHook(
      ({ suspended }) => useTaskBrowser(identity, suspended),
      { initialProps: { suspended: true } },
    );
    await act(async () => Promise.resolve());
    expect(mocks.suspended).toHaveBeenCalledWith({ identity, suspended: true });

    rerender({ suspended: false });
    await act(async () => { finishSuspend(); });
    await act(async () => Promise.resolve());

    expect(mocks.suspended).toHaveBeenLastCalledWith({ identity, suspended: false });
    expect(result.current.suspended).toBe(false);
  });

  it('serializes rapid suspension changes after the Browser is ready', async () => {
    const { result, rerender } = renderHook(
      ({ suspended }) => useTaskBrowser(identity, suspended),
      { initialProps: { suspended: false } },
    );
    await act(async () => Promise.resolve());
    mocks.suspended.mockClear();

    let finishSuspend!: () => void;
    mocks.suspended.mockReturnValueOnce(new Promise<void>((resolve) => {
      finishSuspend = resolve;
    }));
    rerender({ suspended: true });
    await act(async () => Promise.resolve());
    rerender({ suspended: false });

    await act(async () => { finishSuspend(); });
    await act(async () => Promise.resolve());

    expect(mocks.suspended).toHaveBeenCalledWith({ identity, suspended: true });
    expect(mocks.suspended).toHaveBeenLastCalledWith({ identity, suspended: false });
    expect(result.current.suspended).toBe(false);
  });

  it('deactivates after mount-time suspension failure and can retry without remounting', async () => {
    mocks.suspended.mockRejectedValueOnce(new Error('native bridge unavailable'));
    const { result } = renderHook(() => useTaskBrowser(identity));

    await vi.waitFor(() => expect(mocks.deactivate).toHaveBeenCalledWith({ identity }));
    expect(result.current.runtimeReady).toBe(false);
    expect(result.current.overlaySafe).toBe(true);

    act(() => result.current.retryRuntime());
    await vi.waitFor(() => expect(mocks.activate).toHaveBeenCalledTimes(2));
    await vi.waitFor(() => expect(result.current.runtimeReady).toBe(true));
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

  it('keeps overlays unsafe when suspension and fallback deactivation both fail', async () => {
    mocks.suspended.mockRejectedValueOnce(new Error('native bridge unavailable'));
    mocks.deactivate.mockRejectedValueOnce(new Error('native bridge unavailable'));
    const { result } = renderHook(() => useTaskBrowser(identity));

    await vi.waitFor(() => expect(mocks.deactivate).toHaveBeenCalledWith({ identity }));
    await vi.waitFor(() => expect(result.current.busy).toBe(false));
    expect(result.current.runtimeReady).toBe(false);
    expect(result.current.overlaySafe).toBe(false);
  });

  it('invalidates an acknowledged suspension when resume and fallback deactivation fail', async () => {
    const { result, rerender } = renderHook(
      ({ suspended }) => useTaskBrowser(identity, suspended),
      { initialProps: { suspended: true } },
    );
    await vi.waitFor(() => expect(result.current.runtimeReady).toBe(true));
    expect(result.current.suspended).toBe(true);
    expect(result.current.overlaySafe).toBe(true);

    mocks.suspended.mockRejectedValueOnce(new Error('native bridge unavailable'));
    mocks.deactivate.mockRejectedValueOnce(new Error('native bridge unavailable'));
    rerender({ suspended: false });

    await vi.waitFor(() => expect(mocks.deactivate).toHaveBeenCalledWith({ identity }));
    await vi.waitFor(() => expect(result.current.runtimeReady).toBe(false));
    expect(result.current.overlaySafe).toBe(false);
  });

  it('restores and activates only the exact session descriptor', async () => {
    const { result, unmount } = renderHook(() => useTaskBrowser(identity));
    await act(async () => Promise.resolve());

    expect(result.current.workspace?.sessionId).toBe(identity.sessionId);
    expect(mocks.activate).toHaveBeenCalledWith({
      identity,
      tabs: [
        {
          tabId: `bt_${'b'.repeat(32)}`,
          url: 'https://example.com/',
          manualReopenRequired: false,
        },
      ],
      activeTabId: `bt_${'b'.repeat(32)}`,
    });

    unmount();
    await act(async () => new Promise((resolve) => window.setTimeout(resolve, 60)));
    expect(mocks.deactivate).toHaveBeenCalledWith({ identity });
  });

  it('preserves the initial recovery notice when a reset supplies the workspace', async () => {
    mocks.load.mockResolvedValueOnce({ workspace: null, recoveryNotice: 'browserStateReset' });
    mocks.reset.mockResolvedValueOnce({ workspace: fixture() });

    const { result } = renderHook(() => useTaskBrowser(identity));
    await act(async () => Promise.resolve());

    expect(result.current.workspace).not.toBeNull();
    expect(result.current.recoveryNotice).toBe('browserStateReset');
  });

  it('preserves a destructive recovery notice across Strict Mode effect replay', async () => {
    const replacementWorkspace = fixture(identity, 'c');
    const staleWorkspace = fixture(identity, 'd');
    let persistedWorkspace: BrowserWorkspace | null = null;
    let release!: (value: { workspace: null; recoveryNotice: 'browserStateReset' }) => void;
    mocks.load
      .mockReturnValueOnce(new Promise((resolve) => { release = resolve; }))
      .mockResolvedValue({ workspace: null, recoveryNotice: null });
    let resetCount = 0;
    mocks.reset.mockImplementation(async () => {
      const workspace = resetCount++ === 0 ? replacementWorkspace : staleWorkspace;
      persistedWorkspace = workspace;
      return { workspace };
    });

    const { result } = renderHook(() => useTaskBrowser(identity), { reactStrictMode: true });
    await act(async () => Promise.resolve());
    expect(mocks.load).toHaveBeenCalledTimes(2);
    expect(result.current.recoveryNotice).toBeNull();
    expect(result.current.workspace?.activeTabId).toBe(replacementWorkspace.activeTabId);

    await act(async () => { release({ workspace: null, recoveryNotice: 'browserStateReset' }); });

    expect(result.current.recoveryNotice).toBe('browserStateReset');
    expect(mocks.reset).toHaveBeenCalledTimes(1);
    expect(persistedWorkspace).toBe(replacementWorkspace);
    expect(result.current.workspace?.activeTabId).toBe(replacementWorkspace.activeTabId);
    expect(mocks.activate).toHaveBeenCalledTimes(1);
    expect(mocks.activate).toHaveBeenCalledWith(expect.objectContaining({
      tabs: [expect.objectContaining({ tabId: replacementWorkspace.activeTabId })],
      activeTabId: replacementWorkspace.activeTabId,
    }));
  });

  it('consumes recovery handed off while replacement activation is pending', async () => {
    const owner: SessionIdentity = { scope: 'local', sessionId: `s_${'g'.repeat(32)}` };
    const replacementWorkspace = fixture(owner, 'e');
    let releaseLoad!: (value: { workspace: null; recoveryNotice: 'browserStateReset' }) => void;
    let finishActivation!: () => void;
    mocks.load
      .mockReturnValueOnce(new Promise((resolve) => { releaseLoad = resolve; }))
      .mockResolvedValueOnce({ workspace: null, recoveryNotice: null })
      .mockResolvedValue({ workspace: replacementWorkspace, recoveryNotice: null });
    mocks.reset.mockResolvedValue({ workspace: replacementWorkspace });
    mocks.activate
      .mockReturnValueOnce(new Promise<void>((resolve) => { finishActivation = resolve; }))
      .mockResolvedValue(undefined);

    const first = renderHook(() => useTaskBrowser(owner), { reactStrictMode: true });
    await act(async () => Promise.resolve());
    expect(mocks.activate).toHaveBeenCalledTimes(1);

    await act(async () => {
      releaseLoad({ workspace: null, recoveryNotice: 'browserStateReset' });
    });
    expect(first.result.current.recoveryNotice).toBe('browserStateReset');

    await act(async () => { finishActivation(); });
    first.unmount();

    const second = renderHook(() => useTaskBrowser(owner));
    await act(async () => Promise.resolve());

    expect(second.result.current.recoveryNotice).toBeNull();
  });

  it('retains recovery when replacement activation fails', async () => {
    const owner: SessionIdentity = { scope: 'local', sessionId: `s_${'h'.repeat(32)}` };
    const workspace = fixture(owner, 'f');
    mocks.load
      .mockResolvedValueOnce({ workspace, recoveryNotice: 'browserStateReset' })
      .mockResolvedValueOnce({ workspace, recoveryNotice: null });
    mocks.activate
      .mockRejectedValueOnce(new Error('activation failed'))
      .mockResolvedValueOnce(undefined);

    const first = renderHook(() => useTaskBrowser(owner));
    await act(async () => Promise.resolve());
    expect(first.result.current.recoveryNotice).toBe('browserStateReset');
    expect(first.result.current.errorMessage).toBe('Browser unavailable. Try again.');
    first.unmount();

    const second = renderHook(() => useTaskBrowser(owner));
    await act(async () => Promise.resolve());

    expect(second.result.current.recoveryNotice).toBe('browserStateReset');
  });

  it('keeps the recovery notice visible when reset fails', async () => {
    const owner: SessionIdentity = { scope: 'local', sessionId: `s_${'e'.repeat(32)}` };
    mocks.load.mockResolvedValueOnce({ workspace: null, recoveryNotice: 'browserStateReset' });
    mocks.reset.mockRejectedValueOnce(new Error('reset failed'));

    const { result } = renderHook(() => useTaskBrowser(owner));
    await act(async () => Promise.resolve());

    expect(result.current.recoveryNotice).toBe('browserStateReset');
    expect(result.current.errorMessage).toBe('Browser unavailable. Try again.');
  });

  it('hands an unsurfaced recovery notice to a later mount of the same identity', async () => {
    const owner: SessionIdentity = { scope: 'local', sessionId: `s_${'f'.repeat(32)}` };
    mocks.load
      .mockResolvedValueOnce({ workspace: null, recoveryNotice: 'browserStateReset' })
      .mockResolvedValueOnce({ workspace: null, recoveryNotice: null });
    mocks.reset
      .mockRejectedValueOnce(new Error('reset failed'))
      .mockResolvedValueOnce({ workspace: fixture(owner) });

    const first = renderHook(() => useTaskBrowser(owner));
    await act(async () => Promise.resolve());
    first.unmount();

    const second = renderHook(() => useTaskBrowser(owner));
    await act(async () => Promise.resolve());

    expect(second.result.current.recoveryNotice).toBe('browserStateReset');
  });

  it('clears a stale recovery notice before loading a replacement identity', async () => {
    const replacement: SessionIdentity = { scope: 'local', sessionId: `s_${'d'.repeat(32)}` };
    let release!: (value: { workspace: BrowserWorkspace; recoveryNotice: null }) => void;
    mocks.load
      .mockResolvedValueOnce({ workspace: null, recoveryNotice: 'browserStateReset' })
      .mockReturnValueOnce(new Promise((resolve) => { release = resolve; }));

    const { result, rerender } = renderHook(
      ({ owner }) => useTaskBrowser(owner),
      { initialProps: { owner: identity as SessionIdentity } },
    );
    await act(async () => Promise.resolve());
    expect(result.current.recoveryNotice).toBe('browserStateReset');

    rerender({ owner: replacement });
    expect(result.current.recoveryNotice).toBeNull();

    await act(async () => { release({ workspace: fixture(replacement), recoveryNotice: null }); });
  });

  it('does not let a stale initial load publish its recovery notice', async () => {
    const replacement: SessionIdentity = { scope: 'local', sessionId: `s_${'d'.repeat(32)}` };
    let release!: (value: { workspace: null; recoveryNotice: 'browserStateReset' }) => void;
    mocks.load
      .mockReturnValueOnce(new Promise((resolve) => { release = resolve; }))
      .mockResolvedValueOnce({ workspace: fixture(replacement), recoveryNotice: null });

    const { result, rerender } = renderHook(
      ({ owner }) => useTaskBrowser(owner),
      { initialProps: { owner: identity as SessionIdentity } },
    );
    rerender({ owner: replacement });
    await act(async () => Promise.resolve());

    await act(async () => { release({ workspace: null, recoveryNotice: 'browserStateReset' }); });
    expect(result.current.recoveryNotice).toBeNull();
  });

  it('keeps the initial recovery notice across ordinary refreshes', async () => {
    mocks.load
      .mockResolvedValueOnce({ workspace: null, recoveryNotice: 'browserStateReset' })
      .mockResolvedValue({ workspace: fixture(), recoveryNotice: null });

    const { result } = renderHook(() => useTaskBrowser(identity));
    await act(async () => Promise.resolve());
    expect(result.current.recoveryNotice).toBe('browserStateReset');

    await act(async () => new Promise((resolve) => window.setTimeout(resolve, 430)));
    expect(result.current.recoveryNotice).toBe('browserStateReset');
  });

  it('does not fabricate a recovery notice from an ordinary refresh', async () => {
    mocks.load
      .mockResolvedValueOnce({ workspace: null, recoveryNotice: null })
      .mockResolvedValue({ workspace: fixture(), recoveryNotice: 'browserStateReset' });

    const { result } = renderHook(() => useTaskBrowser(identity));
    await act(async () => Promise.resolve());
    expect(result.current.recoveryNotice).toBeNull();

    await act(async () => new Promise((resolve) => window.setTimeout(resolve, 430)));
    expect(result.current.recoveryNotice).toBeNull();
  });

  it('asks for exact loopback approval and never auto-approves casual browsing', async () => {
    mocks.navigate.mockRejectedValueOnce({ kind: 'NeedsApproval' });
    const { result } = renderHook(() => useTaskBrowser(identity));
    await act(async () => Promise.resolve());

    let outcome: Awaited<ReturnType<typeof result.current.navigate>> | undefined;
    await act(async () => {
      outcome = await result.current.navigate('http://localhost:5173/path');
    });
    expect(outcome).toEqual({ kind: 'needsApproval', origin: 'http://localhost:5173' });
  });

  it('restores a loopback page as manual reopen instead of silently loading it', async () => {
    const workspace = fixture();
    workspace.tabs[0].history[0].url = 'http://localhost:5173/';
    mocks.load.mockResolvedValue({ workspace, recoveryNotice: null });
    const { result } = renderHook(() => useTaskBrowser(identity));
    await act(async () => Promise.resolve());

    expect(result.current.activeTab?.manualReopenRequired).toBe(true);
    expect(mocks.activate).toHaveBeenCalledWith(expect.objectContaining({
      tabs: [expect.objectContaining({
        url: 'http://localhost:5173/',
        manualReopenRequired: true,
      })],
    }));
  });

  it('reactivates a persisted loopback page behind a fresh manual-reopen gate', async () => {
    const workspace = fixture();
    workspace.tabs[0].history[0].url = 'http://localhost:5173/';
    workspace.tabs[0].manualReopenRequired = false;
    workspace.tabs[0].restorationStatus = 'restorable';
    mocks.load.mockResolvedValue({ workspace, recoveryNotice: null });
    mocks.activate.mockImplementationOnce(async ({ tabs }) => {
      expect(tabs).toEqual([expect.objectContaining({
        url: 'http://localhost:5173/',
        manualReopenRequired: true,
      })]);
    });

    const { result } = renderHook(() => useTaskBrowser(identity));
    await act(async () => Promise.resolve());

    expect(result.current.errorMessage).toBeNull();
    expect(result.current.activeTab?.manualReopenRequired).toBe(true);
  });

  it('keeps the restored loopback reopen gate across a layout save response', async () => {
    const restored = fixture();
    restored.tabs[0].history[0].url = 'http://localhost:5173/';
    mocks.load.mockResolvedValue({ workspace: restored, recoveryNotice: null });
    mocks.save.mockImplementationOnce(async ({ workspace }) => ({
      workspace: {
        ...workspace,
        tabs: workspace.tabs.map((tab: BrowserWorkspace['tabs'][number]) => ({
          ...tab,
          manualReopenRequired: false,
          restorationStatus: 'restorable' as const,
        })),
      },
    }));
    const { result } = renderHook(() => useTaskBrowser(identity));
    await act(async () => Promise.resolve());
    expect(result.current.activeTab?.manualReopenRequired).toBe(true);

    await act(async () => { await result.current.setLayout('expanded'); });

    expect(result.current.activeTab?.manualReopenRequired).toBe(true);
    expect(result.current.activeTab?.restorationStatus).toBe('manualReopenRequired');
  });

  it('marks an explicit reopen so the persisted privacy gate can be cleared', async () => {
    const { result } = renderHook(() => useTaskBrowser(identity));
    await act(async () => Promise.resolve());

    await act(async () => {
      await result.current.reopen('https://example.com/');
    });
    expect(mocks.navigate).toHaveBeenLastCalledWith({
      identity,
      tabId: fixture().activeTabId,
      url: 'https://example.com/',
      explicitReopen: true,
    });
  });

  it('clears manual reopen after leaving a restored loopback page through history', async () => {
    const restored = fixture();
    restored.tabs[0].history = [
      { position: 0, url: 'https://example.com/', recordedAtMs: 1 },
      { position: 1, url: 'http://localhost:5173/', recordedAtMs: 2 },
    ];
    restored.tabs[0].currentHistoryIndex = 1;
    const afterBack = structuredClone(restored);
    afterBack.tabs[0].currentHistoryIndex = 0;
    mocks.load
      .mockResolvedValueOnce({ workspace: restored, recoveryNotice: null })
      .mockResolvedValue({ workspace: afterBack, recoveryNotice: null });
    const { result } = renderHook(() => useTaskBrowser(identity));
    await act(async () => Promise.resolve());
    expect(result.current.activeTab?.manualReopenRequired).toBe(true);

    await act(async () => { await result.current.back(); });
    await act(async () => new Promise((resolve) => window.setTimeout(resolve, 180)));
    expect(result.current.activeTab?.currentHistoryIndex).toBe(0);
    expect(result.current.activeTab?.manualReopenRequired).toBe(false);
  });

  it('serializes rapid tab creation against the latest persisted workspace', async () => {
    let releaseFirstSave!: () => void;
    mocks.save
      .mockImplementationOnce(({ workspace }) => new Promise((resolve) => {
        releaseFirstSave = () => resolve({ workspace });
      }))
      .mockImplementation(async ({ workspace }) => ({ workspace }));
    const { result } = renderHook(() => useTaskBrowser(identity));
    await act(async () => Promise.resolve());

    let first!: Promise<boolean>;
    let second!: Promise<boolean>;
    act(() => {
      first = result.current.openTab();
      second = result.current.openTab();
    });
    await act(async () => Promise.resolve());
    expect(mocks.save).toHaveBeenCalledTimes(1);

    await act(async () => {
      releaseFirstSave();
      await Promise.all([first, second]);
    });

    expect(mocks.save).toHaveBeenCalledTimes(2);
    const secondWorkspace = mocks.save.mock.calls[1]![0].workspace as BrowserWorkspace;
    expect(secondWorkspace.tabs).toHaveLength(3);
    expect(new Set(secondWorkspace.tabs.map((tab) => tab.id)).size).toBe(3);
    expect(mocks.openTab).toHaveBeenCalledTimes(2);
  });

  it('does not let the Strict Mode replay cleanup deactivate the replacement mount', async () => {
    renderHook(() => useTaskBrowser(identity), {
      wrapper: ({ children }: { children: ReactNode }) => <StrictMode>{children}</StrictMode>,
    });
    await act(async () => Promise.resolve());
    await act(async () => new Promise((resolve) => window.setTimeout(resolve, 60)));
    expect(mocks.activate).toHaveBeenCalled();
    expect(lastCallOrder(mocks.activate)).toBeGreaterThan(lastCallOrder(mocks.deactivate));
  });

  it('does not let a replaced identity cleanup clear the newly activated runtime', async () => {
    const replacement: SessionIdentity = { scope: 'local', sessionId: `s_${'d'.repeat(32)}` };
    const { rerender } = renderHook(
      ({ owner }) => useTaskBrowser(owner),
      { initialProps: { owner: identity as SessionIdentity } },
    );
    await act(async () => Promise.resolve());
    rerender({ owner: replacement });
    await act(async () => Promise.resolve());
    await act(async () => new Promise((resolve) => window.setTimeout(resolve, 60)));
    expect(mocks.activate).toHaveBeenLastCalledWith(expect.objectContaining({ identity: replacement }));
    expect(lastCallOrder(mocks.activate)).toBeGreaterThan(lastCallOrder(mocks.deactivate));
  });

  it('replays geometry only after the native runtime is activated', async () => {
    let release!: (value: { workspace: BrowserWorkspace; recoveryNotice: null }) => void;
    mocks.load.mockReturnValueOnce(new Promise((resolve) => { release = resolve; }));
    const { result } = renderHook(() => useTaskBrowser(identity));
    const host = { x: 10, y: 20, width: 600, height: 400, scaleFactor: 2 };
    await act(async () => { await result.current.setGeometry(host); });
    expect(mocks.geometry).not.toHaveBeenCalled();

    await act(async () => { release({ workspace: fixture(), recoveryNotice: null }); });
    expect(mocks.activate).toHaveBeenCalled();
    expect(mocks.geometry).toHaveBeenCalledWith({ identity, host });
    expect(lastCallOrder(mocks.geometry)).toBeGreaterThan(lastCallOrder(mocks.activate));
  });

  it('polls persisted state so page-authored navigation reaches the address bar', async () => {
    const navigated = fixture();
    navigated.tabs[0].history.push({ position: 1, url: 'https://example.com/next', recordedAtMs: 2 });
    navigated.tabs[0].currentHistoryIndex = 1;
    mocks.load
      .mockResolvedValueOnce({ workspace: fixture(), recoveryNotice: null })
      .mockResolvedValue({ workspace: navigated, recoveryNotice: null });
    const { result } = renderHook(() => useTaskBrowser(identity));
    await act(async () => Promise.resolve());
    expect(result.current.activeTab && result.current.activeTab.history[0].url)
      .toBe('https://example.com/');

    await act(async () => new Promise((resolve) => window.setTimeout(resolve, 430)));
    expect(result.current.activeTab?.currentHistoryIndex).toBe(1);
    expect(result.current.activeTab && result.current.activeTab.history[1].url)
      .toBe('https://example.com/next');
  });
});

function lastCallOrder(mock: ReturnType<typeof vi.fn>): number {
  return mock.mock.invocationCallOrder.at(-1) ?? 0;
}

function fixture(owner: SessionIdentity = identity, tabIdFill = 'b'): BrowserWorkspace {
  const tabId = `bt_${tabIdFill.repeat(32)}`;
  return {
    sessionId: owner.sessionId,
    scope: owner.scope,
    layoutMode: 'split',
    splitWidthPx: 560,
    activeTabId: tabId,
    tabs: [
      {
        id: tabId,
        position: 0,
        currentHistoryIndex: 0,
        manualReopenRequired: false,
        restorationStatus: 'restorable',
        history: [{ position: 0, url: 'https://example.com/', recordedAtMs: 1 }],
      },
    ],
    recovery: null,
  };
}
