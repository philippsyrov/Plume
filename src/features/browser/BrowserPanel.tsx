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
import type { BrowserTab } from '../../lib/api/browserWorkspace';
import type { ContextSourceRef } from '../../lib/api/chat';
import type { SessionIdentity } from '../../lib/api/sessions';
import type { AddContextSourceResult } from '../chat/contextSources';
import { formatBytes } from '../chat/formatters';
import { Icon } from '../project-shell/Icon';
import { currentUrl, useTaskBrowser } from './useTaskBrowser';

type PendingLocalApproval =
  | { action: 'navigate'; url: string; origin: string }
  | { action: 'reopen'; url: string; origin: string }
  | { action: 'back' | 'forward'; origin: string };

const CAPTURE_NOTICE_MS = 2_000;

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
  const addressDirtyRef = useRef(false);
  const addressFocusedRef = useRef(false);
  const addressCommitRef = useRef<string | null>(null);
  const addressSubmitPointerRef = useRef(false);
  const [localError, setLocalError] = useState<string | null>(null);
  const [pendingApproval, setPendingApproval] = useState<PendingLocalApproval | null>(null);
  const [captureNotice, setCaptureNotice] = useState<string | null>(null);
  const [dismissedBrowserError, setDismissedBrowserError] = useState<string | null>(null);
  const [recoveryDismissed, setRecoveryDismissed] = useState(false);
  const [capturePending, setCapturePending] = useState(false);
  const hostRef = useRef<HTMLDivElement>(null);
  const rootRef = useRef<HTMLElement>(null);
  const resizeCleanupRef = useRef<(() => void) | null>(null);
  const keyboardResizeQueueRef = useRef<Promise<void>>(Promise.resolve());
  const keyboardResizePendingRef = useRef(0);
  const splitWidthRef = useRef(560);
  const [dragWidth, setDragWidth] = useState<number | null>(null);
  const [containerWidth, setContainerWidth] = useState<number | null>(null);
  const [attachOpen, setAttachOpen] = useState(false);
  const [expandedChatOpen, setExpandedChatOpen] = useState(false);
  const attachRef = useRef<HTMLDivElement>(null);
  const attachMenuRef = useRef<HTMLDivElement>(null);
  const attachButtonRef = useRef<HTMLButtonElement>(null);
  const firstAttachItemRef = useRef<HTMLButtonElement>(null);
  const mountedRef = useRef(false);

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
      resizeCleanupRef.current?.();
    };
  }, []);

  useEffect(() => {
    if (!captureNotice) return;
    const handle = window.setTimeout(() => setCaptureNotice(null), CAPTURE_NOTICE_MS);
    return () => window.clearTimeout(handle);
  }, [captureNotice]);

  useEffect(() => {
    if (dismissedBrowserError !== null && browser.errorMessage !== dismissedBrowserError) {
      setDismissedBrowserError(null);
    }
  }, [browser.errorMessage, dismissedBrowserError]);

  useEffect(() => {
    if (browser.recoveryNotice === null) setRecoveryDismissed(false);
  }, [browser.recoveryNotice]);

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
    const root = rootRef.current;
    if (!root) return;
    const report = () => {
      const width = root.getBoundingClientRect().width;
      if (width > 0) setContainerWidth(width);
    };
    report();
    const observer = new ResizeObserver(report);
    observer.observe(root);
    return () => observer.disconnect();
  }, []);

  useEffect(() => {
    const url = browser.activeTab ? currentUrl(browser.activeTab) : null;
    const commit = browser.activeTab ? committedAddressKey(browser.activeTab) : null;
    const committedPageChanged = addressCommitRef.current !== commit;
    addressCommitRef.current = commit;
    if (committedPageChanged) {
      addressDirtyRef.current = false;
      setAddress(url ?? '');
      return;
    }
    if (!(addressDirtyRef.current && addressFocusedRef.current)) setAddress(url ?? '');
  }, [browser.activeTab]);

  useEffect(() => {
    if (!attachOpen) return;
    firstAttachItemRef.current?.focus();
    const dismissOutside = (event: MouseEvent) => {
      const target = event.target as Node;
      if (!attachRef.current?.contains(target) && !attachMenuRef.current?.contains(target)) {
        setAttachOpen(false);
      }
    };
    const dismissWithEscape = (event: KeyboardEvent) => {
      if (event.key !== 'Escape') return;
      event.preventDefault();
      setAttachOpen(false);
      attachButtonRef.current?.focus();
    };
    const dismissForNativePageFocus = () => {
      setAttachOpen(false);
    };
    document.addEventListener('mousedown', dismissOutside);
    document.addEventListener('keydown', dismissWithEscape);
    window.addEventListener('blur', dismissForNativePageFocus);
    return () => {
      document.removeEventListener('mousedown', dismissOutside);
      document.removeEventListener('keydown', dismissWithEscape);
      window.removeEventListener('blur', dismissForNativePageFocus);
    };
  }, [attachOpen]);

  const navigateTo = async (url: string, explicitReopen = false) => {
    setLocalError(null);
    setPendingApproval(null);
    const outcome = await browser[explicitReopen ? 'reopen' : 'navigate'](url);
    if (outcome.kind === 'needsApproval') {
      if (identity.scope === 'project') {
        setPendingApproval({ action: explicitReopen ? 'reopen' : 'navigate', url, origin: outcome.origin });
      }
      else setLocalError('Open a project chat to test a local site.');
    }
  };

  const openAddress = async (event: FormEvent) => {
    event.preventDefault();
    addressSubmitPointerRef.current = false;
    const normalized = normalizeAddress(address);
    if (!normalized) {
      setLocalError('Enter a valid web address.');
      return;
    }
    setAddress(normalized);
    addressDirtyRef.current = false;
    await navigateTo(normalized);
  };

  const reopenPage = async () => {
    const url = browser.activeTab ? currentUrl(browser.activeTab) : null;
    if (url) await navigateTo(url, true);
  };

  const moveHistory = async (action: 'back' | 'forward') => {
    addressDirtyRef.current = false;
    setLocalError(null);
    setPendingApproval(null);
    const outcome = await browser[action]();
    if (outcome.kind === 'needsApproval') {
      if (identity.scope === 'project') setPendingApproval({ action, origin: outcome.origin });
      else setLocalError('Open a project chat to test a local site.');
    }
  };

  const confirmLocalSite = async () => {
    if (!pendingApproval) return;
    const approved = pendingApproval;
    setPendingApproval(null);
    if (approved.action === 'navigate' || approved.action === 'reopen') {
      await browser[approved.action](approved.url, approved.origin);
    }
    else await browser[approved.action](approved.origin);
  };

  const captureText = async (kind: BrowserCaptureKind) => {
    setCapturePending(true);
    setCaptureNotice(null);
    const outcome = await browser.captureText(kind);
    if (outcome.kind === 'captured' && mountedRef.current) await handoff(outcome.source, captureSuccessMessage(outcome.evidence, 'added'));
    else if (mountedRef.current) setCapturePending(false);
  };

  const captureScreenshot = async () => {
    setCapturePending(true);
    setCaptureNotice(null);
    const outcome = await browser.captureScreenshot();
    if (outcome.kind === 'captured' && mountedRef.current) {
      await handoff(
        outcome.source,
        `Added screenshot · ${outcome.evidence.width}×${outcome.evidence.height} · ${formatBytes(outcome.evidence.bytes)}.`,
      );
    } else if (mountedRef.current) setCapturePending(false);
  };

  const handoff = async (source: ContextSourceRef, success: string) => {
    if (!mountedRef.current) return;
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
  const maxSplitWidth = containerWidth === null
    ? 1_600
    : Math.max(320, Math.min(1_600, containerWidth - 368));
  const preferredSplitWidth = dragWidth ?? browser.workspace?.splitWidthPx ?? 560;
  const splitWidth = expanded
    ? Math.min(1_600, Math.max(320, preferredSplitWidth))
    : Math.min(maxSplitWidth, Math.max(320, preferredSplitWidth));
  const tabs = browser.workspace?.tabs ?? [];
  const activeTabId = browser.workspace?.activeTabId ?? null;
  const activeTabLabel = hostLabel(browser.activeTab ? currentUrl(browser.activeTab) : null) ?? 'New page';
  const browserError = browser.errorMessage === dismissedBrowserError
    ? null
    : browser.errorMessage;
  const recoveryNotice = browser.recoveryNotice === 'browserStateReset' && !recoveryDismissed
    ? 'Browser state was reset because its saved data was damaged. Your chat is safe.'
    : null;
  const notice = captureNotice ?? localError ?? browserError ?? recoveryNotice;
  const hasChromeStack = attachOpen || pendingApproval !== null || notice !== null;
  const captureDisabled = browser.busy || capturePending || !browser.activeTab
    || browser.activeTab.manualReopenRequired || !currentUrl(browser.activeTab);
  const activeIndex = browser.activeTab?.currentHistoryIndex;
  const canGoBack = activeIndex !== null && activeIndex !== undefined && activeIndex > 0;
  const canGoForward = activeIndex !== null && activeIndex !== undefined
    && activeIndex + 1 < (browser.activeTab?.history.length ?? 0);

  useEffect(() => {
    if (captureDisabled) setAttachOpen(false);
  }, [captureDisabled]);

  const dismissNotice = () => {
    if (captureNotice !== null) {
      setCaptureNotice(null);
      return;
    }
    if (localError !== null) {
      setLocalError(null);
      return;
    }
    if (browserError !== null) {
      setDismissedBrowserError(browser.errorMessage);
      return;
    }
    if (recoveryNotice !== null) setRecoveryDismissed(true);
  };

  useEffect(() => {
    if (keyboardResizePendingRef.current === 0) splitWidthRef.current = splitWidth;
  }, [splitWidth]);

  const beginResize = (event: ReactPointerEvent<HTMLButtonElement>) => {
    if (!rootRef.current || expanded) return;
    event.preventDefault();
    resizeCleanupRef.current?.();
    const startX = event.clientX;
    const startWidth = splitWidth;
    let latestWidth = startWidth;
    const maxWidth = Math.max(320, Math.min(1_600, rootRef.current.getBoundingClientRect().width - 368));
    const move = (moveEvent: PointerEvent) => {
      latestWidth = Math.round(Math.min(maxWidth, Math.max(320, startWidth - (moveEvent.clientX - startX))));
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
      splitWidthRef.current = latestWidth;
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
    const width = Math.round(Math.min(maxSplitWidth, Math.max(320, splitWidthRef.current + delta)));
    splitWidthRef.current = width;
    keyboardResizePendingRef.current += 1;
    setDragWidth(width);
    keyboardResizeQueueRef.current = keyboardResizeQueueRef.current
      .then(async () => { await browser.setSplitWidth(width); })
      .catch(() => undefined)
      .finally(() => {
        keyboardResizePendingRef.current -= 1;
        if (keyboardResizePendingRef.current === 0 && mountedRef.current) setDragWidth(null);
      });
  };

  const selectTabWithKeyboard = (tabId: string, key: string) => {
    const currentIndex = tabs.findIndex((tab) => tab.id === tabId);
    if (currentIndex < 0 || tabs.length === 0) return;
    let nextIndex: number | null = null;
    if (key === 'ArrowLeft') nextIndex = currentIndex > 0 ? currentIndex - 1 : tabs.length - 1;
    if (key === 'ArrowRight') nextIndex = currentIndex < tabs.length - 1 ? currentIndex + 1 : 0;
    if (key === 'Home') nextIndex = 0;
    if (key === 'End') nextIndex = tabs.length - 1;
    if (nextIndex === null) return;
    const nextTab = tabs[nextIndex];
    if (!nextTab) return;
    resetAddressDraft();
    document.getElementById(browserTabDomId(nextTab.id))?.focus();
    void browser.selectTab(nextTab.id);
  };

  const closeAttachAndFocus = () => {
    setAttachOpen(false);
    attachButtonRef.current?.focus();
  };

  const resetAddressDraft = () => {
    addressDirtyRef.current = false;
    setAddress(browser.activeTab ? currentUrl(browser.activeTab) ?? '' : '');
  };

  return (
    <main
      ref={rootRef}
      className={`plume-browser plume-browser-${expanded ? 'expanded' : 'split'}${expanded && expandedChatOpen ? ' has-chat-open' : ''}`}
      aria-label="Browser"
      style={{ '--plume-browser-split-width': `${splitWidth}px` } as CSSProperties}
    >
      {!expanded || expandedChatOpen ? (
        <aside className="plume-browser-chat" aria-label="Task chat">{chatPane}</aside>
      ) : null}
      {!expanded ? (
        <button
          type="button"
          className="plume-browser-resizer"
          role="separator"
          aria-label="Resize Browser and chat"
          aria-orientation="vertical"
          aria-valuemin={320}
          aria-valuemax={maxSplitWidth}
          aria-valuenow={splitWidth}
          onPointerDown={beginResize}
          onKeyDown={(event) => {
            if (event.key === 'ArrowLeft') { event.preventDefault(); resizeByKeyboard(24); }
            if (event.key === 'ArrowRight') { event.preventDefault(); resizeByKeyboard(-24); }
          }}
        />
      ) : null}
      <section className={`plume-browser-page${hasChromeStack ? ' has-chrome-stack' : ''}`}>
        <div className="plume-browser-tabs">
          <div className="plume-browser-tablist" role="tablist" aria-label="Browser tabs">
            {tabs.map((tab) => (
              <button
                key={tab.id}
                id={browserTabDomId(tab.id)}
                type="button"
                className={`plume-browser-tab plume-browser-tab-select${tab.id === activeTabId ? ' is-active' : ''}`}
                role="tab"
                aria-selected={tab.id === activeTabId}
                aria-controls="plume-browser-tabpanel"
                tabIndex={tab.id === activeTabId ? 0 : -1}
                onClick={() => {
                  resetAddressDraft();
                  void browser.selectTab(tab.id);
                }}
                onKeyDown={(event) => {
                  if (!['ArrowLeft', 'ArrowRight', 'Home', 'End'].includes(event.key)) return;
                  event.preventDefault();
                  selectTabWithKeyboard(tab.id, event.key);
                }}
              >
                <span>{hostLabel(currentUrl(tab)) ?? 'New page'}</span>
              </button>
            ))}
          </div>
          <div className="plume-browser-tab-controls">
            {tabs.length > 1 && activeTabId ? (
              <button
                type="button"
                className="plume-browser-icon-button"
                aria-label="Close current tab"
                onClick={() => {
                  resetAddressDraft();
                  void browser.closeTab(activeTabId);
                }}
              >
                <Icon name="close" size={13} />
              </button>
            ) : null}
            <button
              type="button"
              className="plume-browser-icon-button"
              aria-label="New browser tab"
              onClick={() => {
                resetAddressDraft();
                void browser.openTab();
              }}
              disabled={tabs.length >= 5}
            >
              <Icon name="plus" />
            </button>
            <button
              type="button"
              className="plume-browser-layout-button plume-browser-icon-button"
              onClick={() => void browser.setLayout(expanded ? 'split' : 'expanded')}
              aria-label={expanded ? 'Return to split view' : 'Expand Browser'}
            >
              <Icon name={expanded ? 'contract' : 'expand'} />
            </button>
            {expanded ? (
              <button
                type="button"
                className={`plume-browser-chat-toggle${expandedChatOpen ? ' is-open' : ''}`}
                aria-label={expandedChatOpen ? 'Hide chat' : 'Show chat'}
                aria-expanded={expandedChatOpen}
                onClick={() => setExpandedChatOpen((open) => !open)}
              >
                <Icon name="chevron-down" />
                <span>{expandedChatOpen ? 'Hide chat' : 'Show chat'}</span>
              </button>
            ) : null}
          </div>
        </div>

        <form className="plume-browser-toolbar" onSubmit={(event) => void openAddress(event)}>
          <button className="plume-browser-icon-button" type="button" aria-label="Back" onClick={() => void moveHistory('back')} disabled={browser.busy || !canGoBack}><Icon name="arrow-left" /></button>
          <button className="plume-browser-icon-button" type="button" aria-label="Forward" onClick={() => void moveHistory('forward')} disabled={browser.busy || !canGoForward}><Icon name="arrow-right" /></button>
          <button className="plume-browser-icon-button" type="button" aria-label="Reload" onClick={() => { resetAddressDraft(); void browser.reload(); }} disabled={browser.busy}><Icon name="reload" /></button>
          <input
            aria-label="Web address"
            value={address}
            placeholder="Search or enter address"
            autoCapitalize="none"
            autoCorrect="off"
            spellCheck={false}
            onFocus={() => { addressFocusedRef.current = true; }}
            onBlur={() => {
              addressFocusedRef.current = false;
              if (addressSubmitPointerRef.current) return;
              resetAddressDraft();
            }}
            onChange={(event) => {
              addressDirtyRef.current = true;
              setAddress(event.target.value);
            }}
          />
          <button
            type="submit"
            aria-label="Open address"
            disabled={browser.busy || !address.trim()}
            onPointerDown={() => { addressSubmitPointerRef.current = true; }}
            onPointerCancel={() => { addressSubmitPointerRef.current = false; }}
          >
            Go
          </button>
          <div className="plume-browser-attach" ref={attachRef}>
            <button
              ref={attachButtonRef}
              type="button"
              aria-label="Attach page evidence"
              aria-haspopup="menu"
              aria-expanded={attachOpen}
              disabled={captureDisabled}
              onClick={() => setAttachOpen((open) => !open)}
            >
              Attach
            </button>
          </div>
        </form>

        {hasChromeStack ? <div className="plume-browser-chrome-stack">{attachOpen ? (
          <div
            ref={attachMenuRef}
            className="plume-browser-attach-menu"
            role="menu"
            aria-label="Attach page evidence"
            onBlur={(event) => {
              const next = event.relatedTarget;
              if (next instanceof Node
                && (event.currentTarget.contains(next) || attachRef.current?.contains(next))) return;
              setAttachOpen(false);
            }}
            onKeyDown={(event) => {
              const items = Array.from(event.currentTarget.querySelectorAll<HTMLButtonElement>('[role="menuitem"]'));
              const current = items.indexOf(document.activeElement as HTMLButtonElement);
              let next: number | null = null;
              if (event.key === 'ArrowDown') next = current < items.length - 1 ? current + 1 : 0;
              if (event.key === 'ArrowUp') next = current > 0 ? current - 1 : items.length - 1;
              if (event.key === 'Home') next = 0;
              if (event.key === 'End') next = items.length - 1;
              if (event.key === 'Tab') setAttachOpen(false);
              if (next === null) return;
              event.preventDefault();
              items[next]?.focus();
            }}
          >
            <button
              ref={firstAttachItemRef}
              type="button"
              role="menuitem"
              onClick={() => { closeAttachAndFocus(); void captureText('selection'); }}
            >
              Selected text
            </button>
            <button
              type="button"
              role="menuitem"
              onClick={() => { closeAttachAndFocus(); void captureText('page'); }}
            >
              Readable page text
            </button>
            <button
              type="button"
              role="menuitem"
              onClick={() => { closeAttachAndFocus(); void captureScreenshot(); }}
            >
              Visible screenshot
            </button>
          </div>
        ) : null}

        {pendingApproval ? (
          <section className="plume-browser-approval" aria-label="Local site approval">
            <span><strong>Open this local site?</strong> {pendingApproval.origin}</span>
            <div><button type="button" onClick={() => setPendingApproval(null)}>Cancel</button><button type="button" onClick={() => void confirmLocalSite()}>Open</button></div>
          </section>
        ) : null}

        {notice ? (
          <div className="plume-browser-notice" role="status">
            <span>{notice}</span>
            <button
              type="button"
              aria-label="Dismiss Browser notice"
              onClick={dismissNotice}
            >
              <Icon name="close" size={13} />
            </button>
          </div>
        ) : null}
        </div> : null}

        <div
          id="plume-browser-tabpanel"
          className="plume-browser-host"
          ref={hostRef}
          role="tabpanel"
          aria-labelledby={activeTabId ? browserTabDomId(activeTabId) : undefined}
          aria-label={activeTabId ? undefined : activeTabLabel}
        >
          {browser.activeTab?.manualReopenRequired ? (
            <div className="plume-browser-manual-reopen">
              <p>For your privacy, reopen this page when you're ready.</p>
              <button type="button" disabled={browser.busy} onClick={() => void reopenPage()}>
                Reopen page
              </button>
            </div>
          ) : null}
          {!browser.activeTab || currentUrl(browser.activeTab) === null ? <p>Enter an address to start browsing.</p> : null}
        </div>
      </section>
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

function committedAddressKey(tab: BrowserTab): string {
  const index = tab.currentHistoryIndex;
  const current = index === null ? null : tab.history[index] ?? null;
  return [tab.id, index ?? 'blank', current?.position ?? 'blank', current?.url ?? '', current?.recordedAtMs ?? ''].join(':');
}

function browserTabDomId(tabId: string): string {
  return `plume-browser-tab-${tabId}`;
}
