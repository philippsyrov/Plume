// D37: Memory panel.
//
// Tiny visible surface for local project memory. Shows the
// current entries, lets the user remember a new one, and lets
// them forget any entry. Pure read/write through the
// `memory.*` IPC family — see `src/lib/api/memory.ts`.
//
// Scope:
//   * Render `MemoryIndex.entries` as a flat list. Each row has
//     the redacted text, a small "n redacted" badge when
//     applicable, and a Forget button.
//   * A short input + Remember button at the top. Disabled when
//     a request is in flight; on success the input clears and
//     the panel refetches.
//   * In-band failure messages from the backend show inline
//     under the input. Out-of-band trust-gate failures
//     (`NeedsApproval`) collapse the whole panel to a tiny
//     "trust the project to use memory" hint, matching how the
//     patch surfaces gate on trust.
//
// Not in this slice:
//   * Edit-in-place — only add/remove.
//   * Topic files (`USER.md`, `SOUL.md`, `topics/`) from the
//     LOCAL_AGENT_NORTH_STAR layout.
//   * Search / FTS / SQLite — the brief says file-based first.

import { useCallback, useEffect, useState } from 'react';

import {
  forgetMemory,
  getMemoryIndex,
  rememberMemory,
  type MemoryEntry,
  type MemoryIndex,
  type MemoryRememberFailure,
} from '../../lib/api/memory';
import { isIpcError } from '../../lib/api/errors';

type LoadState =
  | { kind: 'idle' }
  | { kind: 'loading' }
  | { kind: 'ready'; index: MemoryIndex }
  | { kind: 'needs-trust' }
  | { kind: 'error'; message: string };

export function MemoryPanel() {
  const [state, setState] = useState<LoadState>({ kind: 'idle' });
  const [draft, setDraft] = useState('');
  const [busy, setBusy] = useState(false);
  const [rememberError, setRememberError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    setState({ kind: 'loading' });
    try {
      const index = await getMemoryIndex();
      setState({ kind: 'ready', index });
    } catch (err) {
      if (isIpcError(err) && err.kind === 'NeedsApproval') {
        setState({ kind: 'needs-trust' });
        return;
      }
      setState({
        kind: 'error',
        message: err instanceof Error ? err.message : String(err),
      });
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const onRemember = useCallback(async () => {
    const text = draft.trim();
    if (!text || busy) return;
    setBusy(true);
    setRememberError(null);
    try {
      const resp = await rememberMemory(text);
      if (resp.ok) {
        setDraft('');
        await refresh();
      } else {
        setRememberError(`${rememberFailureLabel(resp.reason)} — ${resp.message}`);
      }
    } catch (err) {
      setRememberError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusy(false);
    }
  }, [busy, draft, refresh]);

  const onForget = useCallback(
    async (entryId: string) => {
      if (busy) return;
      setBusy(true);
      try {
        await forgetMemory(entryId);
        await refresh();
      } catch (err) {
        setRememberError(err instanceof Error ? err.message : String(err));
      } finally {
        setBusy(false);
      }
    },
    [busy, refresh],
  );

  if (state.kind === 'needs-trust') {
    return (
      <section className="plume-memory-card ink-panel" aria-label="Project memory">
        <h3>Memory</h3>
        <p className="plume-memory-empty">Trust the project to use memory.</p>
      </section>
    );
  }
  if (state.kind === 'loading' || state.kind === 'idle') {
    return (
      <section className="plume-memory-card ink-panel" aria-label="Project memory">
        <h3>Memory</h3>
        <p className="plume-memory-empty" role="status">
          Loading memory…
        </p>
      </section>
    );
  }
  if (state.kind === 'error') {
    return (
      <section className="plume-memory-card ink-panel" aria-label="Project memory">
        <h3>Memory</h3>
        <p className="plume-memory-error" role="alert">
          {state.message}
        </p>
      </section>
    );
  }

  const { entries, limits, totalBytes } = state.index;
  const atCapacity = entries.length >= limits.maxEntries;

  return (
    <section className="plume-memory-card ink-panel" aria-label="Project memory">
      <h3>Memory</h3>
      <p className="plume-memory-hint">
        {entries.length} of {limits.maxEntries} entries · {formatBytes(totalBytes)} used
      </p>
      <div className="plume-memory-form">
        <textarea
          className="plume-memory-input"
          placeholder={
            atCapacity
              ? 'Memory is full. Forget an entry to make room.'
              : 'Remember something about this project…'
          }
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          disabled={busy || atCapacity}
          maxLength={limits.maxBytesPerEntry}
          rows={2}
        />
        <button
          type="button"
          onClick={() => void onRemember()}
          disabled={busy || atCapacity || draft.trim().length === 0}
        >
          Remember
        </button>
      </div>
      {rememberError !== null && (
        <p className="plume-memory-error" role="alert">
          {rememberError}
        </p>
      )}
      {entries.length === 0 ? (
        <p className="plume-memory-empty">No memories yet.</p>
      ) : (
        <ul className="plume-memory-list" role="list">
          {entries.map((entry) => (
            <MemoryRow
              key={entry.id}
              entry={entry}
              busy={busy}
              onForget={() => void onForget(entry.id)}
            />
          ))}
        </ul>
      )}
    </section>
  );
}

function MemoryRow({
  entry,
  busy,
  onForget,
}: {
  entry: MemoryEntry;
  busy: boolean;
  onForget: () => void;
}) {
  return (
    <li className="plume-memory-row">
      <div className="plume-memory-row-body">
        <span className="plume-memory-text">{entry.text}</span>
        {entry.redactionCount > 0 && (
          <span
            className="plume-memory-badge"
            title={`${entry.redactionCount} secret value${entry.redactionCount === 1 ? '' : 's'} redacted before this was stored`}
          >
            {entry.redactionCount} redacted
          </span>
        )}
      </div>
      <button
        type="button"
        className="plume-memory-forget"
        onClick={onForget}
        disabled={busy}
        title="Remove this memory entry"
      >
        Forget
      </button>
    </li>
  );
}

function rememberFailureLabel(reason: MemoryRememberFailure): string {
  switch (reason) {
    case 'empty':
      return 'Empty';
    case 'tooLong':
      return 'Too long';
    case 'redactedToEmpty':
      return 'Nothing left after redaction';
    case 'capacityReached':
      return 'Memory full';
    case 'storeFailed':
      return 'Storage error';
  }
}

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  const kib = bytes / 1024;
  if (kib < 1024) return `${kib.toFixed(1)} KiB`;
  return `${(kib / 1024).toFixed(1)} MiB`;
}
