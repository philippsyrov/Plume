import { beforeEach, describe, expect, it, vi } from 'vitest';

const mocks = vi.hoisted(() => ({ invokeIpc: vi.fn() }));
vi.mock('./ipc', () => ({ invokeIpc: mocks.invokeIpc }));

import {
  backBrowserSandbox,
  captureBrowserText,
  captureBrowserScreenshot,
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

  it('captures only a fixed text kind through the dedicated command', async () => {
    await captureBrowserText('selection');
    expect(mocks.invokeIpc).toHaveBeenCalledWith('browser_sandbox_capture_text', {
      captureKind: 'selection',
    });
  });

  it('captures a native screenshot through an empty-payload command', async () => {
    await captureBrowserScreenshot();
    expect(mocks.invokeIpc).toHaveBeenCalledWith('browser_sandbox_capture_screenshot', {});
  });
});
