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
  isTaskBrowserCapturePageChanged,
  loadBrowserWorkspace,
  navigateTaskBrowser,
  openTaskBrowserTab,
  reloadTaskBrowser,
  resetBrowserWorkspace,
  saveBrowserWorkspace,
  selectTaskBrowserTab,
  setTaskBrowserGeometry,
  setTaskBrowserSuspended,
  type BrowserLayoutMode,
  type BrowserWorkspace,
  type BrowserWorkspaceRecovery,
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
  recoveryNotice: BrowserWorkspaceRecovery | null;
  activeTab: BrowserWorkspace['tabs'][number] | null;
  busy: boolean;
  errorMessage: string | null;
  suspended: boolean;
  runtimeReady: boolean;
  overlaySafe: boolean;
  retryRuntime: () => void;
  navigate: (url: string, approvedLoopbackOrigin?: string) => Promise<TaskBrowserNavigateOutcome>;
  reopen: (url: string, approvedLoopbackOrigin?: string) => Promise<TaskBrowserNavigateOutcome>;
  back: (approvedLoopbackOrigin?: string) => Promise<TaskBrowserNavigateOutcome>;
  forward: (approvedLoopbackOrigin?: string) => Promise<TaskBrowserNavigateOutcome>;
  reload: () => Promise<boolean>;
  setGeometry: (host: TaskBrowserHostRect) => Promise<void>;
  setLayout: (mode: BrowserLayoutMode) => Promise<boolean>;
  setSplitWidth: (widthPx: number) => Promise<boolean>;
  openTab: () => Promise<boolean>;
  closeTab: (tabId: string) => Promise<boolean>;
  selectTab: (tabId: string) => Promise<boolean>;
  captureText: (kind: BrowserCaptureKind) => Promise<TaskBrowserCaptureOutcome<BrowserEvidenceSummary>>;
  captureScreenshot: () => Promise<TaskBrowserCaptureOutcome<BrowserScreenshotSummary>>;
};

export const SUSPENSION_ACK_TIMEOUT_MS = 1_500;
export const BROWSER_ACTIVATION_ACK_TIMEOUT_MS = 1_500;

type RuntimeState = 'starting' | 'ready' | 'inactive' | 'unknown';

let browserLeaseGeneration = 0;
type BrowserLease = { generation: number; identityKey: string };
let currentBrowserLease: BrowserLease | null = null;
let activeBrowserLease: BrowserLease | null = null;
let browserActivationQueue: Promise<void> = Promise.resolve();
const MAX_RECOVERY_NOTICE_HANDOFFS = 32;
type RecoveryNoticeHandoff = { notice: BrowserWorkspaceRecovery };
const recoveryNoticeHandoffs = new Map<string, RecoveryNoticeHandoff>();

export function useTaskBrowser(identity: SessionIdentity, shouldSuspend = false): TaskBrowserApi {
  const [workspace, setWorkspace] = useState<BrowserWorkspace | null>(null);
  const [recoveryNotice, setRecoveryNotice] = useState<BrowserWorkspaceRecovery | null>(null);
  const [busy, setBusy] = useState(true);
  const [errorMessage, setErrorMessage] = useState<string | null>(null);
  const [suspended, setSuspended] = useState(false);
  const [suspensionProofValid, setSuspensionProofValid] = useState(false);
  const [runtimeState, setRuntimeState] = useState<RuntimeState>('starting');
  const [runtimeRetryRevision, setRuntimeRetryRevision] = useState(0);
  const generationRef = useRef(0);
  const pageRequestRevisionRef = useRef(0);
  const activeIdentityKeyRef = useRef<string | null>(null);
  const runtimeReadyRef = useRef(false);
  const suspensionRequestedRef = useRef(shouldSuspend);
  const manualLoopbackTabsRef = useRef(new Set<string>());
  const geometryRef = useRef<TaskBrowserHostRect | null>(null);
  const workspaceRef = useRef(workspace);
  const workspaceWriteRevisionRef = useRef(0);
  const workspaceMutationQueueRef = useRef<Promise<void>>(Promise.resolve());
  const suspensionMutationQueueRef = useRef<Promise<void>>(Promise.resolve());
  workspaceRef.current = workspace;
  suspensionRequestedRef.current = shouldSuspend;

  const commitWorkspace = useCallback((next: BrowserWorkspace) => {
    if (currentPageKey(workspaceRef.current) !== currentPageKey(next)) {
      pageRequestRevisionRef.current += 1;
    }
    workspaceRef.current = next;
    setWorkspace(next);
  }, []);

  const enqueueWorkspaceMutation = useCallback(<T,>(operation: () => Promise<T>): Promise<T> => {
    const run = workspaceMutationQueueRef.current.then(operation, operation);
    workspaceMutationQueueRef.current = run.then(() => undefined, () => undefined);
    return run;
  }, []);

  const enqueueSuspensionSync = useCallback((generation: number): Promise<void> => {
    const operation = async () => {
      while (generation === generationRef.current) {
        const requested = suspensionRequestedRef.current;
        setSuspensionProofValid(false);
        await withDeadline(
          setTaskBrowserSuspended({ identity, suspended: requested }),
          SUSPENSION_ACK_TIMEOUT_MS,
        );
        if (generation !== generationRef.current) return;
        if (suspensionRequestedRef.current === requested) {
          setSuspended(requested);
          setSuspensionProofValid(requested);
          setErrorMessage(null);
          return;
        }
      }
    };
    const run = suspensionMutationQueueRef.current.then(operation, operation);
    suspensionMutationQueueRef.current = run.then(() => undefined, () => undefined);
    return run;
  }, [identity.scope, identity.sessionId]);

  const recoverFromSuspensionFailure = useCallback(async (generation: number, error: unknown) => {
    if (generation !== generationRef.current) return;
    runtimeReadyRef.current = false;
    setSuspensionProofValid(false);
    setRuntimeState('unknown');
    setErrorMessage(productError(error));
    try {
      await withDeadline(
        deactivateTaskBrowser({ identity }),
        SUSPENSION_ACK_TIMEOUT_MS,
      );
      if (generation === generationRef.current) setRuntimeState('inactive');
    } catch {
      if (generation === generationRef.current) setRuntimeState('unknown');
    }
  }, [identity.scope, identity.sessionId]);

  const refresh = useCallback(async (generation: number) => {
    const writeRevision = workspaceWriteRevisionRef.current;
    try {
      const response = await loadBrowserWorkspace({ identity });
      if (generation !== generationRef.current
        || writeRevision !== workspaceWriteRevisionRef.current) return;
      if (response.workspace) {
        commitWorkspace(markManualLoopbackTabs(response.workspace, manualLoopbackTabsRef.current));
      }
    } catch (error) {
      if (generation === generationRef.current) setErrorMessage(productError(error));
    }
  }, [commitWorkspace, identity.scope, identity.sessionId]);

  useEffect(() => {
    const identityKey = taskBrowserIdentityKey(identity);
    const lease = ++browserLeaseGeneration;
    currentBrowserLease = { generation: lease, identityKey };
    const generation = ++generationRef.current;
    pageRequestRevisionRef.current += 1;
    activeIdentityKeyRef.current = identityKey;
    runtimeReadyRef.current = false;
    setSuspensionProofValid(false);
    setRuntimeState('starting');
    setBusy(true);
    setErrorMessage(null);
    setRecoveryNotice(recoveryNoticeHandoffs.get(identityKey)?.notice ?? null);
    void (async () => {
      try {
        const loaded = await loadBrowserWorkspace({ identity });
        if (loaded.recoveryNotice) {
          const handoff = rememberRecoveryNotice(identityKey, loaded.recoveryNotice);
          if (activeIdentityKeyRef.current === identityKey) {
            setRecoveryNotice(loaded.recoveryNotice);
            if (runtimeReadyRef.current) clearRecoveryNotice(identityKey, handoff);
          }
        }
        if (generation !== generationRef.current) return;
        const restored = loaded.workspace ?? (await resetBrowserWorkspace({ identity })).workspace;
        manualLoopbackTabsRef.current = restoredLoopbackTabIds(restored);
        const next = markManualLoopbackTabs(restored, manualLoopbackTabsRef.current);
        if (generation !== generationRef.current) return;
        await enqueueBrowserActivation(async () => {
          try {
            await withDeadline(
              activateTaskBrowser(activationPayload(identity, next)),
              BROWSER_ACTIVATION_ACK_TIMEOUT_MS,
            );
          } catch (error) {
            // The Rust activation command is synchronous: a missing acknowledgement can
            // only leave the requested identity selected before the Promise stalls.
            // Fence that state before releasing the process-wide activation queue.
            await deactivateTaskBrowser({ identity }).catch(() => undefined);
            throw error;
          }
          if (generation !== generationRef.current) {
            await deactivateTaskBrowser({ identity }).catch(() => undefined);
            return;
          }
          activeBrowserLease = { generation: lease, identityKey };
        });
        if (generation !== generationRef.current) return;
        try {
          await enqueueSuspensionSync(generation);
        } catch (error) {
          await recoverFromSuspensionFailure(generation, error);
          return;
        }
        if (generation !== generationRef.current) return;
        runtimeReadyRef.current = true;
        setRuntimeState('ready');
        const recoveryHandoff = recoveryNoticeHandoffs.get(identityKey) ?? null;
        if (recoveryHandoff) clearRecoveryNotice(identityKey, recoveryHandoff);
        commitWorkspace(next);
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
      if (activeIdentityKeyRef.current === identityKey) activeIdentityKeyRef.current = null;
      runtimeReadyRef.current = false;
      window.setTimeout(() => {
        void enqueueBrowserActivation(async () => {
          if (currentBrowserLease?.generation !== lease
            && currentBrowserLease?.identityKey === identityKey) return;
          const leaseBeingDeactivated = activeBrowserLease;
          if (leaseBeingDeactivated?.identityKey !== identityKey) return;
          await deactivateTaskBrowser({ identity });
          if (activeBrowserLease?.generation === leaseBeingDeactivated.generation) {
            activeBrowserLease = null;
          }
          if (currentBrowserLease?.generation === lease) currentBrowserLease = null;
        }).catch(() => undefined);
      }, 50);
    };
  }, [
    commitWorkspace,
    enqueueSuspensionSync,
    identity.scope,
    identity.sessionId,
    recoverFromSuspensionFailure,
    runtimeRetryRevision,
  ]);

  useEffect(() => {
    const generation = generationRef.current;
    if (!runtimeReadyRef.current) return;
    void enqueueSuspensionSync(generation)
      .catch((error) => recoverFromSuspensionFailure(generation, error));
  }, [
    enqueueSuspensionSync,
    identity.scope,
    identity.sessionId,
    recoverFromSuspensionFailure,
    shouldSuspend,
  ]);

  useEffect(() => {
    const interval = window.setInterval(() => {
      if (!runtimeReadyRef.current) return;
      void refresh(generationRef.current);
    }, 400);
    return () => window.clearInterval(interval);
  }, [identity.scope, identity.sessionId, refresh]);

  const activeTab = useMemo(() => {
    if (!workspace?.activeTabId) return null;
    return workspace.tabs.find((tab) => tab.id === workspace.activeTabId) ?? null;
  }, [workspace]);

  const runTabAction = useCallback(async (
    action: (payload: { identity: SessionIdentity; tabId: string }) => Promise<void>,
  ) => {
    const tabId = workspaceRef.current?.activeTabId;
    if (!tabId) return false;
    pageRequestRevisionRef.current += 1;
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

  const navigateWithIntent = useCallback(async (
    url: string,
    approvedLoopbackOrigin?: string,
    explicitReopen = false,
  ): Promise<TaskBrowserNavigateOutcome> => {
    const tabId = workspaceRef.current?.activeTabId;
    if (!tabId) return { kind: 'failed' };
    pageRequestRevisionRef.current += 1;
    setBusy(true);
    try {
      await navigateTaskBrowser({
        identity,
        tabId,
        url,
        ...(approvedLoopbackOrigin ? { approvedLoopbackOrigin } : {}),
        ...(explicitReopen ? { explicitReopen: true } : {}),
      });
      manualLoopbackTabsRef.current.delete(tabId);
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

  const navigate = useCallback(
    (url: string, approvedLoopbackOrigin?: string) => (
      navigateWithIntent(url, approvedLoopbackOrigin)
    ),
    [navigateWithIntent],
  );

  const reopen = useCallback(
    (url: string, approvedLoopbackOrigin?: string) => (
      navigateWithIntent(url, approvedLoopbackOrigin, true)
    ),
    [navigateWithIntent],
  );

  const runHistoryAction = useCallback(async (
    direction: 'back' | 'forward',
    approvedLoopbackOrigin?: string,
  ): Promise<TaskBrowserNavigateOutcome> => {
    const current = workspaceRef.current;
    const tab = current?.tabs.find((candidate) => candidate.id === current.activeTabId);
    const index = tab?.currentHistoryIndex;
    const targetIndex = index === null || index === undefined
      ? null
      : direction === 'back' ? index - 1 : index + 1;
    const target = targetIndex === null || targetIndex < 0 ? null : tab?.history[targetIndex]?.url;
    if (!tab || !target) return { kind: 'failed' };
    pageRequestRevisionRef.current += 1;
    setBusy(true);
    try {
      const payload = {
        identity,
        tabId: tab.id,
        ...(approvedLoopbackOrigin ? { approvedLoopbackOrigin } : {}),
      };
      await (direction === 'back' ? backTaskBrowser(payload) : forwardTaskBrowser(payload));
      manualLoopbackTabsRef.current.delete(tab.id);
      setErrorMessage(null);
      window.setTimeout(() => void refresh(generationRef.current), 160);
      return { kind: 'opened' };
    } catch (error) {
      if (isIpcError(error) && error.kind === 'NeedsApproval') {
        try { return { kind: 'needsApproval', origin: new URL(target).origin }; }
        catch { return { kind: 'failed' }; }
      }
      setErrorMessage(productError(error));
      return { kind: 'failed' };
    } finally {
      setBusy(false);
    }
  }, [identity.scope, identity.sessionId, refresh]);

  const saveLocalWorkspace = useCallback(async (next: BrowserWorkspace) => {
    const generation = generationRef.current;
    const writeRevision = ++workspaceWriteRevisionRef.current;
    try {
      const saved = await saveBrowserWorkspace({ identity, workspace: next });
      const restored = markManualLoopbackTabs(saved.workspace, manualLoopbackTabsRef.current);
      if (generation === generationRef.current
        && writeRevision === workspaceWriteRevisionRef.current) {
        commitWorkspace(restored);
        setErrorMessage(null);
      }
      return restored;
    } catch (error) {
      setErrorMessage(productError(error));
      return null;
    }
  }, [commitWorkspace, identity.scope, identity.sessionId]);

  const setLayout = useCallback((layoutMode: BrowserLayoutMode) => (
    enqueueWorkspaceMutation(async () => {
      const current = workspaceRef.current;
      if (!current) return false;
      return (await saveLocalWorkspace({ ...current, layoutMode })) !== null;
    })
  ), [enqueueWorkspaceMutation, saveLocalWorkspace]);

  const setSplitWidth = useCallback((splitWidthPx: number) => (
    enqueueWorkspaceMutation(async () => {
      const current = workspaceRef.current;
      if (!current) return false;
      return (await saveLocalWorkspace({ ...current, splitWidthPx })) !== null;
    })
  ), [enqueueWorkspaceMutation, saveLocalWorkspace]);

  const openTab = useCallback(() => (
    enqueueWorkspaceMutation(async () => {
      const current = workspaceRef.current;
      if (!current || current.tabs.length >= 5) return false;
      pageRequestRevisionRef.current += 1;
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
    })
  ), [enqueueWorkspaceMutation, identity.scope, identity.sessionId, saveLocalWorkspace]);

  const closeTab = useCallback((tabId: string) => (
    enqueueWorkspaceMutation(async () => {
      const current = workspaceRef.current;
      if (!current || current.tabs.length <= 1) return false;
      if (current.activeTabId === tabId) pageRequestRevisionRef.current += 1;
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
    })
  ), [enqueueWorkspaceMutation, identity.scope, identity.sessionId, saveLocalWorkspace]);

  const selectTab = useCallback((tabId: string) => (
    enqueueWorkspaceMutation(async () => {
      const current = workspaceRef.current;
      if (!current || !current.tabs.some((tab) => tab.id === tabId)) return false;
      if (current.activeTabId !== tabId) pageRequestRevisionRef.current += 1;
      try {
        await selectTaskBrowserTab({ identity, tabId });
        return (await saveLocalWorkspace({ ...current, activeTabId: tabId })) !== null;
      } catch (error) {
        setErrorMessage(productError(error));
        return false;
      }
    })
  ), [enqueueWorkspaceMutation, identity.scope, identity.sessionId, saveLocalWorkspace]);

  const captureText = useCallback(async (captureKind: BrowserCaptureKind) => {
    const tabId = workspaceRef.current?.activeTabId;
    if (!tabId) return { kind: 'failed' } as const;
    const generation = generationRef.current;
    const pageRevision = pageRequestRevisionRef.current;
    try {
      const captured = await captureTaskBrowserText({ identity, tabId, captureKind });
      if (generation !== generationRef.current
        || pageRevision !== pageRequestRevisionRef.current) return { kind: 'failed' } as const;
      return { kind: 'captured', ...captured } as const;
    } catch (error) {
      if (generation === generationRef.current
        && pageRevision === pageRequestRevisionRef.current
        && !isTaskBrowserCapturePageChanged(error)) setErrorMessage(productError(error));
      return { kind: 'failed' } as const;
    }
  }, [identity.scope, identity.sessionId]);

  const captureScreenshot = useCallback(async () => {
    const tabId = workspaceRef.current?.activeTabId;
    if (!tabId) return { kind: 'failed' } as const;
    const generation = generationRef.current;
    const pageRevision = pageRequestRevisionRef.current;
    try {
      const captured = await captureTaskBrowserScreenshot({ identity, tabId });
      if (generation !== generationRef.current
        || pageRevision !== pageRequestRevisionRef.current) return { kind: 'failed' } as const;
      return { kind: 'captured', ...captured } as const;
    } catch (error) {
      if (generation === generationRef.current
        && pageRevision === pageRequestRevisionRef.current
        && !isTaskBrowserCapturePageChanged(error)) setErrorMessage(productError(error));
      return { kind: 'failed' } as const;
    }
  }, [identity.scope, identity.sessionId]);

  return {
    workspace,
    recoveryNotice,
    activeTab,
    busy,
    errorMessage,
    suspended,
    runtimeReady: runtimeState === 'ready',
    overlaySafe: (suspended && suspensionProofValid) || runtimeState === 'inactive',
    retryRuntime: () => setRuntimeRetryRevision((revision) => revision + 1),
    navigate,
    reopen,
    back: (approvedLoopbackOrigin) => runHistoryAction('back', approvedLoopbackOrigin),
    forward: (approvedLoopbackOrigin) => runHistoryAction('forward', approvedLoopbackOrigin),
    reload: () => runTabAction(reloadTaskBrowser),
    setGeometry: async (host) => {
      geometryRef.current = host;
      if (!runtimeReadyRef.current) return;
      await setTaskBrowserGeometry({ identity, host });
    },
    setLayout,
    setSplitWidth,
    openTab,
    closeTab,
    selectTab,
    captureText,
    captureScreenshot,
  };
}

function withDeadline<T>(promise: Promise<T>, timeoutMs: number): Promise<T> {
  return new Promise<T>((resolve, reject) => {
    const handle = window.setTimeout(
      () => reject(new Error('Browser suspension acknowledgement timed out.')),
      timeoutMs,
    );
    promise.then(
      (value) => {
        window.clearTimeout(handle);
        resolve(value);
      },
      (error) => {
        window.clearTimeout(handle);
        reject(error);
      },
    );
  });
}

function enqueueBrowserActivation(operation: () => Promise<void>): Promise<void> {
  const run = browserActivationQueue.then(operation, operation);
  browserActivationQueue = run.then(() => undefined, () => undefined);
  return run;
}

function taskBrowserIdentityKey(identity: SessionIdentity): string {
  return `${identity.scope}:${identity.sessionId}`;
}

function currentPageKey(workspace: BrowserWorkspace | null): string | null {
  if (!workspace?.activeTabId) return null;
  const activeTab = workspace.tabs.find((tab) => tab.id === workspace.activeTabId);
  if (!activeTab) return null;
  return `${activeTab.id}:${activeTab.currentHistoryIndex ?? 'blank'}:${currentUrl(activeTab) ?? ''}`;
}

function rememberRecoveryNotice(
  identityKey: string,
  notice: BrowserWorkspaceRecovery,
): RecoveryNoticeHandoff {
  const handoff = { notice };
  recoveryNoticeHandoffs.delete(identityKey);
  recoveryNoticeHandoffs.set(identityKey, handoff);
  if (recoveryNoticeHandoffs.size > MAX_RECOVERY_NOTICE_HANDOFFS) {
    const oldestIdentityKey = recoveryNoticeHandoffs.keys().next().value;
    if (oldestIdentityKey !== undefined) recoveryNoticeHandoffs.delete(oldestIdentityKey);
  }
  return handoff;
}

function clearRecoveryNotice(identityKey: string, handoff: RecoveryNoticeHandoff): void {
  if (recoveryNoticeHandoffs.get(identityKey) === handoff) {
    recoveryNoticeHandoffs.delete(identityKey);
  }
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

function restoredLoopbackTabIds(workspace: BrowserWorkspace): Set<string> {
  if (workspace.scope !== 'project') return new Set();
  return new Set(workspace.tabs.flatMap((tab) => {
    const url = currentUrl(tab);
    return url && isLoopbackUrl(url) ? [tab.id] : [];
  }));
}

function markManualLoopbackTabs(
  workspace: BrowserWorkspace,
  tabIds: ReadonlySet<string>,
): BrowserWorkspace {
  return {
    ...workspace,
    tabs: workspace.tabs.map((tab) => (
      tabIds.has(tab.id)
        ? { ...tab, manualReopenRequired: true, restorationStatus: 'manualReopenRequired' }
        : tab
    )),
  };
}

function isLoopbackUrl(value: string): boolean {
  try {
    const host = new URL(value).hostname.toLowerCase();
    if (host === 'localhost' || host.endsWith('.localhost') || host === '[::1]' || host === '::1') return true;
    const octets = host.split('.').map(Number);
    return octets.length === 4 && (octets[0] === 127 || octets[0] === 0);
  } catch { return false; }
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
