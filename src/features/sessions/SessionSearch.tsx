// D66: compact search overlay over the persisted chats — a small
// palette above the workspace, reusing the settings-modal backdrop
// system. Scope separation is preserved end to end: one IPC call per
// scope, one result section per scope, never a mixed query.
//
// The input is debounced; a stale response (superseded query or a
// closed overlay) is dropped by sequence check, never rendered.

import {
  useEffect,
  useMemo,
  useRef,
  useState,
  type KeyboardEvent as ReactKeyboardEvent,
} from 'react';

import {
  searchSessions,
  SEARCH_SNIPPET_END,
  SEARCH_SNIPPET_START,
  type SessionScope,
  type SessionSearchHit,
} from '../../lib/api/sessions';
import { ipcErrorMessage, isIpcError } from '../../lib/api/errors';
import { relativeTime } from './SessionRow';

/** Debounce between the last keystroke and the IPC calls. */
export const SEARCH_DEBOUNCE_MS = 150;

/** Mirror of the backend's "no searchable characters" rule: a query
 * without a single alphanumeric scalar would only earn a BadArgument,
 * so the overlay never sends it. */
export function hasSearchableText(query: string): boolean {
  return /\p{L}|\p{N}/u.test(query);
}

/** Split a snippet on the private-use highlight markers into plain and
 * highlighted runs. Pure, so the marker contract stays unit-tested. */
export function snippetParts(
  snippet: string,
): Array<{ text: string; highlighted: boolean }> {
  const parts: Array<{ text: string; highlighted: boolean }> = [];
  for (const [i, chunk] of snippet.split(SEARCH_SNIPPET_START).entries()) {
    if (i === 0) {
      if (chunk !== '') parts.push({ text: chunk, highlighted: false });
      continue;
    }
    const end = chunk.indexOf(SEARCH_SNIPPET_END);
    if (end === -1) {
      // Unpaired marker — render the run unhighlighted rather than
      // losing text.
      if (chunk !== '') parts.push({ text: chunk, highlighted: false });
      continue;
    }
    const hit = chunk.slice(0, end);
    if (hit !== '') parts.push({ text: hit, highlighted: true });
    const rest = chunk.slice(end + SEARCH_SNIPPET_END.length);
    if (rest !== '') parts.push({ text: rest, highlighted: false });
  }
  return parts;
}

/** Cmd+K opens the overlay from anywhere in the shell. Plain Cmd+K
 * only — Cmd+Shift+[ / ] (panel toggles) and other chords are
 * untouched. */
export function useSearchShortcut(open: () => void): void {
  useEffect(() => {
    const onKey = (event: globalThis.KeyboardEvent) => {
      if (!event.metaKey || event.shiftKey || event.altKey || event.ctrlKey) return;
      if (event.key.toLowerCase() !== 'k') return;
      event.preventDefault();
      open();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [open]);
}

const SCOPE_LABEL: Record<SessionScope, string> = {
  local: 'Chats',
  project: 'Project chats',
};

type ScopeHits = { scope: SessionScope; hits: SessionSearchHit[] };

export function SessionSearchOverlay({
  projectAvailable,
  notice,
  onSelect,
  onClose,
}: {
  /** Adds the project section (its own database, its own IPC call). */
  projectAvailable: boolean;
  /** Shell notice to surface when a selection is refused (streaming
   * switch block, load failure). */
  notice: string | null;
  /** Resolves `true` when the chat was opened; `false` keeps the
   * overlay open with the notice. */
  onSelect: (scope: SessionScope, sessionId: string) => Promise<boolean>;
  onClose: () => void;
}) {
  const [query, setQuery] = useState('');
  const [sections, setSections] = useState<ScopeHits[]>([]);
  const [searching, setSearching] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [selectFailed, setSelectFailed] = useState(false);
  const [activeIndex, setActiveIndex] = useState(0);
  const seqRef = useRef(0);
  const inputRef = useRef<HTMLInputElement | null>(null);

  const scopes = useMemo<SessionScope[]>(
    () => (projectAvailable ? ['local', 'project'] : ['local']),
    [projectAvailable],
  );

  useEffect(() => {
    inputRef.current?.focus();
  }, []);

  useEffect(() => {
    const trimmed = query.trim();
    const seq = ++seqRef.current;
    if (trimmed === '' || !hasSearchableText(trimmed)) {
      setSections([]);
      setSearching(false);
      setError(null);
      setActiveIndex(0);
      return;
    }
    setSearching(true);
    const timer = setTimeout(() => {
      void (async () => {
        try {
          // One call per scope — separate databases, never one query.
          const next: ScopeHits[] = [];
          for (const scope of scopes) {
            const { hits } = await searchSessions({ scope, query: trimmed });
            next.push({ scope, hits });
          }
          if (seqRef.current !== seq) return;
          setSections(next);
          setError(null);
          setActiveIndex(0);
        } catch (err) {
          if (seqRef.current !== seq) return;
          const message = formatError(err);
          console.error('sessions.search failed:', message);
          setSections([]);
          setError(message);
        } finally {
          if (seqRef.current === seq) setSearching(false);
        }
      })();
    }, SEARCH_DEBOUNCE_MS);
    return () => clearTimeout(timer);
  }, [query, scopes]);

  const flat = useMemo(
    () =>
      sections.flatMap(({ scope, hits }) =>
        hits.map((hit) => ({ scope, hit })),
      ),
    [sections],
  );
  const hasResults = flat.length > 0;
  const trimmedQuery = query.trim();

  const select = (scope: SessionScope, sessionId: string) => {
    void onSelect(scope, sessionId).then((ok) => {
      if (ok) {
        onClose();
      } else {
        setSelectFailed(true);
      }
    });
  };

  const onKeyDown = (event: ReactKeyboardEvent) => {
    if (event.key === 'Escape') {
      event.preventDefault();
      onClose();
      return;
    }
    if (!hasResults) return;
    if (event.key === 'ArrowDown') {
      event.preventDefault();
      setActiveIndex((i) => (i + 1) % flat.length);
    } else if (event.key === 'ArrowUp') {
      event.preventDefault();
      setActiveIndex((i) => (i - 1 + flat.length) % flat.length);
    } else if (event.key === 'Enter') {
      event.preventDefault();
      const active = flat[Math.min(activeIndex, flat.length - 1)];
      if (active !== undefined) select(active.scope, active.hit.id);
    }
  };

  let flatIndex = -1;
  return (
    <div
      className="plume-project-settings-backdrop"
      role="presentation"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) onClose();
      }}
    >
      <section
        className="plume-project-settings-window plume-session-search"
        role="dialog"
        aria-modal="true"
        aria-label="Search chats"
        onKeyDown={onKeyDown}
      >
        <input
          ref={inputRef}
          className="plume-session-search-input"
          type="text"
          role="combobox"
          aria-expanded={hasResults}
          aria-controls="plume-session-search-results"
          aria-activedescendant={
            hasResults ? `plume-search-hit-${Math.min(activeIndex, flat.length - 1)}` : undefined
          }
          aria-label="Search chats"
          placeholder="Search chat titles and transcripts…"
          maxLength={200}
          value={query}
          onChange={(event) => {
            setQuery(event.target.value);
            setSelectFailed(false);
          }}
        />
        {selectFailed ? (
          <p className="plume-session-dialog-error" role="alert">
            {notice ?? 'Could not open that chat.'}
          </p>
        ) : null}
        {error !== null ? (
          <p className="plume-session-dialog-error" role="alert">
            {error}
          </p>
        ) : null}
        <div id="plume-session-search-results" role="listbox" aria-label="Matching chats">
          {sections.map(({ scope, hits }) =>
            hits.length === 0 ? null : (
              <div key={scope} className="plume-session-search-section">
                <p className="plume-session-search-scope">{SCOPE_LABEL[scope]}</p>
                {hits.map((hit) => {
                  flatIndex += 1;
                  const index = flatIndex;
                  return (
                    <SearchHitRow
                      key={`${scope}:${hit.id}`}
                      id={`plume-search-hit-${index}`}
                      hit={hit}
                      active={index === activeIndex}
                      onHover={() => setActiveIndex(index)}
                      onPick={() => select(scope, hit.id)}
                    />
                  );
                })}
              </div>
            ),
          )}
          {trimmedQuery !== '' && !searching && !hasResults && error === null ? (
            <p className="plume-session-search-empty" role="status">
              No chats match “{trimmedQuery}”.
            </p>
          ) : null}
          {trimmedQuery === '' ? (
            <p className="plume-session-search-empty" role="status">
              Type to search chat titles and transcripts.
            </p>
          ) : null}
        </div>
      </section>
    </div>
  );
}

function SearchHitRow({
  id,
  hit,
  active,
  onHover,
  onPick,
}: {
  id: string;
  hit: SessionSearchHit;
  active: boolean;
  onHover: () => void;
  onPick: () => void;
}) {
  return (
    <div
      id={id}
      role="option"
      aria-selected={active}
      className={`plume-session-search-hit${active ? ' plume-session-search-hit-active' : ''}`}
      onMouseEnter={onHover}
      onMouseDown={(event) => {
        // mousedown, not click: the input keeps focus and the overlay
        // never closes from a backdrop blur mid-selection.
        event.preventDefault();
        onPick();
      }}
    >
      <span className="plume-session-search-title">
        {hit.title}
        {hit.archivedAtMs !== null ? (
          <span className="plume-session-search-archived"> archived</span>
        ) : null}
      </span>
      <span className="plume-session-search-meta">{relativeTime(hit.updatedAtMs)}</span>
      {hit.snippet !== null ? (
        <span className="plume-session-search-snippet">
          {snippetParts(hit.snippet).map((part, i) =>
            part.highlighted ? (
              // eslint-disable-next-line react/no-array-index-key
              <mark key={i}>{part.text}</mark>
            ) : (
              // eslint-disable-next-line react/no-array-index-key
              <span key={i}>{part.text}</span>
            ),
          )}
        </span>
      ) : null}
    </div>
  );
}

function formatError(err: unknown): string {
  if (isIpcError(err)) return ipcErrorMessage(err);
  if (err instanceof Error) return err.message;
  return 'Search failed.';
}
