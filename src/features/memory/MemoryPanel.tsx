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
//   * Topic files (`USER.md`, `SOUL.md`, `topics/`) from the
//     LOCAL_AGENT_NORTH_STAR layout.
//   * SQLite / FTS / semantic search — the D43 brief explicitly
//     keeps this file-based; the SQLite path is a follow-up.

import { useCallback, useEffect, useRef, useState } from 'react';

import {
  applyMemoryDistill,
  forgetMemory,
  getMemoryDistillLog,
  getMemoryDistillPreview,
  getMemoryIndex,
  getMemoryTopics,
  rememberMemory,
  searchMemory,
  setMemoryLinks,
  updateMemory,
  MEMORY_SEARCH_MAX_QUERY_BYTES,
  type MemoryDistillLogEntry,
  type MemoryEntry,
  type MemoryForgetFailure,
  type MemoryIndex,
  type MemoryTopics,
  type MemoryRememberFailure,
  type MemorySearchFailure,
  type MemorySearchHit,
  type MemoryUpdateFailure,
} from '../../lib/api/memory';
import { isIpcError } from '../../lib/api/errors';
import { bumpMemoryRevision } from './memoryRevision';
import {
  DistillPreviewDisclosure,
  distillApplyFailureLabel,
  type DistillState,
} from './MemoryDistill';
import { MemoryTopicsDisclosure } from './MemoryTopics';
import { MemoryEntryRow as MemoryRow, MemoryLinksEditor } from './MemoryEntryRow';

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
  const [linkNotice, setLinkNotice] = useState<string | null>(null);
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
  // D70: append-only compaction history shown under the preview.
  const [distillLog, setDistillLog] = useState<MemoryDistillLogEntry[]>([]);
  const [linkEditor, setLinkEditor] = useState<{
    entry: MemoryEntry;
    topics: MemoryTopics | null;
    selected: string[];
    loading: boolean;
    saving: boolean;
    error: string | null;
  } | null>(null);
  const linkRequestGeneration = useRef(0);

  // D81 (review M1): the distill fetch/apply handlers are event-driven
  // (not effects), so they can't use the search effect's cleanup flag.
  // A mounted ref lets them skip their post-await state writes if the
  // panel unmounted while a request was in flight — matching the search
  // path's cancellation posture.
  const mountedRef = useRef(true);
  useEffect(
    () => () => {
      mountedRef.current = false;
    },
    [],
  );

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

  // D54/D70: fetch the distillation preview (and the audit log). Same
  // trust-gate behaviour as the index/search fetches — `NeedsApproval`
  // collapses the disclosure with a hint.
  //
  // Does NOT clear `distillNotice` (D75): `onApplyDistill` sets the
  // "Removed N" confirmation and then calls this to resync, so clearing
  // here would wipe the success message before the user sees it. The
  // explicit `onRefreshDistill` clears the notice for a manual rescan.
  const fetchDistill = useCallback(async () => {
    setDistillState({ kind: 'loading' });
    try {
      // The preview is essential; the audit log is secondary history.
      // Degrade a log-only failure (D75 review H2) to an empty log so a
      // corrupt `distill-log.jsonl` can't sink the duplicate preview and
      // the Compact action with it. A preview failure still surfaces.
      const [preview, log] = await Promise.all([
        getMemoryDistillPreview(),
        getMemoryDistillLog().catch(() => [] as MemoryDistillLogEntry[]),
      ]);
      if (!mountedRef.current) return;
      setDistillState({ kind: 'ready', preview });
      setDistillLog(log);
    } catch (err: unknown) {
      if (!mountedRef.current) return;
      const message =
        isIpcError(err) && err.kind === 'NeedsApproval'
          ? 'Trust the project to preview distillation.'
          : err instanceof Error
            ? err.message
            : String(err);
      setDistillState({ kind: 'error', message });
    }
  }, []);

  // Manual rescan: clear any lingering apply notice, then refetch.
  const onRefreshDistill = useCallback(() => {
    setDistillNotice(null);
    void fetchDistill();
  }, [fetchDistill]);

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
        if (!mountedRef.current) return;
        if (resp.ok) {
          // Surface any groups the backend refused because merging their
          // topic links would exceed the per-entry cap — never hidden.
          const conflicts = resp.conflictedGroupIds.length;
          const conflictNote =
            conflicts > 0
              ? ` ${conflicts} ${conflicts === 1 ? 'group was' : 'groups were'} left unchanged` +
                ' due to a topic-link conflict — prune links to compact.'
              : '';
          if (resp.removedEntryCount === 0) {
            setDistillNotice(
              conflicts > 0
                ? `Nothing compacted.${conflictNote}`
                : 'Nothing to compact — the store changed since the preview.',
            );
          } else {
            const n = resp.removedEntryCount;
            // D81: surface an unrecorded compaction rather than hiding it.
            const auditNote = resp.auditLogged ? '' : ' (not recorded in the audit log)';
            setDistillNotice(
              `Removed ${n} duplicate${n === 1 ? '' : 's'}.${auditNote}${conflictNote}`,
            );
          }
          bumpMemoryRevision();
          await refresh();
          await fetchDistill();
        } else {
          setDistillNotice(`${distillApplyFailureLabel(resp.reason)} — ${resp.message}`);
        }
      } catch (err: unknown) {
        if (!mountedRef.current) return;
        const message =
          isIpcError(err) && err.kind === 'NeedsApproval'
            ? 'Trust the project to compact memory.'
            : err instanceof Error
              ? err.message
              : String(err);
        setDistillNotice(message);
      } finally {
        if (mountedRef.current) setDistillBusy(false);
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

  // D80: save an in-place edit. Returns whether it succeeded so the row
  // can leave edit mode on success. Mirrors onForget's resync/error
  // handling; the backend re-redacts + re-caps the new text.
  const onUpdate = useCallback(
    async (entryId: string, text: string): Promise<boolean> => {
      setRememberError(null);
      try {
        const resp = await updateMemory(entryId, text);
        if (resp.ok) {
          await refresh();
          bumpMemoryRevision();
          return true;
        }
        setRememberError(`${updateFailureLabel(resp.reason)} — ${resp.message}`);
        return false;
      } catch (err) {
        setRememberError(err instanceof Error ? err.message : String(err));
        return false;
      }
    },
    [refresh],
  );

  const onOpenLinks = useCallback((entry: MemoryEntry) => {
    const generation = ++linkRequestGeneration.current;
    setLinkNotice(null);
    setLinkEditor({
      entry,
      topics: null,
      selected: [...entry.links],
      loading: true,
      saving: false,
      error: null,
    });
    void getMemoryTopics()
      .then((topics) => {
        if (!mountedRef.current || generation !== linkRequestGeneration.current) return;
        setLinkEditor((current) =>
          current?.entry.id === entry.id ? { ...current, topics, loading: false } : current,
        );
      })
      .catch((err: unknown) => {
        if (!mountedRef.current || generation !== linkRequestGeneration.current) return;
        const message = err instanceof Error ? err.message : String(err);
        setLinkEditor((current) =>
          current?.entry.id === entry.id
            ? { ...current, loading: false, error: message }
            : current,
        );
      });
  }, []);

  const onCancelLinks = useCallback(() => {
    linkRequestGeneration.current += 1;
    setLinkEditor(null);
  }, []);

  const onSaveLinks = useCallback(async () => {
    if (linkEditor === null || linkEditor.saving || linkEditor.loading) return;
    const generation = ++linkRequestGeneration.current;
    const entryId = linkEditor.entry.id;
    const links = [...linkEditor.selected].sort();
    setLinkEditor((current) => (current ? { ...current, saving: true, error: null } : current));
    try {
      const response = await setMemoryLinks(entryId, links);
      if (!mountedRef.current) return;
      if (!response.ok) {
        if (generation !== linkRequestGeneration.current) return;
        setLinkEditor((current) =>
          current?.entry.id === entryId
            ? { ...current, saving: false, error: response.message }
            : current,
        );
        return;
      }
      setState((current) =>
        current.kind === 'ready'
          ? {
              ...current,
              index: {
                ...current.index,
                entries: current.index.entries.map((entry) =>
                  entry.id === entryId ? response.entry : entry,
                ),
              },
            }
          : current,
      );
      setSearchState((current) =>
        current.kind === 'results'
          ? {
              ...current,
              hits: current.hits.map((hit) =>
                hit.entry.id === entryId ? { ...hit, entry: response.entry } : hit,
              ),
            }
          : current,
      );
      bumpMemoryRevision();
      if (generation === linkRequestGeneration.current) {
        setLinkNotice('Memory links saved.');
        setLinkEditor(null);
      }
    } catch (err: unknown) {
      if (!mountedRef.current || generation !== linkRequestGeneration.current) return;
      const message = err instanceof Error ? err.message : String(err);
      setLinkEditor((current) =>
        current?.entry.id === entryId ? { ...current, saving: false, error: message } : current,
      );
    }
  }, [linkEditor]);

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
      {linkNotice !== null && (
        <p className="plume-memory-hint" role="status">
          {linkNotice}
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
            onUpdate={onUpdate}
            onOpenLinks={onOpenLinks}
            activeLinkEntryId={linkEditor?.entry.id ?? null}
          />
          {searchState.kind === 'idle' && (
            <ul className="plume-memory-list" role="list">
              {entries.map((entry) => (
                <MemoryRow
                  key={entry.id}
                  entry={entry}
                  busy={busy}
                  onForget={() => void onForget(entry.id)}
                  onUpdate={(text) => onUpdate(entry.id, text)}
                  onOpenLinks={() => onOpenLinks(entry)}
                  linksExpanded={linkEditor?.entry.id === entry.id}
                />
              ))}
            </ul>
          )}
          {linkEditor !== null && (
            <MemoryLinksEditor
              state={linkEditor}
              onSelectionChange={(selected) =>
                setLinkEditor((current) => (current ? { ...current, selected, error: null } : current))
              }
              onCancel={onCancelLinks}
              onSave={() => void onSaveLinks()}
            />
          )}
          <DistillPreviewDisclosure
            expanded={distillExpanded}
            state={distillState}
            log={distillLog}
            applyBusy={distillBusy}
            notice={distillNotice}
            onToggle={onToggleDistill}
            onRefresh={onRefreshDistill}
            onApply={(groupIds) => void onApplyDistill(groupIds)}
          />
        </>
      )}
      {/* D71: curated topic files — independent of the entries list, so
          shown in every ready state. */}
      <MemoryTopicsDisclosure />
    </section>
  );
}


function MemorySearchResults({
  state,
  busy,
  onForget,
  onUpdate,
  onOpenLinks,
  activeLinkEntryId,
}: {
  state: SearchState;
  busy: boolean;
  onForget: (entryId: string) => void;
  onUpdate: (entryId: string, text: string) => Promise<boolean>;
  onOpenLinks: (entry: MemoryEntry) => void;
  activeLinkEntryId: string | null;
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
            onUpdate={(text) => onUpdate(hit.entry.id, text)}
            onOpenLinks={() => onOpenLinks(hit.entry)}
            linksExpanded={activeLinkEntryId === hit.entry.id}
          />
        ))}
      </ul>
    </>
  );
}

function updateFailureLabel(reason: MemoryUpdateFailure): string {
  switch (reason) {
    case 'badId':
      return 'Invalid entry id';
    case 'notFound':
      return 'Entry not found';
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
