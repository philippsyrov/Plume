// D37 + D43: Memory panel.
//
// Visible surface for local project memory. Shows the current
// entries, lets the user remember a new one, forget any entry,
// and (D43) search across the redacted text. Pure read/write
// through the `memory.*` IPC family — see `src/lib/api/memory.ts`.
//
// Scope:
//   * Render `MemoryIndex.entries` as a flat list. Each row has
//     the redacted text, a small "n redacted" badge when
//     applicable, and a Forget button.
//   * A short input + Remember button at the top. Disabled when
//     a request is in flight; on success the input clears and
//     the panel refetches.
//   * D43: a tiny search field above the list with 200 ms
//     debounce. While a non-empty query is active the result view
//     replaces the entry list; clearing the field returns to the
//     full list.
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
//   * SQLite / FTS / semantic search — the D43 brief explicitly
//     keeps this file-based; the SQLite path is a follow-up.

import { useCallback, useEffect, useState } from 'react';

import {
  applyMemoryDistill,
  forgetMemory,
  getMemoryDistillPreview,
  getMemoryIndex,
  rememberMemory,
  searchMemory,
  MEMORY_SEARCH_MAX_QUERY_BYTES,
  type MemoryDistillApplyFailure,
  type MemoryDistillPreview,
  type MemoryDuplicateGroup,
  type MemoryEntry,
  type MemoryForgetFailure,
  type MemoryIndex,
  type MemoryRememberFailure,
  type MemorySearchFailure,
  type MemorySearchHit,
} from '../../lib/api/memory';
import { isIpcError } from '../../lib/api/errors';
import { bumpMemoryRevision } from './memoryRevision';

type LoadState =
  | { kind: 'idle' }
  | { kind: 'loading' }
  | { kind: 'ready'; index: MemoryIndex }
  | { kind: 'needs-trust' }
  | { kind: 'error'; message: string };

type SearchState =
  | { kind: 'idle' }
  | { kind: 'loading' }
  | { kind: 'results'; hits: MemorySearchHit[]; truncated: boolean; query: string }
  | { kind: 'error'; message: string };

/** D54: distill-preview affordance. `idle` = button hidden body;
 *  `loading` = waiting on `memory.distillPreview`; `ready` = result
 *  displayed inline; `error` = surface the failure under the toggle.
 *  Read-only — no apply, no delete. */
type DistillState =
  | { kind: 'idle' }
  | { kind: 'loading' }
  | { kind: 'ready'; preview: MemoryDistillPreview }
  | { kind: 'error'; message: string };

/** Debounce delay for the search input. Keeps the IPC quiet while
 * the user is still typing without losing responsiveness. */
const SEARCH_DEBOUNCE_MS = 200;
/** D43 result cap — the backend rejects > 50, the UI asks for fewer. */
const SEARCH_LIMIT = 20;

export function MemoryPanel() {
  const [state, setState] = useState<LoadState>({ kind: 'idle' });
  const [draft, setDraft] = useState('');
  const [busy, setBusy] = useState(false);
  const [rememberError, setRememberError] = useState<string | null>(null);
  // D43: search-as-you-type. `query` is the input value; the
  // debounced fetch lives in the effect below.
  const [query, setQuery] = useState('');
  const [searchState, setSearchState] = useState<SearchState>({ kind: 'idle' });
  // D54: distill-preview affordance — collapsed by default; fetches
  // on toggle and stays cached until the user collapses it again.
  const [distillExpanded, setDistillExpanded] = useState(false);
  const [distillState, setDistillState] = useState<DistillState>({ kind: 'idle' });
  // D64: apply (compact) affordance. `distillBusy` disables the
  // buttons during the rewrite; `distillNotice` surfaces the outcome
  // ("Removed 3 duplicates." / a store error / "store changed").
  const [distillBusy, setDistillBusy] = useState(false);
  const [distillNotice, setDistillNotice] = useState<string | null>(null);

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

  // D43: debounced search effect. Clears the result state when the
  // query empties; otherwise schedules a fetch SEARCH_DEBOUNCE_MS
  // after the last keystroke. Each effect run captures a local
  // `cancelled` flag so a stale fetch can't overwrite a fresher one.
  useEffect(() => {
    const trimmed = query.trim();
    if (trimmed.length === 0) {
      setSearchState({ kind: 'idle' });
      return;
    }
    if (trimmed.length > MEMORY_SEARCH_MAX_QUERY_BYTES) {
      setSearchState({
        kind: 'error',
        message: `Query is too long (max ${MEMORY_SEARCH_MAX_QUERY_BYTES} characters).`,
      });
      return;
    }
    let cancelled = false;
    setSearchState({ kind: 'loading' });
    const handle = window.setTimeout(async () => {
      try {
        const resp = await searchMemory(trimmed, SEARCH_LIMIT);
        if (cancelled) return;
        if (resp.ok) {
          setSearchState({
            kind: 'results',
            hits: resp.hits,
            truncated: resp.truncated,
            query: resp.query,
          });
        } else {
          setSearchState({
            kind: 'error',
            message: `${searchFailureLabel(resp.reason)} — ${resp.message}`,
          });
        }
      } catch (err: unknown) {
        if (cancelled) return;
        const message =
          isIpcError(err) && err.kind === 'NeedsApproval'
            ? 'Trust the project to search memory.'
            : err instanceof Error
              ? err.message
              : String(err);
        setSearchState({ kind: 'error', message });
      }
    }, SEARCH_DEBOUNCE_MS);
    return () => {
      cancelled = true;
      window.clearTimeout(handle);
    };
  }, [query]);

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
        // D42 Codex fix: tell the chat-context preview hook the
        // memory store changed so the chat header's MemoryBadge
        // re-fetches against fresh counts. Only on `ok` — a
        // rejected remember didn't change anything on disk.
        bumpMemoryRevision();
      } else {
        setRememberError(`${rememberFailureLabel(resp.reason)} — ${resp.message}`);
      }
    } catch (err) {
      setRememberError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusy(false);
    }
  }, [busy, draft, refresh]);

  // D54: fetch the distillation preview. Same trust-gate behaviour
  // as the index/search fetches — `NeedsApproval` collapses the
  // disclosure with a hint.
  const fetchDistill = useCallback(async () => {
    setDistillState({ kind: 'loading' });
    setDistillNotice(null);
    try {
      const preview = await getMemoryDistillPreview();
      setDistillState({ kind: 'ready', preview });
    } catch (err: unknown) {
      const message =
        isIpcError(err) && err.kind === 'NeedsApproval'
          ? 'Trust the project to preview distillation.'
          : err instanceof Error
            ? err.message
            : String(err);
      setDistillState({ kind: 'error', message });
    }
  }, []);

  const onToggleDistill = useCallback(() => {
    const next = !distillExpanded;
    setDistillExpanded(next);
    if (next && distillState.kind === 'idle') {
      void fetchDistill();
    }
  }, [distillExpanded, distillState.kind, fetchDistill]);

  // D64: compact the confirmed duplicate groups. Hard delete of the
  // non-survivor entries — the preview the user just looked at IS the
  // confirmation step (each group shows its surviving text). On success
  // we resync the index, the chat-context badge, and the preview itself
  // so the disclosure reflects the post-apply store.
  const onApplyDistill = useCallback(
    async (groupIds: string[]) => {
      if (groupIds.length === 0 || distillBusy) return;
      setDistillBusy(true);
      setDistillNotice(null);
      try {
        const resp = await applyMemoryDistill(groupIds);
        if (resp.ok) {
          if (resp.removedEntryCount === 0) {
            setDistillNotice('Nothing to compact — the store changed since the preview.');
          } else {
            const n = resp.removedEntryCount;
            setDistillNotice(`Removed ${n} duplicate${n === 1 ? '' : 's'}.`);
          }
          bumpMemoryRevision();
          await refresh();
          await fetchDistill();
        } else {
          setDistillNotice(`${distillApplyFailureLabel(resp.reason)} — ${resp.message}`);
        }
      } catch (err: unknown) {
        const message =
          isIpcError(err) && err.kind === 'NeedsApproval'
            ? 'Trust the project to compact memory.'
            : err instanceof Error
              ? err.message
              : String(err);
        setDistillNotice(message);
      } finally {
        setDistillBusy(false);
      }
    },
    [distillBusy, fetchDistill, refresh],
  );

  const onForget = useCallback(
    async (entryId: string) => {
      if (busy) return;
      setBusy(true);
      setRememberError(null);
      try {
        const resp = await forgetMemory(entryId);
        if (!resp.ok) {
          // Codex D37 LOW: in-band forget failures (badId,
          // storeFailed) were previously dropped on the floor —
          // the panel silently refreshed and the entry stayed.
          // Surface the failure under the input the same way
          // remember does.
          setRememberError(`${forgetFailureLabel(resp.reason)} — ${resp.message}`);
        } else {
          // D42 Codex fix: bump the memory revision so the chat
          // header's MemoryBadge refetches. Only on `ok` — a
          // rejected forget didn't change the store. The `removed`
          // field can still be false (idempotent no-op for an
          // already-gone id); we bump anyway because the UI's
          // mental model is "the user clicked Forget, the panel
          // should resync everything that watches memory."
          bumpMemoryRevision();
        }
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
        <>
          <input
            type="search"
            className="plume-memory-search"
            placeholder="Search memory…"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            maxLength={MEMORY_SEARCH_MAX_QUERY_BYTES}
            aria-label="Search project memory"
          />
          <MemorySearchResults
            state={searchState}
            busy={busy}
            onForget={(entryId) => void onForget(entryId)}
          />
          {searchState.kind === 'idle' && (
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
          <DistillPreviewDisclosure
            expanded={distillExpanded}
            state={distillState}
            applyBusy={distillBusy}
            notice={distillNotice}
            onToggle={onToggleDistill}
            onRefresh={() => void fetchDistill()}
            onApply={(groupIds) => void onApplyDistill(groupIds)}
          />
        </>
      )}
    </section>
  );
}

/**
 * D54: tiny "Find duplicates" affordance. The toggle is always
 * available when the memory store has ≥1 entry; clicking opens a
 * disclosure that fetches `memory.distillPreview` and renders the
 * candidate groups inline. Read-only — there is no apply / delete
 * here. A future `memory.distillApply` slice will add the
 * affirmative action; the preview verb landed first so the user can
 * see what an apply WOULD do before any rewrite path exists.
 *
 * The disclosure is a peer of the search results, not a child of an
 * individual row, because duplication is a property of the whole
 * store. Refresh re-runs the verb against the current on-disk state
 * so the user can preview after remembering / forgetting without
 * collapsing and re-expanding.
 */
function DistillPreviewDisclosure({
  expanded,
  state,
  applyBusy,
  notice,
  onToggle,
  onRefresh,
  onApply,
}: {
  expanded: boolean;
  state: DistillState;
  applyBusy: boolean;
  notice: string | null;
  onToggle: () => void;
  onRefresh: () => void;
  onApply: (groupIds: string[]) => void;
}) {
  return (
    <div className="plume-memory-distill">
      <button
        type="button"
        className="plume-memory-distill-toggle"
        onClick={onToggle}
        aria-expanded={expanded}
      >
        <span className="plume-local-models-caret" aria-hidden="true">
          {expanded ? '▾' : '▸'}
        </span>
        Find duplicates
      </button>
      {expanded ? (
        <DistillPreviewBody
          state={state}
          applyBusy={applyBusy}
          notice={notice}
          onRefresh={onRefresh}
          onApply={onApply}
        />
      ) : null}
    </div>
  );
}

function DistillPreviewBody({
  state,
  applyBusy,
  notice,
  onRefresh,
  onApply,
}: {
  state: DistillState;
  applyBusy: boolean;
  notice: string | null;
  onRefresh: () => void;
  onApply: (groupIds: string[]) => void;
}) {
  if (state.kind === 'loading' || state.kind === 'idle') {
    return (
      <p className="plume-memory-hint" role="status">
        Scanning entries…
      </p>
    );
  }
  if (state.kind === 'error') {
    return (
      <div>
        <p className="plume-memory-error" role="alert">
          {state.message}
        </p>
        <button type="button" className="plume-memory-distill-refresh" onClick={onRefresh}>
          Retry
        </button>
      </div>
    );
  }
  const { preview } = state;
  if (preview.duplicateGroups.length === 0) {
    return (
      <div>
        <p className="plume-memory-hint">
          No duplicates found among {preview.totalEntries}{' '}
          {preview.totalEntries === 1 ? 'entry' : 'entries'}.
        </p>
        {notice !== null && (
          <p className="plume-memory-hint" role="status">
            {notice}
          </p>
        )}
        <button type="button" className="plume-memory-distill-refresh" onClick={onRefresh}>
          Refresh
        </button>
      </div>
    );
  }
  // D64: apply every previewed group. The preview list above is the
  // confirmation surface — each row shows the surviving (newest) text —
  // so the button removes all non-survivors in one pass. Hard delete;
  // no undo in v1 (the JSONL is hand-editable).
  const allGroupIds = preview.duplicateGroups.map((group) => group.id);
  return (
    <div>
      <p className="plume-memory-hint">
        {preview.duplicateGroups.length}{' '}
        {preview.duplicateGroups.length === 1 ? 'duplicate group' : 'duplicate groups'} ·{' '}
        compact from {preview.totalEntries} to {preview.totalEntries - preview.wouldRemove}{' '}
        entries
      </p>
      <ul className="plume-memory-distill-groups" role="list">
        {preview.duplicateGroups.map((group) => (
          <DistillGroupRow key={group.id} group={group} />
        ))}
      </ul>
      {notice !== null && (
        <p className="plume-memory-hint" role="status">
          {notice}
        </p>
      )}
      <div className="plume-memory-distill-actions">
        <button
          type="button"
          className="plume-memory-distill-apply"
          onClick={() => onApply(allGroupIds)}
          disabled={applyBusy}
          title="Remove every duplicate, keeping the newest of each group"
        >
          {applyBusy
            ? 'Compacting…'
            : `Compact ${preview.wouldRemove} duplicate${preview.wouldRemove === 1 ? '' : 's'}`}
        </button>
        <button
          type="button"
          className="plume-memory-distill-refresh"
          onClick={onRefresh}
          disabled={applyBusy}
        >
          Refresh
        </button>
      </div>
    </div>
  );
}

function DistillGroupRow({ group }: { group: MemoryDuplicateGroup }) {
  const survivor = group.entries[0];
  return (
    <li className="plume-memory-distill-group">
      <div className="plume-memory-distill-text" title="Newest entry — would survive an apply">
        {survivor?.text ?? '(empty group)'}
      </div>
      <p className="plume-memory-hint">
        {group.entries.length} {group.entries.length === 1 ? 'entry' : 'entries'} ·{' '}
        {group.removableCount} {group.removableCount === 1 ? 'duplicate' : 'duplicates'} would be
        removed
      </p>
    </li>
  );
}

function MemorySearchResults({
  state,
  busy,
  onForget,
}: {
  state: SearchState;
  busy: boolean;
  onForget: (entryId: string) => void;
}) {
  if (state.kind === 'idle') return null;
  if (state.kind === 'loading') {
    return (
      <p className="plume-memory-hint" role="status">
        Searching…
      </p>
    );
  }
  if (state.kind === 'error') {
    return (
      <p className="plume-memory-error" role="alert">
        {state.message}
      </p>
    );
  }
  if (state.hits.length === 0) {
    return (
      <p className="plume-memory-empty">No matches for {JSON.stringify(state.query)}.</p>
    );
  }
  return (
    <>
      <p className="plume-memory-hint">
        {state.hits.length} {state.hits.length === 1 ? 'match' : 'matches'}
        {state.truncated ? ' (more dropped to fit cap)' : ''}
      </p>
      <ul className="plume-memory-list" role="list">
        {state.hits.map((hit) => (
          <MemoryRow
            key={hit.entry.id}
            entry={hit.entry}
            busy={busy}
            onForget={() => onForget(hit.entry.id)}
          />
        ))}
      </ul>
    </>
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

function forgetFailureLabel(reason: MemoryForgetFailure): string {
  switch (reason) {
    case 'badId':
      return 'Invalid entry id';
    case 'storeFailed':
      return 'Storage error';
  }
}

function distillApplyFailureLabel(reason: MemoryDistillApplyFailure): string {
  switch (reason) {
    case 'storeFailed':
      return 'Storage error';
  }
}

function searchFailureLabel(reason: MemorySearchFailure): string {
  switch (reason) {
    case 'emptyQuery':
      return 'Empty query';
    case 'queryTooLong':
      return 'Query too long';
    case 'badLimit':
      return 'Bad limit';
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
