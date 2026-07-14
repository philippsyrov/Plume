import { act, renderHook } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import type { BrowserSandboxState } from '../../lib/api/browser';
import { useBrowserWorkspace } from './useBrowserWorkspace';

const mocks = vi.hoisted(() => ({
  getState: vi.fn(),
  open: vi.fn(),
  focus: vi.fn(),
  back: vi.fn(),
  forward: vi.fn(),
  reload: vi.fn(),
  close: vi.fn(),
}));

vi.mock('../../lib/api/browser', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../../lib/api/browser')>();
  return {
    ...actual,
    getBrowserSandboxState: mocks.getState,
    openBrowserSandbox: mocks.open,
    focusBrowserSandbox: mocks.focus,
    backBrowserSandbox: mocks.back,
    forwardBrowserSandbox: mocks.forward,
    reloadBrowserSandbox: mocks.reload,
    closeBrowserSandbox: mocks.close,
  };
});

describe('useBrowserWorkspace', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.clearAllMocks();
    mocks.getState.mockResolvedValue(stateFixture());
    for (const action of [mocks.open, mocks.focus, mocks.back, mocks.forward, mocks.reload, mocks.close]) {
      action.mockResolvedValue(stateFixture());
    }
  });

  afterEach(() => vi.useRealTimers());

  it('loads current state and uses a faster poll while navigation is loading', async () => {
    mocks.getState.mockResolvedValueOnce(stateFixture({ open: true, loading: true }));
    const { result } = renderHook(() => useBrowserWorkspace());

    await act(async () => Promise.resolve());
    expect(result.current.state?.loading).toBe(true);
    expect(mocks.getState).toHaveBeenCalledTimes(1);

    await act(async () => vi.advanceTimersByTimeAsync(500));
    expect(mocks.getState).toHaveBeenCalledTimes(2);
  });

  it('ignores an older state response after a newer refresh resolves', async () => {
    const old = deferred<BrowserSandboxState>();
    const fresh = deferred<BrowserSandboxState>();
    mocks.getState.mockReturnValueOnce(old.promise).mockReturnValueOnce(fresh.promise);
    const { result } = renderHook(() => useBrowserWorkspace());

    act(() => result.current.refresh());
    fresh.resolve(stateFixture({ currentUrl: 'https://fresh.example/' }));
    await act(async () => Promise.resolve());
    expect(result.current.state?.currentUrl).toBe('https://fresh.example/');

    old.resolve(stateFixture({ currentUrl: 'https://old.example/' }));
    await act(async () => Promise.resolve());
    expect(result.current.state?.currentUrl).toBe('https://fresh.example/');
  });

  it('returns a localhost approval request without replacing it with a generic error', async () => {
    mocks.open.mockRejectedValueOnce({ kind: 'NeedsApproval' });
    const { result } = renderHook(() => useBrowserWorkspace());
    await act(async () => Promise.resolve());

    let outcome: Awaited<ReturnType<typeof result.current.open>> | undefined;
    await act(async () => {
      outcome = await result.current.open('http://localhost:5173/path');
    });
    expect(outcome).toEqual({ kind: 'needsApproval', origin: 'http://localhost:5173' });
    expect(result.current.errorMessage).toBeNull();
  });

  it('maps backend details to short product copy and refreshes after fixed actions', async () => {
    mocks.reload.mockRejectedValueOnce({ kind: 'Internal', details: 'private native detail' });
    const { result } = renderHook(() => useBrowserWorkspace());
    await act(async () => Promise.resolve());

    await act(async () => {
      await result.current.reload();
    });
    expect(result.current.errorMessage).toBe('Browser unavailable. Try again.');
    expect(result.current.errorMessage).not.toContain('private native detail');

    await act(async () => vi.advanceTimersByTimeAsync(2_000));
    expect(mocks.getState).toHaveBeenCalledTimes(2);

    mocks.focus.mockResolvedValueOnce(stateFixture({ open: true }));
    await act(async () => {
      await result.current.focus();
    });
    expect(result.current.state?.open).toBe(true);
    expect(result.current.errorMessage).toBeNull();
  });

  it('does not update after unmount', async () => {
    const pending = deferred<BrowserSandboxState>();
    mocks.getState.mockReturnValueOnce(pending.promise);
    const hook = renderHook(() => useBrowserWorkspace());
    hook.unmount();

    pending.resolve(stateFixture({ open: true }));
    await act(async () => Promise.resolve());
    expect(mocks.getState).toHaveBeenCalledTimes(1);
  });
});

function stateFixture(overrides: Partial<BrowserSandboxState> = {}): BrowserSandboxState {
  return {
    open: false,
    windowLabel: null,
    requestedUrl: null,
    currentUrl: null,
    title: null,
    loading: false,
    failure: null,
    ...overrides,
  };
}

function deferred<T>(): { promise: Promise<T>; resolve: (value: T) => void } {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}
