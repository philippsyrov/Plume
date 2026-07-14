import { describe, expect, it, vi } from 'vitest';

const mocks = vi.hoisted(() => ({ invokeIpc: vi.fn() }));
vi.mock('./ipc', () => ({ invokeIpc: mocks.invokeIpc }));

import {
  activateTaskBrowser,
  captureTaskBrowserText,
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
  it('activates the native runtime from the persisted descriptor only', async () => {
    mocks.invokeIpc.mockResolvedValue(undefined);
    await activateTaskBrowser({
      identity,
      tabs: [
        {
          tabId: workspace.tabs[0]!.id,
          url: null,
          manualReopenRequired: false,
        },
      ],
      activeTabId: workspace.tabs[0]!.id,
    });
    expect(mocks.invokeIpc).toHaveBeenCalledWith('task_browser_activate', {
      identity,
      tabs: [
        {
          tabId: workspace.tabs[0]!.id,
          url: null,
          manualReopenRequired: false,
        },
      ],
      activeTabId: workspace.tabs[0]!.id,
    });
  });

  it('captures text against an exact session and tab', async () => {
    mocks.invokeIpc.mockResolvedValue({ evidence: {}, source: {} });
    await captureTaskBrowserText({
      identity,
      tabId: workspace.tabs[0]!.id,
      captureKind: 'selection',
    });
    expect(mocks.invokeIpc).toHaveBeenCalledWith('task_browser_capture_text', {
      identity,
      tabId: workspace.tabs[0]!.id,
      captureKind: 'selection',
    });
  });

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
