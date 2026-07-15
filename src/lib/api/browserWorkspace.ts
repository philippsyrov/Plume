// Typed wrappers for the session-owned Browser restoration descriptor.
// Live WebKit views and cookies never cross this boundary.

import { invokeIpc } from './ipc';
import type {
  BrowserCaptureKind,
  BrowserEvidenceSummary,
  BrowserScreenshotSummary,
} from './browser';
import type { ContextSourceRef } from './chat';
import { isIpcError } from './errors';
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

export type TaskBrowserTabPayload = {
  tabId: string;
  url: string | null;
  manualReopenRequired: boolean;
};

export type TaskBrowserActivatePayload = {
  identity: SessionIdentity;
  tabs: TaskBrowserTabPayload[];
  activeTabId: string;
};

export type TaskBrowserTabPayloadWithIdentity = {
  identity: SessionIdentity;
  tabId: string;
};

export function activateTaskBrowser(payload: TaskBrowserActivatePayload): Promise<void> {
  return invokeIpc('task_browser_activate', payload);
}

export function deactivateTaskBrowser(payload: { identity: SessionIdentity }): Promise<void> {
  return invokeIpc('task_browser_deactivate', payload);
}

export function setTaskBrowserSuspended(payload: {
  identity: SessionIdentity;
  suspended: boolean;
}): Promise<void> {
  return invokeIpc('task_browser_set_suspended', payload);
}

export function openTaskBrowserTab(payload: {
  identity: SessionIdentity;
  tab: TaskBrowserTabPayload;
}): Promise<void> {
  return invokeIpc('task_browser_open_tab', payload);
}

export function closeTaskBrowserTab(
  payload: TaskBrowserTabPayloadWithIdentity,
): Promise<string | null> {
  return invokeIpc('task_browser_close_tab', payload);
}

export function selectTaskBrowserTab(payload: TaskBrowserTabPayloadWithIdentity): Promise<void> {
  return invokeIpc('task_browser_select_tab', payload);
}

export function navigateTaskBrowser(payload: TaskBrowserTabPayloadWithIdentity & {
  url: string;
  approvedLoopbackOrigin?: string;
  explicitReopen?: boolean;
}): Promise<void> {
  return invokeIpc('task_browser_navigate', payload);
}

export function backTaskBrowser(payload: TaskBrowserTabPayloadWithIdentity & {
  approvedLoopbackOrigin?: string;
}): Promise<void> {
  return invokeIpc('task_browser_back', payload);
}

export function forwardTaskBrowser(payload: TaskBrowserTabPayloadWithIdentity & {
  approvedLoopbackOrigin?: string;
}): Promise<void> {
  return invokeIpc('task_browser_forward', payload);
}

export function reloadTaskBrowser(payload: TaskBrowserTabPayloadWithIdentity): Promise<void> {
  return invokeIpc('task_browser_reload', payload);
}

export type TaskBrowserHostRect = {
  x: number;
  y: number;
  width: number;
  height: number;
  scaleFactor: number;
};

export function setTaskBrowserGeometry(payload: {
  identity: SessionIdentity;
  host: TaskBrowserHostRect;
}): Promise<void> {
  return invokeIpc('task_browser_set_geometry', payload);
}

export function captureTaskBrowserText(payload: TaskBrowserTabPayloadWithIdentity & {
  captureKind: BrowserCaptureKind;
}): Promise<{ evidence: BrowserEvidenceSummary; source: ContextSourceRef }> {
  return invokeIpc('task_browser_capture_text', payload);
}

export function captureTaskBrowserScreenshot(
  payload: TaskBrowserTabPayloadWithIdentity,
): Promise<{ evidence: BrowserScreenshotSummary; source: ContextSourceRef }> {
  return invokeIpc('task_browser_capture_screenshot', payload);
}

export function isTaskBrowserCapturePageChanged(error: unknown): boolean {
  return isIpcError(error)
    && error.kind === 'Blocked'
    && error.details === 'browser.capturePageChanged';
}
