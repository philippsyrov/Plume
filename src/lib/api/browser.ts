import { invokeIpc } from './ipc';

export type BrowserNavigationFailureReason =
  | 'navigationFailed'
  | 'loopbackApprovalRequired';

export type BrowserNavigationFailure = {
  reason: BrowserNavigationFailureReason;
  message: string;
};

export type BrowserSandboxState = {
  open: boolean;
  windowLabel: string | null;
  requestedUrl: string | null;
  currentUrl: string | null;
  title: string | null;
  loading: boolean;
  failure: BrowserNavigationFailure | null;
};

export type BrowserCaptureKind = 'selection' | 'page';

export type BrowserEvidenceSummary = {
  evidenceId: string;
  captureKind: BrowserCaptureKind;
  sourceUrl: string;
  title: string | null;
  capturedAtMs: number;
  bytes: number;
  redactionCount: number;
  truncated: boolean;
  preview: string;
};

type EmptyPayload = Record<string, never>;

export function getBrowserSandboxState(): Promise<BrowserSandboxState> {
  return invokeIpc<EmptyPayload, BrowserSandboxState>('browser_sandbox_state', {});
}

export function openBrowserSandbox(
  url: string,
  approvedLoopbackOrigin?: string,
): Promise<BrowserSandboxState> {
  const payload =
    approvedLoopbackOrigin === undefined ? { url } : { url, approvedLoopbackOrigin };
  return invokeIpc<
    { url: string; approvedLoopbackOrigin?: string },
    BrowserSandboxState
  >('browser_sandbox_open', payload);
}

export function closeBrowserSandbox(): Promise<BrowserSandboxState> {
  return invokeIpc<EmptyPayload, BrowserSandboxState>('browser_sandbox_close', {});
}

export function focusBrowserSandbox(): Promise<BrowserSandboxState> {
  return invokeIpc<EmptyPayload, BrowserSandboxState>('browser_sandbox_focus', {});
}

export function backBrowserSandbox(): Promise<BrowserSandboxState> {
  return invokeIpc<EmptyPayload, BrowserSandboxState>('browser_sandbox_back', {});
}

export function forwardBrowserSandbox(): Promise<BrowserSandboxState> {
  return invokeIpc<EmptyPayload, BrowserSandboxState>('browser_sandbox_forward', {});
}

export function reloadBrowserSandbox(): Promise<BrowserSandboxState> {
  return invokeIpc<EmptyPayload, BrowserSandboxState>('browser_sandbox_reload', {});
}

export function captureBrowserText(
  captureKind: BrowserCaptureKind,
): Promise<BrowserEvidenceSummary> {
  return invokeIpc<{ captureKind: BrowserCaptureKind }, BrowserEvidenceSummary>(
    'browser_sandbox_capture_text',
    { captureKind },
  );
}
