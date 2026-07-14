import {
  useEffect,
  useRef,
  useState,
  type CSSProperties,
  type FormEvent,
  type PointerEvent as ReactPointerEvent,
  type ReactNode,
} from 'react';

import type { BrowserCaptureKind, BrowserEvidenceSummary } from '../../lib/api/browser';
import type { ContextSourceRef } from '../../lib/api/chat';
import type { SessionIdentity } from '../../lib/api/sessions';
import type { AddContextSourceResult } from '../chat/contextSources';
import { formatBytes } from '../chat/formatters';
import { currentUrl, useTaskBrowser } from './useTaskBrowser';

type PendingLocalApproval = { url: string; origin: string };

export function BrowserPanel({
  identity,
  chatPane,
  onUseInChat,
}: {
  identity: SessionIdentity;
  chatPane: ReactNode;
  onUseInChat: (source: ContextSourceRef) => Promise<AddContextSourceResult>;
}) {
  const browser = useTaskBrowser(identity);
  const [address, setAddress] = useState('');
  const [localError, setLocalError] = useState<string | null>(null);
  const [pendingApproval, setPendingApproval] = useState<PendingLocalApproval | null>(null);
  const [captureNotice, setCaptureNotice] = useState<string | null>(null);
  const [capturePending, setCapturePending] = useState(false);
  const hostRef = useRef<HTMLDivElement>(null);
  const rootRef = useRef<HTMLElement>(null);
  const resizeCleanupRef = useRef<(() => void) | null>(null);
  const [dragWidth, setDragWidth] = useState<number | null>(null);
  const mountedRef = useRef(false);

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
      resizeCleanupRef.current?.();
    };
  }, []);

  useEffect(() => {
    const host = hostRef.current;
    if (!host) return;
    const report = () => {
      const rect = host.getBoundingClientRect();
      if (rect.width < 1 || rect.height < 1) return;
      void browser.setGeometry({
        x: rect.x,
        y: rect.y,
        width: rect.width,
        height: rect.height,
        scaleFactor: window.devicePixelRatio,
      });
    };
    report();
    const observer = new ResizeObserver(report);
    observer.observe(host);
    window.addEventListener('resize', report);
    return () => {
      observer.disconnect();
      window.removeEventListener('resize', report);
    };
  }, [browser.workspace?.layoutMode, browser.activeTab?.id]);

  useEffect(() => {
    const url = browser.activeTab ? currentUrl(browser.activeTab) : null;
    setAddress(url ?? '');
  }, [browser.activeTab]);

  const openAddress = async (event: FormEvent) => {
    event.preventDefault();
    setLocalError(null);
    setPendingApproval(null);
    const normalized = normalizeAddress(address);
    if (!normalized) {
      setLocalError('Enter a valid web address.');
      return;
    }
    setAddress(normalized);
    const outcome = await browser.navigate(normalized);
    if (outcome.kind === 'needsApproval') {
      if (identity.scope === 'project') setPendingApproval({ url: normalized, origin: outcome.origin });
      else setLocalError('Open a project chat to test a local site.');
    }
  };

  const confirmLocalSite = async () => {
    if (!pendingApproval) return;
    const approved = pendingApproval;
    setPendingApproval(null);
    await browser.navigate(approved.url, approved.origin);
  };

  const captureText = async (kind: BrowserCaptureKind) => {
    setCapturePending(true);
    setCaptureNotice(null);
    const outcome = await browser.captureText(kind);
    if (outcome.kind === 'captured') await handoff(outcome.source, captureSuccessMessage(outcome.evidence, 'added'));
    else if (mountedRef.current) setCapturePending(false);
  };

  const captureScreenshot = async () => {
    setCapturePending(true);
    setCaptureNotice(null);
    const outcome = await browser.captureScreenshot();
    if (outcome.kind === 'captured') {
      await handoff(
        outcome.source,
        `Added screenshot · ${outcome.evidence.width}×${outcome.evidence.height} · ${formatBytes(outcome.evidence.bytes)}.`,
      );
    } else if (mountedRef.current) setCapturePending(false);
  };

  const handoff = async (source: ContextSourceRef, success: string) => {
    try {
      const result = await onUseInChat(source);
      if (!mountedRef.current) return;
      if (result === 'added') setCaptureNotice(success);
      else if (result === 'duplicate') setCaptureNotice('Already in this chat.');
      else if (result === 'full') setCaptureNotice('Chat context is full. Remove something and try again.');
      else setCaptureNotice('This chat changed. Try again.');
    } catch {
      if (mountedRef.current) setCaptureNotice('This chat changed. Try again.');
    } finally {
      if (mountedRef.current) setCapturePending(false);
    }
  };

  const expanded = browser.workspace?.layoutMode === 'expanded';
  const splitWidth = dragWidth ?? browser.workspace?.splitWidthPx ?? 560;
  const captureDisabled = browser.busy || capturePending || !browser.activeTab || !currentUrl(browser.activeTab);
  const activeIndex = browser.activeTab?.currentHistoryIndex;
  const canGoBack = activeIndex !== null && activeIndex !== undefined && activeIndex > 0;
  const canGoForward = activeIndex !== null && activeIndex !== undefined
    && activeIndex + 1 < (browser.activeTab?.history.length ?? 0);

  const beginResize = (event: ReactPointerEvent<HTMLButtonElement>) => {
    if (!rootRef.current || expanded) return;
    event.preventDefault();
    resizeCleanupRef.current?.();
    const startX = event.clientX;
    const startWidth = splitWidth;
    let latestWidth = startWidth;
    const maxWidth = Math.max(320, Math.min(1_600, rootRef.current.getBoundingClientRect().width - 308));
    const move = (moveEvent: PointerEvent) => {
      latestWidth = Math.round(Math.min(maxWidth, Math.max(320, startWidth + moveEvent.clientX - startX)));
      setDragWidth(latestWidth);
    };
    const cleanup = () => {
      window.removeEventListener('pointermove', move);
      window.removeEventListener('pointerup', finish);
      window.removeEventListener('pointercancel', cancel);
      resizeCleanupRef.current = null;
    };
    const finish = () => {
      cleanup();
      setDragWidth(null);
      void browser.setSplitWidth(latestWidth);
    };
    const cancel = () => {
      cleanup();
      setDragWidth(null);
    };
    resizeCleanupRef.current = cleanup;
    window.addEventListener('pointermove', move);
    window.addEventListener('pointerup', finish);
    window.addEventListener('pointercancel', cancel);
  };

  const resizeByKeyboard = (delta: number) => {
    const width = Math.round(Math.min(1_600, Math.max(320, splitWidth + delta)));
    void browser.setSplitWidth(width);
  };

  return (
    <main
      ref={rootRef}
      className={`plume-browser plume-browser-${expanded ? 'expanded' : 'split'}`}
      aria-label="Browser"
      style={{ '--plume-browser-split-width': `${splitWidth}px` } as CSSProperties}
    >
      <section className={`plume-browser-page${pendingApproval ? ' has-approval' : ''}`}>
        <div className="plume-browser-tabs" aria-label="Browser tabs">
          {browser.workspace?.tabs.map((tab) => (
            <button
              type="button"
              key={tab.id}
              className={tab.id === browser.workspace?.activeTabId ? 'is-active' : ''}
              onClick={() => void browser.selectTab(tab.id)}
            >
              <span>{hostLabel(currentUrl(tab)) ?? 'New page'}</span>
              {browser.workspace && browser.workspace.tabs.length > 1 ? (
                <span
                  role="button"
                  tabIndex={0}
                  aria-label={`Close ${hostLabel(currentUrl(tab)) ?? 'tab'}`}
                  onClick={(event) => { event.stopPropagation(); void browser.closeTab(tab.id); }}
                  onKeyDown={(event) => {
                    if (event.key === 'Enter' || event.key === ' ') {
                      event.stopPropagation();
                      void browser.closeTab(tab.id);
                    }
                  }}
                >×</span>
              ) : null}
            </button>
          ))}
          <button type="button" aria-label="New browser tab" onClick={() => void browser.openTab()} disabled={(browser.workspace?.tabs.length ?? 5) >= 5}>+</button>
          <button
            type="button"
            className="plume-browser-layout-button"
            onClick={() => void browser.setLayout(expanded ? 'split' : 'expanded')}
            aria-label={expanded ? 'Show chat beside Browser' : 'Expand Browser'}
          >{expanded ? '⇲' : '⇱'}</button>
        </div>

        <form className="plume-browser-toolbar" onSubmit={(event) => void openAddress(event)}>
          <button type="button" aria-label="Back" onClick={() => void browser.back()} disabled={browser.busy || !canGoBack}>←</button>
          <button type="button" aria-label="Forward" onClick={() => void browser.forward()} disabled={browser.busy || !canGoForward}>→</button>
          <button type="button" aria-label="Reload" onClick={() => void browser.reload()} disabled={browser.busy}>↻</button>
          <input
            aria-label="Web address"
            value={address}
            placeholder="Search or enter address"
            autoCapitalize="none"
            autoCorrect="off"
            spellCheck={false}
            onChange={(event) => setAddress(event.target.value)}
          />
          <button type="submit" aria-label="Open address" disabled={browser.busy || !address.trim()}>Go</button>
        </form>

        {pendingApproval ? (
          <section className="plume-browser-approval" aria-label="Local site approval">
            <span><strong>Open this local site?</strong> {pendingApproval.origin}</span>
            <div><button type="button" onClick={() => setPendingApproval(null)}>Cancel</button><button type="button" onClick={() => void confirmLocalSite()}>Open</button></div>
          </section>
        ) : null}

        <div className="plume-browser-host" ref={hostRef} aria-label="Web page">
          {browser.activeTab?.manualReopenRequired ? <p>For your privacy, reopen this local page manually.</p> : null}
          {!browser.activeTab || currentUrl(browser.activeTab) === null ? <p>Enter an address to start browsing.</p> : null}
        </div>

        <div className="plume-browser-evidence">
          <span>{captureNotice ?? localError ?? browser.errorMessage ?? 'Add only what you choose to this chat.'}</span>
          <button type="button" disabled={captureDisabled} onClick={() => void captureText('selection')}>Use selection</button>
          <button type="button" disabled={captureDisabled} onClick={() => void captureText('page')}>Use page</button>
          <button type="button" disabled={captureDisabled} onClick={() => void captureScreenshot()}>Screenshot</button>
        </div>
      </section>
      {!expanded ? (
        <button
          type="button"
          className="plume-browser-resizer"
          role="separator"
          aria-label="Resize Browser and chat"
          aria-orientation="vertical"
          aria-valuemin={320}
          aria-valuemax={1600}
          aria-valuenow={splitWidth}
          onPointerDown={beginResize}
          onKeyDown={(event) => {
            if (event.key === 'ArrowLeft') { event.preventDefault(); resizeByKeyboard(-24); }
            if (event.key === 'ArrowRight') { event.preventDefault(); resizeByKeyboard(24); }
          }}
        />
      ) : null}
      <aside className="plume-browser-chat" aria-label="Task chat">{chatPane}</aside>
    </main>
  );
}

function captureSuccessMessage(evidence: BrowserEvidenceSummary, result: 'added'): string {
  const noun = evidence.captureKind === 'selection' ? 'selection' : 'page text';
  const details = [`${result === 'added' ? 'Added' : 'Already added'} ${noun} from ${hostLabel(evidence.sourceUrl) ?? 'page'}`, formatBytes(evidence.bytes)];
  if (evidence.redactionCount) details.push(`${evidence.redactionCount} redacted`);
  if (evidence.truncated) details.push('shortened');
  return `${details.join(' · ')}.`;
}

function normalizeAddress(raw: string): string | null {
  const trimmed = raw.trim();
  if (!trimmed) return null;
  const withScheme = /^[a-z][a-z\d+.-]*:\/\//i.test(trimmed) ? trimmed : `${looksLocal(trimmed) ? 'http' : 'https'}://${trimmed}`;
  try { return new URL(withScheme).toString(); } catch { return null; }
}

function looksLocal(value: string): boolean {
  try {
    const host = new URL(`https://${value}`).hostname.toLowerCase();
    if (host === 'localhost' || host.endsWith('.localhost') || host === '[::1]' || host === '::1') return true;
    const octets = host.split('.').map(Number);
    return octets.length === 4 && (octets[0] === 127 || octets.every((part) => Number.isInteger(part) && part >= 0 && part <= 255) && octets[0] === 0);
  } catch { return false; }
}

function hostLabel(url: string | null | undefined): string | null {
  if (!url) return null;
  try { return new URL(url).host; } catch { return null; }
}
