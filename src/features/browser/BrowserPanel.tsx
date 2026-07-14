import { useEffect, useRef, useState } from 'react';
import type { FormEvent } from 'react';

import type { BrowserCaptureKind } from '../../lib/api/browser';
import type { ContextSourceRef } from '../../lib/api/chat';
import type { AddContextSourceResult } from '../chat/contextSources';
import { useBrowserWorkspace } from './useBrowserWorkspace';

type PendingLocalApproval = {
  url: string;
  origin: string;
};

export function BrowserPanel({
  onUseInChat,
}: {
  onUseInChat?: (source: ContextSourceRef) => Promise<AddContextSourceResult>;
}) {
  const browser = useBrowserWorkspace();
  const [address, setAddress] = useState('');
  const [editing, setEditing] = useState(false);
  const [localError, setLocalError] = useState<string | null>(null);
  const [pendingApproval, setPendingApproval] = useState<PendingLocalApproval | null>(null);
  const [captureNotice, setCaptureNotice] = useState<string | null>(null);
  const [capturePending, setCapturePending] = useState(false);
  const mountedRef = useRef(false);

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
    };
  }, []);

  useEffect(() => {
    if (!editing && browser.state?.currentUrl) setAddress(browser.state.currentUrl);
  }, [browser.state?.currentUrl, editing]);

  const openAddress = async (event: FormEvent) => {
    event.preventDefault();
    setLocalError(null);
    setPendingApproval(null);
    const normalized = normalizeAddress(address);
    if (normalized === null) {
      setLocalError('Enter a valid web address.');
      return;
    }
    setAddress(normalized);
    const outcome = await browser.open(normalized);
    if (outcome.kind === 'needsApproval') {
      setPendingApproval({ url: normalized, origin: outcome.origin });
    }
  };

  const confirmLocalSite = async () => {
    if (!pendingApproval) return;
    const approved = pendingApproval;
    setPendingApproval(null);
    await browser.open(approved.url, approved.origin);
  };

  const usePageText = async (captureKind: BrowserCaptureKind) => {
    if (!onUseInChat) return;
    setCaptureNotice(null);
    setCapturePending(true);
    const outcome = await browser.captureText(captureKind);
    if (outcome.kind !== 'captured') {
      if (mountedRef.current) setCapturePending(false);
      return;
    }
    let result: AddContextSourceResult;
    try {
      result = await onUseInChat({
        kind: 'browserTextEvidence',
        evidenceId: outcome.evidence.evidenceId,
      });
    } catch {
      if (mountedRef.current) {
        setCapturePending(false);
        setCaptureNotice('Project chat changed. Try again.');
      }
      return;
    }
    if (!mountedRef.current) return;
    setCapturePending(false);
    const noun = captureKind === 'selection' ? 'selection' : 'page text';
    if (result === 'added' || result === 'duplicate') {
      setCaptureNotice(`Added ${noun} to project chat.`);
    } else if (result === 'full') {
      setCaptureNotice('Chat context is full. Remove something and try again.');
    } else {
      setCaptureNotice('Project chat changed. Try again.');
    }
  };

  const state = browser.state;
  const currentHost = hostLabel(state?.currentUrl ?? state?.requestedUrl);
  const message = statusMessage(browser.initialLoading, state, currentHost);
  const displayedError = localError ?? browser.errorMessage;
  const controlsDisabled = browser.busy || !state?.open;

  return (
    <main className="plume-browser" aria-label="Browser">
      <header className="plume-browser-header">
        <div>
          <h2>Browser</h2>
          <p>Sandboxed window</p>
        </div>
      </header>

      <form className="plume-browser-toolbar" onSubmit={(event) => void openAddress(event)}>
        <button type="button" onClick={() => void browser.back()} disabled={controlsDisabled}>
          Back
        </button>
        <button type="button" onClick={() => void browser.forward()} disabled={controlsDisabled}>
          Forward
        </button>
        <button type="button" onClick={() => void browser.reload()} disabled={controlsDisabled}>
          Reload
        </button>
        <label>
          <input
            aria-label="Web address"
            value={address}
            placeholder="Search or enter address"
            autoCapitalize="none"
            autoCorrect="off"
            spellCheck={false}
            onFocus={() => setEditing(true)}
            onBlur={() => setEditing(false)}
            onChange={(event) => setAddress(event.target.value)}
          />
        </label>
        <button type="submit" disabled={browser.busy || address.trim().length === 0}>
          Go
        </button>
        <button type="button" onClick={() => void browser.focus()} disabled={controlsDisabled}>
          Show
        </button>
        <button type="button" onClick={() => void browser.close()} disabled={controlsDisabled}>
          Close
        </button>
      </form>

      {pendingApproval ? (
        <section className="plume-browser-approval" aria-label="Local site approval">
          <div>
            <strong>Allow this local site?</strong>
            <code>{pendingApproval.origin}</code>
            <p>Allowed until you close the sandboxed window.</p>
          </div>
          <div className="plume-browser-approval-actions">
            <button type="button" onClick={() => setPendingApproval(null)}>
              Cancel
            </button>
            <button type="button" onClick={() => void confirmLocalSite()}>
              Open local site
            </button>
          </div>
        </section>
      ) : null}

      <section className="plume-browser-capture" aria-label="Use page text in chat">
        <div>
          <strong>Use what you found</strong>
          <p>
            {onUseInChat
              ? 'Add selected text or the visible page text to your project chat.'
              : 'Open a trusted project to use page text in chat.'}
          </p>
        </div>
        <div className="plume-browser-capture-actions">
          <button
            type="button"
            disabled={controlsDisabled || capturePending || !onUseInChat}
            onClick={() => void usePageText('selection')}
          >
            Use selection in chat
          </button>
          <button
            type="button"
            disabled={controlsDisabled || capturePending || !onUseInChat}
            onClick={() => void usePageText('page')}
          >
            Use page text in chat
          </button>
        </div>
        {captureNotice ? <p role="status">{captureNotice}</p> : null}
      </section>

      <section className="plume-browser-state" aria-live="polite">
        <p>{message}</p>
        {state?.failure?.reason === 'loopbackApprovalRequired' ? (
          <p>A page tried to open a local site. Enter its address above to approve it.</p>
        ) : null}
        {displayedError ? <p role="status">{displayedError}</p> : null}
      </section>
    </main>
  );
}

function normalizeAddress(raw: string): string | null {
  const trimmed = raw.trim();
  if (trimmed.length === 0) return null;
  const withScheme = /^[a-z][a-z\d+.-]*:\/\//i.test(trimmed)
    ? trimmed
    : `${looksLocal(trimmed) ? 'http' : 'https'}://${trimmed}`;
  try {
    return new URL(withScheme).toString();
  } catch {
    return null;
  }
}

function looksLocal(value: string): boolean {
  try {
    const host = new URL(`https://${value}`).hostname.toLowerCase();
    if (host === 'localhost' || host.endsWith('.localhost')) return true;
    if (host === '[::1]' || host === '::1') return true;
    const octets = host.split('.');
    return (
      octets.length === 4 &&
      octets[0] === '127' &&
      octets.every((octet) => /^\d{1,3}$/.test(octet) && Number(octet) <= 255)
    );
  } catch {
    return false;
  }
}

function hostLabel(raw: string | null | undefined): string | null {
  if (!raw) return null;
  try {
    return new URL(raw).hostname;
  } catch {
    return null;
  }
}

function statusMessage(
  initialLoading: boolean,
  state: ReturnType<typeof useBrowserWorkspace>['state'],
  host: string | null,
): string {
  if (initialLoading) return 'Checking the sandboxed window…';
  if (!state?.open) return 'No page open.';
  if (state.loading) return `Opening ${host ?? 'page'}…`;
  return host ? `${host} is open in the sandboxed window.` : 'Page open in the sandboxed window.';
}
