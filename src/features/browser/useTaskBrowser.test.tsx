import { act, renderHook } from '@testing-library/react';
import { StrictMode, type ReactNode } from 'react';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import type { BrowserWorkspace } from '../../lib/api/browserWorkspace';
import type { SessionIdentity } from '../../lib/api/sessions';
import { useTaskBrowser } from './useTaskBrowser';

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
  geometry: vi.fn(),
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
  setTaskBrowserGeometry: mocks.geometry,
  captureTaskBrowserText: mocks.captureText,
  captureTaskBrowserScreenshot: mocks.captureScreenshot,
}));

const identity = { scope: 'project' as const, sessionId: `s_${'a'.repeat(32)}` };

describe('useTaskBrowser', () => {
  beforeEach(() => {
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
      mocks.geometry,
    ]) {
      mock.mockResolvedValue(undefined);
    }
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
});

function lastCallOrder(mock: ReturnType<typeof vi.fn>): number {
  return mock.mock.invocationCallOrder.at(-1) ?? 0;
}

function fixture(): BrowserWorkspace {
  return {
    sessionId: identity.sessionId,
    scope: identity.scope,
    layoutMode: 'split',
    splitWidthPx: 560,
    activeTabId: `bt_${'b'.repeat(32)}`,
    tabs: [
      {
        id: `bt_${'b'.repeat(32)}`,
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
