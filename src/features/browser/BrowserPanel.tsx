import { useEffect, useState } from 'react';
import type { FormEvent } from 'react';

import { useBrowserWorkspace } from './useBrowserWorkspace';

type PendingLocalApproval = {
  url: string;
  origin: string;
};

export function BrowserPanel() {
  const browser = useBrowserWorkspace();
  const [address, setAddress] = useState('');
  const [editing, setEditing] = useState(false);
  const [localError, setLocalError] = useState<string | null>(null);
  const [pendingApproval, setPendingApproval] = useState<PendingLocalApproval | null>(null);

  useEffect(() => {
    if (!editing && browser.state?.currentUrl) setAddress(browser.state.currentUrl);
  }, [browser.state?.currentUrl, editing]);

  const openAddress = async (event: FormEvent) => {
    event.preventDefault();
    setLocalError(null);
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
  if (value.toLowerCase().startsWith('[::1]')) return true;
  const host = value.split('/')[0]?.split(':')[0]?.toLowerCase() ?? '';
  return host === 'localhost' || host.endsWith('.localhost') || host.startsWith('127.');
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
