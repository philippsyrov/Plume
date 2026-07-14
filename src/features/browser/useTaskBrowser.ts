import { useCallback, useEffect, useMemo, useRef, useState } from 'react';

import type { BrowserCaptureKind, BrowserEvidenceSummary, BrowserScreenshotSummary } from '../../lib/api/browser';
import {
  activateTaskBrowser,
  backTaskBrowser,
  captureTaskBrowserScreenshot,
  captureTaskBrowserText,
  closeTaskBrowserTab,
  deactivateTaskBrowser,
  forwardTaskBrowser,
  loadBrowserWorkspace,
  navigateTaskBrowser,
  openTaskBrowserTab,
  reloadTaskBrowser,
  resetBrowserWorkspace,
  saveBrowserWorkspace,
  selectTaskBrowserTab,
  setTaskBrowserGeometry,
  type BrowserLayoutMode,
  type BrowserWorkspace,
  type TaskBrowserHostRect,
} from '../../lib/api/browserWorkspace';
import type { ContextSourceRef } from '../../lib/api/chat';
import { isIpcError } from '../../lib/api/errors';
import type { SessionIdentity } from '../../lib/api/sessions';

export type TaskBrowserNavigateOutcome =
  | { kind: 'opened' }
  | { kind: 'needsApproval'; origin: string }
  | { kind: 'failed' };

export type TaskBrowserCaptureOutcome<T> =
  | { kind: 'captured'; evidence: T; source: ContextSourceRef }
  | { kind: 'failed' };

export type TaskBrowserApi = {
  workspace: BrowserWorkspace | null;
  activeTab: BrowserWorkspace['tabs'][number] | null;
  busy: boolean;
  errorMessage: string | null;
  navigate: (url: string, approvedLoopbackOrigin?: string) => Promise<TaskBrowserNavigateOutcome>;
  back: () => Promise<boolean>;
  forward: () => Promise<boolean>;
  reload: () => Promise<boolean>;
  setGeometry: (host: TaskBrowserHostRect) => Promise<void>;
  setLayout: (mode: BrowserLayoutMode) => Promise<boolean>;
  openTab: () => Promise<boolean>;
  closeTab: (tabId: string) => Promise<boolean>;
  selectTab: (tabId: string) => Promise<boolean>;
  captureText: (kind: BrowserCaptureKind) => Promise<TaskBrowserCaptureOutcome<BrowserEvidenceSummary>>;
  captureScreenshot: () => Promise<TaskBrowserCaptureOutcome<BrowserScreenshotSummary>>;
};

let browserLeaseGeneration = 0;

export function useTaskBrowser(identity: SessionIdentity): TaskBrowserApi {
  const [workspace, setWorkspace] = useState<BrowserWorkspace | null>(null);
  const [busy, setBusy] = useState(true);
  const [errorMessage, setErrorMessage] = useState<string | null>(null);
  const generationRef = useRef(0);
  const runtimeReadyRef = useRef(false);
  const geometryRef = useRef<TaskBrowserHostRect | null>(null);
  const workspaceRef = useRef(workspace);
  workspaceRef.current = workspace;

  const refresh = useCallback(async (generation: number) => {
    const response = await loadBrowserWorkspace({ identity });
    if (generation !== generationRef.current) return;
    if (response.workspace) setWorkspace(response.workspace);
  }, [identity.scope, identity.sessionId]);

  useEffect(() => {
    const lease = ++browserLeaseGeneration;
    const generation = ++generationRef.current;
    runtimeReadyRef.current = false;
    setBusy(true);
    setErrorMessage(null);
    void (async () => {
      try {
        const loaded = await loadBrowserWorkspace({ identity });
        const next = loaded.workspace ?? (await resetBrowserWorkspace({ identity })).workspace;
        if (generation !== generationRef.current) return;
        await activateTaskBrowser(activationPayload(identity, next));
        if (generation !== generationRef.current) return;
        runtimeReadyRef.current = true;
        setWorkspace(next);
        if (geometryRef.current) {
          await setTaskBrowserGeometry({ identity, host: geometryRef.current });
        }
      } catch (error) {
        if (generation === generationRef.current) setErrorMessage(productError(error));
      } finally {
        if (generation === generationRef.current) setBusy(false);
      }
    })();
    return () => {
      generationRef.current += 1;
      runtimeReadyRef.current = false;
      window.setTimeout(() => {
        if (browserLeaseGeneration !== lease) return;
        void deactivateTaskBrowser({ identity }).catch(() => undefined);
      }, 50);
    };
  }, [identity.scope, identity.sessionId]);

  const activeTab = useMemo(() => {
    if (!workspace?.activeTabId) return null;
    return workspace.tabs.find((tab) => tab.id === workspace.activeTabId) ?? null;
  }, [workspace]);

  const runTabAction = useCallback(async (
    action: (payload: { identity: SessionIdentity; tabId: string }) => Promise<void>,
  ) => {
    const tabId = workspaceRef.current?.activeTabId;
    if (!tabId) return false;
    setBusy(true);
    try {
      await action({ identity, tabId });
      setErrorMessage(null);
      window.setTimeout(() => void refresh(generationRef.current), 160);
      return true;
    } catch (error) {
      setErrorMessage(productError(error));
      return false;
    } finally {
      setBusy(false);
    }
  }, [identity.scope, identity.sessionId, refresh]);

  const navigate = useCallback(async (
    url: string,
    approvedLoopbackOrigin?: string,
  ): Promise<TaskBrowserNavigateOutcome> => {
    const tabId = workspaceRef.current?.activeTabId;
    if (!tabId) return { kind: 'failed' };
    setBusy(true);
    try {
      await navigateTaskBrowser({
        identity,
        tabId,
        url,
        ...(approvedLoopbackOrigin ? { approvedLoopbackOrigin } : {}),
      });
      setErrorMessage(null);
      window.setTimeout(() => void refresh(generationRef.current), 160);
      return { kind: 'opened' };
    } catch (error) {
      if (isIpcError(error) && error.kind === 'NeedsApproval') {
        try {
          return { kind: 'needsApproval', origin: new URL(url).origin };
        } catch {
          return { kind: 'failed' };
        }
      }
      setErrorMessage(productError(error));
      return { kind: 'failed' };
    } finally {
      setBusy(false);
    }
  }, [identity.scope, identity.sessionId, refresh]);

  const saveLocalWorkspace = useCallback(async (next: BrowserWorkspace) => {
    try {
      const saved = await saveBrowserWorkspace({ identity, workspace: next });
      setWorkspace(saved.workspace);
      setErrorMessage(null);
      return saved.workspace;
    } catch (error) {
      setErrorMessage(productError(error));
      return null;
    }
  }, [identity.scope, identity.sessionId]);

  const setLayout = useCallback(async (layoutMode: BrowserLayoutMode) => {
    const current = workspaceRef.current;
    if (!current) return false;
    return (await saveLocalWorkspace({ ...current, layoutMode })) !== null;
  }, [saveLocalWorkspace]);

  const openTab = useCallback(async () => {
    const current = workspaceRef.current;
    if (!current || current.tabs.length >= 5) return false;
    const tab = {
      id: mintTabId(),
      position: current.tabs.length,
      currentHistoryIndex: null,
      manualReopenRequired: false,
      restorationStatus: 'blank' as const,
      history: [],
    };
    const saved = await saveLocalWorkspace({
      ...current,
      activeTabId: tab.id,
      tabs: [...current.tabs, tab],
    });
    if (!saved) return false;
    try {
      await openTaskBrowserTab({
        identity,
        tab: { tabId: tab.id, url: null, manualReopenRequired: false },
      });
      return true;
    } catch (error) {
      setErrorMessage(productError(error));
      await saveLocalWorkspace(current);
      return false;
    }
  }, [identity.scope, identity.sessionId, saveLocalWorkspace]);

  const closeTab = useCallback(async (tabId: string) => {
    const current = workspaceRef.current;
    if (!current || current.tabs.length <= 1) return false;
    try {
      const nextActive = await closeTaskBrowserTab({ identity, tabId });
      const tabs = current.tabs
        .filter((tab) => tab.id !== tabId)
        .map((tab, position) => ({ ...tab, position }));
      return (await saveLocalWorkspace({ ...current, tabs, activeTabId: nextActive })) !== null;
    } catch (error) {
      setErrorMessage(productError(error));
      return false;
    }
  }, [identity.scope, identity.sessionId, saveLocalWorkspace]);

  const selectTab = useCallback(async (tabId: string) => {
    const current = workspaceRef.current;
    if (!current || !current.tabs.some((tab) => tab.id === tabId)) return false;
    try {
      await selectTaskBrowserTab({ identity, tabId });
      return (await saveLocalWorkspace({ ...current, activeTabId: tabId })) !== null;
    } catch (error) {
      setErrorMessage(productError(error));
      return false;
    }
  }, [identity.scope, identity.sessionId, saveLocalWorkspace]);

  const captureText = useCallback(async (captureKind: BrowserCaptureKind) => {
    const tabId = workspaceRef.current?.activeTabId;
    if (!tabId) return { kind: 'failed' } as const;
    try {
      const captured = await captureTaskBrowserText({ identity, tabId, captureKind });
      return { kind: 'captured', ...captured } as const;
    } catch (error) {
      setErrorMessage(productError(error));
      return { kind: 'failed' } as const;
    }
  }, [identity.scope, identity.sessionId]);

  const captureScreenshot = useCallback(async () => {
    const tabId = workspaceRef.current?.activeTabId;
    if (!tabId) return { kind: 'failed' } as const;
    try {
      const captured = await captureTaskBrowserScreenshot({ identity, tabId });
      return { kind: 'captured', ...captured } as const;
    } catch (error) {
      setErrorMessage(productError(error));
      return { kind: 'failed' } as const;
    }
  }, [identity.scope, identity.sessionId]);

  return {
    workspace,
    activeTab,
    busy,
    errorMessage,
    navigate,
    back: () => runTabAction(backTaskBrowser),
    forward: () => runTabAction(forwardTaskBrowser),
    reload: () => runTabAction(reloadTaskBrowser),
    setGeometry: async (host) => {
      geometryRef.current = host;
      if (!runtimeReadyRef.current) return;
      await setTaskBrowserGeometry({ identity, host });
    },
    setLayout,
    openTab,
    closeTab,
    selectTab,
    captureText,
    captureScreenshot,
  };
}

function activationPayload(identity: SessionIdentity, workspace: BrowserWorkspace) {
  return {
    identity,
    tabs: workspace.tabs.map((tab) => ({
      tabId: tab.id,
      url: currentUrl(tab),
      manualReopenRequired: tab.manualReopenRequired,
    })),
    activeTabId: workspace.activeTabId ?? workspace.tabs[0]!.id,
  };
}

export function currentUrl(tab: BrowserWorkspace['tabs'][number]): string | null {
  return tab.currentHistoryIndex === null ? null : tab.history[tab.currentHistoryIndex]?.url ?? null;
}

function mintTabId(): string {
  const bytes = new Uint8Array(16);
  crypto.getRandomValues(bytes);
  return `bt_${Array.from(bytes, (byte) => byte.toString(16).padStart(2, '0')).join('')}`;
}

function productError(error: unknown): string {
  if (isIpcError(error)) {
    if (error.kind === 'NotFound') return 'This Browser belongs to another chat.';
    if (error.kind === 'Blocked') return 'That Browser action is unavailable right now.';
    if (error.kind === 'BadArgument') return 'That page could not be opened.';
  }
  return 'Browser unavailable. Try again.';
}
