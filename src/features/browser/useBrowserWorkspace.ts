import { useCallback, useEffect, useRef, useState } from 'react';

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
  type BrowserCaptureKind,
  type BrowserEvidenceSummary,
  type BrowserScreenshotSummary,
  type BrowserSandboxState,
} from '../../lib/api/browser';
import { isIpcError } from '../../lib/api/errors';

const LOADING_POLL_MS = 500;
const IDLE_POLL_MS = 2_000;

export type BrowserOpenOutcome =
  | { kind: 'opened' }
  | { kind: 'needsApproval'; origin: string }
  | { kind: 'failed' };

export type BrowserCaptureOutcome =
  | { kind: 'captured'; evidence: BrowserEvidenceSummary }
  | { kind: 'failed' };

export type BrowserScreenshotOutcome =
  | { kind: 'captured'; evidence: BrowserScreenshotSummary }
  | { kind: 'failed' };

export type BrowserWorkspace = {
  state: BrowserSandboxState | null;
  initialLoading: boolean;
  busy: boolean;
  errorMessage: string | null;
  refresh: () => void;
  open: (url: string, approvedLoopbackOrigin?: string) => Promise<BrowserOpenOutcome>;
  focus: () => Promise<boolean>;
  back: () => Promise<boolean>;
  forward: () => Promise<boolean>;
  reload: () => Promise<boolean>;
  captureText: (captureKind: BrowserCaptureKind) => Promise<BrowserCaptureOutcome>;
  captureScreenshot: () => Promise<BrowserScreenshotOutcome>;
  close: () => Promise<boolean>;
};

export function useBrowserWorkspace(): BrowserWorkspace {
  const [state, setState] = useState<BrowserSandboxState | null>(null);
  const [busy, setBusy] = useState(false);
  const [errorMessage, setErrorMessage] = useState<string | null>(null);
  const mountedRef = useRef(false);
  const generationRef = useRef(0);
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const readStateRef = useRef<() => void>(() => undefined);

  const clearPoll = useCallback(() => {
    if (timerRef.current !== null) {
      clearTimeout(timerRef.current);
      timerRef.current = null;
    }
  }, []);

  const scheduleRead = useCallback(
    (delay: number) => {
      clearPoll();
      if (!mountedRef.current) return;
      timerRef.current = setTimeout(() => readStateRef.current(), delay);
    },
    [clearPoll],
  );

  const schedulePoll = useCallback(
    (next: BrowserSandboxState) => {
      scheduleRead(next.loading ? LOADING_POLL_MS : IDLE_POLL_MS);
    },
    [scheduleRead],
  );

  const readState = useCallback(async () => {
    const generation = ++generationRef.current;
    try {
      const next = await getBrowserSandboxState();
      if (!mountedRef.current || generation !== generationRef.current) return;
      setState(next);
      setErrorMessage(null);
      schedulePoll(next);
    } catch (error) {
      if (!mountedRef.current || generation !== generationRef.current) return;
      setErrorMessage(browserErrorMessage(error));
      scheduleRead(IDLE_POLL_MS);
    }
  }, [schedulePoll, scheduleRead]);
  readStateRef.current = () => void readState();

  useEffect(() => {
    mountedRef.current = true;
    void readState();
    return () => {
      mountedRef.current = false;
      generationRef.current += 1;
      clearPoll();
    };
  }, [clearPoll, readState]);

  const refresh = useCallback(() => {
    clearPoll();
    void readState();
  }, [clearPoll, readState]);

  const runAction = useCallback(
    async (action: () => Promise<BrowserSandboxState>): Promise<boolean> => {
      clearPoll();
      const generation = ++generationRef.current;
      if (mountedRef.current) setBusy(true);
      try {
        const next = await action();
        if (!mountedRef.current || generation !== generationRef.current) return false;
        setState(next);
        setErrorMessage(null);
        schedulePoll(next);
        return true;
      } catch (error) {
        if (!mountedRef.current || generation !== generationRef.current) return false;
        setErrorMessage(browserErrorMessage(error));
        scheduleRead(IDLE_POLL_MS);
        return false;
      } finally {
        if (mountedRef.current && generation === generationRef.current) setBusy(false);
      }
    },
    [clearPoll, schedulePoll, scheduleRead],
  );

  const open = useCallback(
    async (url: string, approvedLoopbackOrigin?: string): Promise<BrowserOpenOutcome> => {
      clearPoll();
      const generation = ++generationRef.current;
      if (mountedRef.current) setBusy(true);
      try {
        const next = await openBrowserSandbox(url, approvedLoopbackOrigin);
        if (!mountedRef.current || generation !== generationRef.current) return { kind: 'failed' };
        setState(next);
        setErrorMessage(null);
        schedulePoll(next);
        return { kind: 'opened' };
      } catch (error) {
        if (!mountedRef.current || generation !== generationRef.current) return { kind: 'failed' };
        if (isIpcError(error) && error.kind === 'NeedsApproval') {
          const origin = safeOrigin(url);
          if (origin !== null) {
            scheduleRead(IDLE_POLL_MS);
            return { kind: 'needsApproval', origin };
          }
        }
        setErrorMessage(browserErrorMessage(error));
        scheduleRead(IDLE_POLL_MS);
        return { kind: 'failed' };
      } finally {
        if (mountedRef.current && generation === generationRef.current) setBusy(false);
      }
    },
    [clearPoll, schedulePoll, scheduleRead],
  );

  const captureText = useCallback(
    async (captureKind: BrowserCaptureKind): Promise<BrowserCaptureOutcome> => {
      clearPoll();
      const generation = ++generationRef.current;
      if (mountedRef.current) setBusy(true);
      try {
        const evidence = await captureBrowserText(captureKind);
        if (!mountedRef.current || generation !== generationRef.current) return { kind: 'failed' };
        setErrorMessage(null);
        if (state) schedulePoll(state);
        return { kind: 'captured', evidence };
      } catch (error) {
        if (!mountedRef.current || generation !== generationRef.current) return { kind: 'failed' };
        setErrorMessage(captureErrorMessage(error, captureKind));
        scheduleRead(IDLE_POLL_MS);
        return { kind: 'failed' };
      } finally {
        if (mountedRef.current && generation === generationRef.current) setBusy(false);
      }
    },
    [clearPoll, schedulePoll, scheduleRead, state],
  );

  const captureScreenshot = useCallback(async (): Promise<BrowserScreenshotOutcome> => {
    clearPoll();
    const generation = ++generationRef.current;
    if (mountedRef.current) setBusy(true);
    try {
      const evidence = await captureBrowserScreenshot();
      if (!mountedRef.current || generation !== generationRef.current) return { kind: 'failed' };
      setErrorMessage(null);
      if (state) schedulePoll(state);
      return { kind: 'captured', evidence };
    } catch (error) {
      if (!mountedRef.current || generation !== generationRef.current) return { kind: 'failed' };
      setErrorMessage(screenshotErrorMessage(error));
      scheduleRead(IDLE_POLL_MS);
      return { kind: 'failed' };
    } finally {
      if (mountedRef.current && generation === generationRef.current) setBusy(false);
    }
  }, [clearPoll, schedulePoll, scheduleRead, state]);

  return {
    state,
    initialLoading: state === null && errorMessage === null,
    busy,
    errorMessage,
    refresh,
    open,
    focus: useCallback(() => runAction(focusBrowserSandbox), [runAction]),
    back: useCallback(() => runAction(backBrowserSandbox), [runAction]),
    forward: useCallback(() => runAction(forwardBrowserSandbox), [runAction]),
    reload: useCallback(() => runAction(reloadBrowserSandbox), [runAction]),
    captureText,
    captureScreenshot,
    close: useCallback(() => runAction(closeBrowserSandbox), [runAction]),
  };
}

function screenshotErrorMessage(error: unknown): string {
  if (isIpcError(error)) {
    if (error.kind === 'NeedsApproval') return 'Open a trusted project first.';
    if (error.kind === 'NotFound') return 'Open a page first.';
    if (error.kind === 'Blocked') return 'Screenshot capture is unavailable right now.';
  }
  return 'Could not capture the screenshot. Try again.';
}

function captureErrorMessage(error: unknown, captureKind: BrowserCaptureKind): string {
  if (isIpcError(error)) {
    if (error.kind === 'BadArgument') {
      return captureKind === 'selection'
        ? 'Select some text on the page first.'
        : 'No readable page text found.';
    }
    if (error.kind === 'NeedsApproval') return 'Open a trusted project first.';
    if (error.kind === 'NotFound') return 'Open a page first.';
    if (error.kind === 'Blocked') return 'Capture is unavailable right now.';
  }
  return 'Could not capture page text. Try again.';
}

function safeOrigin(url: string): string | null {
  try {
    return new URL(url).origin;
  } catch {
    return null;
  }
}

function browserErrorMessage(error: unknown): string {
  if (isIpcError(error)) {
    switch (error.kind) {
      case 'BadArgument':
        return 'Enter a valid web address.';
      case 'Blocked':
        return 'That address cannot be opened safely.';
      case 'NotFound':
        return 'The browser window is closed.';
      default:
        return 'Browser unavailable. Try again.';
    }
  }
  return 'Browser unavailable. Try again.';
}
