import { beforeEach, describe, expect, it, vi } from 'vitest';

const mocks = vi.hoisted(() => ({ invokeIpc: vi.fn() }));
vi.mock('./ipc', () => ({ invokeIpc: mocks.invokeIpc }));

import {
  backBrowserSandbox,
  closeBrowserSandbox,
  focusBrowserSandbox,
  forwardBrowserSandbox,
  getBrowserSandboxState,
  openBrowserSandbox,
  reloadBrowserSandbox,
} from './browser';

describe('browser IPC wrappers', () => {
  beforeEach(() => mocks.invokeIpc.mockReset().mockResolvedValue({ open: false }));

  it('sends the optional exact localhost approval only on open', async () => {
    await openBrowserSandbox('http://localhost:5173/path', 'http://localhost:5173');
    expect(mocks.invokeIpc).toHaveBeenCalledWith('browser_sandbox_open', {
      url: 'http://localhost:5173/path',
      approvedLoopbackOrigin: 'http://localhost:5173',
    });
  });

  it('maps every fixed control to an empty-payload command', async () => {
    await getBrowserSandboxState();
    await focusBrowserSandbox();
    await backBrowserSandbox();
    await forwardBrowserSandbox();
    await reloadBrowserSandbox();
    await closeBrowserSandbox();

    expect(mocks.invokeIpc.mock.calls).toEqual([
      ['browser_sandbox_state', {}],
      ['browser_sandbox_focus', {}],
      ['browser_sandbox_back', {}],
      ['browser_sandbox_forward', {}],
      ['browser_sandbox_reload', {}],
      ['browser_sandbox_close', {}],
    ]);
  });
});
