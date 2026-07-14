// Typed wrappers for the session-owned Browser restoration descriptor.
// Live WebKit views and cookies never cross this boundary.

import { invokeIpc } from './ipc';
import type { SessionIdentity, SessionScope } from './sessions';

export type BrowserLayoutMode = 'split' | 'expanded';
export type BrowserRestorationStatus = 'blank' | 'restorable' | 'manualReopenRequired';
export type BrowserWorkspaceRecovery = 'browserStateReset';

export type BrowserHistoryRecord = {
  position: number;
  url: string;
  recordedAtMs: number;
};

export type BrowserTab = {
  id: string;
  position: number;
  currentHistoryIndex: number | null;
  manualReopenRequired: boolean;
  restorationStatus: BrowserRestorationStatus;
  history: BrowserHistoryRecord[];
};

export type BrowserWorkspace = {
  sessionId: string;
  scope: SessionScope;
  layoutMode: BrowserLayoutMode;
  splitWidthPx: number;
  activeTabId: string | null;
  tabs: BrowserTab[];
  recovery: BrowserWorkspaceRecovery | null;
};

export type BrowserWorkspaceLoadPayload = { identity: SessionIdentity };
export type BrowserWorkspaceSavePayload = {
  identity: SessionIdentity;
  workspace: BrowserWorkspace;
};
export type BrowserWorkspaceResetPayload = { identity: SessionIdentity };

export type BrowserWorkspaceLoadResponse = {
  workspace: BrowserWorkspace | null;
  recoveryNotice: BrowserWorkspaceRecovery | null;
};

export type BrowserWorkspaceResponse = { workspace: BrowserWorkspace };

export function loadBrowserWorkspace(
  payload: BrowserWorkspaceLoadPayload,
): Promise<BrowserWorkspaceLoadResponse> {
  return invokeIpc('browser_workspace_load', payload);
}

export function saveBrowserWorkspace(
  payload: BrowserWorkspaceSavePayload,
): Promise<BrowserWorkspaceResponse> {
  return invokeIpc('browser_workspace_save', payload);
}

export function resetBrowserWorkspace(
  payload: BrowserWorkspaceResetPayload,
): Promise<BrowserWorkspaceResponse> {
  return invokeIpc('browser_workspace_reset', payload);
}
