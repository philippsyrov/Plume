import { describe, expect, it, vi } from 'vitest';

const mocks = vi.hoisted(() => ({ invokeIpc: vi.fn() }));
vi.mock('./ipc', () => ({ invokeIpc: mocks.invokeIpc }));

import {
  loadBrowserWorkspace,
  resetBrowserWorkspace,
  saveBrowserWorkspace,
  type BrowserWorkspace,
} from './browserWorkspace';

const identity = { scope: 'project' as const, sessionId: 's123' };
const workspace: BrowserWorkspace = {
  sessionId: 's123',
  scope: 'project',
  layoutMode: 'split',
  splitWidthPx: 560,
  activeTabId: 'bt_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
  tabs: [
    {
      id: 'bt_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
      position: 0,
      currentHistoryIndex: null,
      manualReopenRequired: false,
      restorationStatus: 'blank',
      history: [],
    },
  ],
  recovery: null,
};

describe('browser workspace API', () => {
  it('loads using only the nested session identity', async () => {
    mocks.invokeIpc.mockResolvedValue({ workspace: null, recoveryNotice: null });
    await loadBrowserWorkspace({ identity });
    expect(mocks.invokeIpc).toHaveBeenCalledWith('browser_workspace_load', { identity });
  });

  it('saves the exact identity and workspace payload', async () => {
    mocks.invokeIpc.mockResolvedValue({ workspace });
    await saveBrowserWorkspace({ identity, workspace });
    expect(mocks.invokeIpc).toHaveBeenCalledWith('browser_workspace_save', {
      identity,
      workspace,
    });
  });

  it('resets without accepting a filesystem path', async () => {
    mocks.invokeIpc.mockResolvedValue({ workspace });
    await resetBrowserWorkspace({ identity });
    expect(mocks.invokeIpc).toHaveBeenCalledWith('browser_workspace_reset', { identity });
  });
});
