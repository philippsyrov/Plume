// File tree state hook + split renderers.
//
// D1.5 splits the previous monolithic FileBrowser into two visual
// halves so they can live in different zones of the workspace shell:
//   - `FileNavigator` is the left-zone listing + breadcrumb.
//   - `FileInspector` is the right-zone selection viewer (CodeMirror,
//     binary placeholder, blocked-file message, etc.).
// Both read from the same `useFileNavigator(projectRoot)` hook, so a
// click in the navigator is reflected in the inspector without prop
// drilling between zones.
//
// Slice C is still "I opened a repo and can read code"; recursive
// indexing and writes live in later slices. Splitting the visual
// halves does not change the IPC surface either component talks to.

import {
  useCallback,
  useEffect,
  useMemo,
  useState,
  type KeyboardEvent,
  type ReactNode,
} from 'react';

import { listDir, readFile, type FileContent, type FileEntry } from '../../lib/api/fs';
import { ipcErrorMessage, isIpcError } from '../../lib/api/errors';
import { formatBytesOneDecimal as formatBytes } from '../../lib/format';
import { ReadOnlyEditor, type EditorLineRange } from '../editor/ReadOnlyEditor';
import type { ContextSourceRef } from '../../lib/api/chat';
import { ContextDragAction } from '../chat/ContextDragAction';

type ListingState =
  | { kind: 'loading' }
  | { kind: 'ready'; entries: FileEntry[] }
  | { kind: 'error'; message: string };

type QuickOpenFile = {
  name: string;
  path: string;
  dir: string;
  size: number | null;
  modifiedMs: number;
};

type QuickOpenState =
  | { kind: 'loading' }
  | { kind: 'ready'; files: QuickOpenFile[]; truncated: boolean }
  | { kind: 'error'; message: string };

/// Discriminated state for the file the user is currently inspecting.
/// Exported so callers (D8 chat attach control) can read selection
/// state without re-implementing the navigator's loading + race
/// guard logic.
export type SelectionState =
  | { kind: 'empty' }
  | { kind: 'loading'; path: string }
  | { kind: 'ready'; path: string; content: FileContent }
  | { kind: 'error'; path: string; message: string };

/// State the navigator and inspector share. The shape is intentionally
/// small: a relative directory cursor, the current listing, the current
/// file selection, and the actions the navigator needs to drive both.
///
/// `currentLineRange` is the D10 piece: it tracks the user's text
/// selection inside the inspector's read-only editor. `null` means
/// either no file is open or the user has only a point cursor.
/// Switching files resets it to `null`. The chat panel reads this
/// directly to enable / shape its "Attach selection" control.
export type FileNavigatorState = {
  projectRoot: string;
  relDir: string;
  setRelDir: (relDir: string) => void;
  listing: ListingState;
  selection: SelectionState;
  onSelectEntry: (entry: FileEntry) => void;
  quickOpen: {
    query: string;
    setQuery: (query: string) => void;
    state: QuickOpenState;
    openPath: (path: string) => void;
    refresh: () => void;
  };
  currentLineRange: EditorLineRange | null;
  setCurrentLineRange: (range: EditorLineRange | null) => void;
};

/// Hook that owns directory + selection state. Identical IPC behavior
/// to the pre-D1.5 FileBrowser; only the rendering moves.
export function useFileNavigator(projectRoot: string): FileNavigatorState {
  // Path relative to the project root. Empty string is root itself.
  const [relDir, setRelDir] = useState('');
  const [listing, setListing] = useState<ListingState>({ kind: 'loading' });
  const [selection, setSelection] = useState<SelectionState>({ kind: 'empty' });
  const [quickOpen, setQuickOpen] = useState<QuickOpenState>({ kind: 'loading' });
  const [quickOpenQuery, setQuickOpenQuery] = useState('');
  const [quickOpenRevision, setQuickOpenRevision] = useState(0);
  // D10: live text-selection range inside the inspector's editor.
  // Reset whenever the open file changes (or when the user clears
  // their selection back to a point cursor).
  const [currentLineRange, setCurrentLineRange] = useState<EditorLineRange | null>(null);

  // Reset everything when the project root changes (open a different
  // project, switch trust state, etc.).
  useEffect(() => {
    setRelDir('');
    setSelection({ kind: 'empty' });
    setCurrentLineRange(null);
    setQuickOpenQuery('');
  }, [projectRoot]);

  useEffect(() => {
    let cancelled = false;
    setQuickOpen({ kind: 'loading' });
    scanProjectFiles(() => cancelled)
      .then((next) => {
        if (!cancelled) setQuickOpen({ kind: 'ready', ...next });
      })
      .catch((err) => {
        if (!cancelled) setQuickOpen({ kind: 'error', message: formatError(err) });
      });
    return () => {
      cancelled = true;
    };
  }, [projectRoot, quickOpenRevision]);

  // Fetch listing on directory change.
  useEffect(() => {
    let cancelled = false;
    setListing({ kind: 'loading' });
    listDir(relDir)
      .then((entries) => {
        if (cancelled) return;
        setListing({ kind: 'ready', entries });
      })
      .catch((err) => {
        if (cancelled) return;
        setListing({ kind: 'error', message: formatError(err) });
      });
    return () => {
      cancelled = true;
    };
  }, [relDir]);

  const onSelectEntry = useCallback(
    (entry: FileEntry) => {
      if (entry.kind === 'dir') {
        setRelDir(joinRel(relDir, entry.name));
        setSelection({ kind: 'empty' });
        setCurrentLineRange(null);
        return;
      }
      if (entry.kind === 'symlink') {
        setSelection({
          kind: 'error',
          path: entry.path,
          message:
            'Symlinks are not followed for display reads. Open the link target directly if it lives inside the project.',
        });
        setCurrentLineRange(null);
        return;
      }
      const targetRel = joinRel(relDir, entry.name);
      setSelection({ kind: 'loading', path: targetRel });
      // Picking a new file invalidates whatever selection the user
      // had inside the previous file's editor. The editor will fire
      // its own `null` selection report once the new content lands,
      // but resetting eagerly avoids a flicker of "Attach selection"
      // pointing at the wrong file.
      setCurrentLineRange(null);
      // Race guard: if the user clicks file B while A's read is still
      // in flight, A's resolve must not overwrite B's loading state.
      // The functional updater keeps state only when the in-flight
      // load is still the active one.
      readFile(targetRel)
        .then((content) =>
          setSelection((prev) =>
            prev.kind === 'loading' && prev.path === targetRel
              ? { kind: 'ready', path: targetRel, content }
              : prev,
          ),
        )
        .catch((err) =>
          setSelection((prev) =>
            prev.kind === 'loading' && prev.path === targetRel
              ? { kind: 'error', path: targetRel, message: formatError(err) }
              : prev,
          ),
        );
    },
    [relDir],
  );

  const openPath = useCallback((path: string) => {
    const targetRel = normalizeRel(path);
    if (!targetRel) return;
    setRelDir(parentRel(targetRel));
    setSelection({ kind: 'loading', path: targetRel });
    setCurrentLineRange(null);
    readFile(targetRel)
      .then((content) =>
        setSelection((prev) =>
          prev.kind === 'loading' && prev.path === targetRel
            ? { kind: 'ready', path: targetRel, content }
            : prev,
        ),
      )
      .catch((err) =>
        setSelection((prev) =>
          prev.kind === 'loading' && prev.path === targetRel
            ? { kind: 'error', path: targetRel, message: formatError(err) }
            : prev,
        ),
      );
  }, []);

  return {
    projectRoot,
    relDir,
    setRelDir,
    listing,
    selection,
    onSelectEntry,
    quickOpen: {
      query: quickOpenQuery,
      setQuery: setQuickOpenQuery,
      state: quickOpen,
      openPath,
      refresh: () => setQuickOpenRevision((n) => n + 1),
    },
    currentLineRange,
    setCurrentLineRange,
  };
}

/// Left-zone view: breadcrumb on top, listing below. Owns no state.
export function FileNavigator({ state }: { state: FileNavigatorState }) {
  const breadcrumb = useMemo(() => buildBreadcrumb(state.relDir), [state.relDir]);
  return (
    <section className="plume-navigator ink-panel" aria-label="Project files">
      <QuickOpen state={state} />
      <Breadcrumb
        segments={breadcrumb}
        rootName={lastSegment(state.projectRoot)}
        onNavigate={state.setRelDir}
      />
      <ListingPane
        state={state.listing}
        onSelect={state.onSelectEntry}
        selection={state.selection}
        relDir={state.relDir}
      />
    </section>
  );
}

/// Right-zone view: header + selection viewer (editor / placeholder /
/// error). Owns no state.
export function FileInspector({
  state,
  contextSource,
  onUseInChat,
  onContextDragActiveChange,
}: {
  state: FileNavigatorState;
  contextSource?: ContextSourceRef | null;
  onUseInChat?: (source: ContextSourceRef) => void | Promise<unknown>;
  onContextDragActiveChange?: (active: boolean) => void;
}) {
  const actionLabel =
    contextSource?.kind === 'projectFile' && contextSource.startLine !== undefined
      ? 'Use selection in chat'
      : 'Use file in chat';
  return (
    <section className="plume-inspector ink-panel" aria-label="File inspector">
      <InspectorHeader selection={state.selection}>
        {contextSource && onUseInChat && onContextDragActiveChange ? (
          <ContextDragAction
            source={contextSource}
            onActivate={onUseInChat}
            onDragActiveChange={onContextDragActiveChange}
          >
            {actionLabel}
          </ContextDragAction>
        ) : contextSource && onUseInChat ? (
          <button type="button" onClick={() => void onUseInChat(contextSource)}>
            {actionLabel}
          </button>
        ) : null}
      </InspectorHeader>
      <div className="plume-inspector-body">
        <SelectionPane
          selection={state.selection}
          onSelectionChange={state.setCurrentLineRange}
        />
      </div>
    </section>
  );
}

type BreadcrumbProps = {
  segments: string[];
  rootName: string;
  onNavigate: (relDir: string) => void;
};

function Breadcrumb({ segments, rootName, onNavigate }: BreadcrumbProps) {
  return (
    <nav className="plume-breadcrumb" aria-label="Project path">
      <button
        type="button"
        className="plume-breadcrumb-segment"
        onClick={() => onNavigate('')}
        title={rootName}
      >
        {rootName}
      </button>
      {segments.map((seg, i) => {
        const target = segments.slice(0, i + 1).join('/');
        return (
          <span key={target} className="plume-breadcrumb-step">
            <span className="plume-breadcrumb-sep" aria-hidden>
              /
            </span>
            <button
              type="button"
              className="plume-breadcrumb-segment"
              onClick={() => onNavigate(target)}
            >
              {seg}
            </button>
          </span>
        );
      })}
    </nav>
  );
}

type ListingPaneProps = {
  state: ListingState;
  onSelect: (entry: FileEntry) => void;
  selection: SelectionState;
  relDir: string;
};

function ListingPane({ state, onSelect, selection, relDir }: ListingPaneProps) {
  if (state.kind === 'loading') {
    return <p className="plume-listing-status">Loading…</p>;
  }
  if (state.kind === 'error') {
    return (
      <p className="plume-listing-status plume-listing-error" role="alert">
        {state.message}
      </p>
    );
  }
  if (state.entries.length === 0) {
    return <p className="plume-listing-status">Empty directory.</p>;
  }
  const selectedPath =
    selection.kind === 'ready' || selection.kind === 'loading' || selection.kind === 'error'
      ? selection.path
      : null;
  return (
    <ul className="plume-listing">
      {state.entries.map((entry) => {
        // Compare relative paths exactly. `endsWith` looked tempting
        // since FileEntry.path is canonical absolute and selectedPath
        // is relative — but that produces false positives whenever a
        // shorter filename happens to be a suffix of a longer one
        // (selecting `a.txt` would also light up `ba.txt`).
        const entryRel = joinRel(relDir, entry.name);
        const isSelected = entryRel === selectedPath;
        return (
          <li key={entry.path}>
            <button
              type="button"
              className={`plume-entry plume-entry-${entry.kind}${
                isSelected ? ' plume-entry-selected' : ''
              }`}
              onClick={() => onSelect(entry)}
              title={entry.path}
            >
              <span className="plume-entry-icon" aria-hidden>
                {entry.kind === 'dir' ? '▸' : entry.kind === 'symlink' ? '↪' : '·'}
              </span>
              <span className="plume-entry-name">{entry.name}</span>
              {entry.kind === 'file' && entry.size !== null ? (
                <span className="plume-entry-size">{formatBytes(entry.size)}</span>
              ) : null}
            </button>
          </li>
        );
      })}
    </ul>
  );
}

function InspectorHeader({
  selection,
  children,
}: {
  selection: SelectionState;
  children?: ReactNode;
}) {
  // The header is the only thing in the inspector that's stable across
  // selection states; it gives the right zone a consistent label so the
  // 3-zone shell doesn't look hollow when nothing is open.
  let detail: string;
  switch (selection.kind) {
    case 'empty':
      detail = 'no file selected';
      break;
    case 'loading':
      detail = `reading ${selection.path}…`;
      break;
    case 'ready':
      detail = selection.path;
      break;
    case 'error':
      detail = selection.path;
      break;
  }
  return (
    <header className="plume-inspector-header">
      <h3>Preview</h3>
      <span className="plume-inspector-detail" title={detail}>
        {detail}
      </span>
      {children}
    </header>
  );
}

type SelectionPaneProps = {
  selection: SelectionState;
  /// D10: forwarded into the editor so the navigator hook can
  /// track which lines (if any) the user has selected.
  onSelectionChange: (range: EditorLineRange | null) => void;
};

function SelectionPane({ selection, onSelectionChange }: SelectionPaneProps) {
  if (selection.kind === 'empty') {
    return (
      <div className="plume-selection-empty">
        <p>Select a file to preview it here.</p>
      </div>
    );
  }
  if (selection.kind === 'loading') {
    return (
      <div className="plume-selection-empty">
        <p>Reading {selection.path}…</p>
      </div>
    );
  }
  if (selection.kind === 'error') {
    return (
      <div className="plume-selection-empty plume-selection-error" role="alert">
        <p>{selection.message}</p>
      </div>
    );
  }
  if (selection.content.encoding === 'binary') {
    return (
      <div className="plume-selection-empty">
        <p>
          Binary file — {formatBytes(selection.content.bytes)}. Plume's display reader
          does not render bytes; open it in your OS for the right viewer.
        </p>
      </div>
    );
  }
  return (
    <ReadOnlyEditor
      content={selection.content.content}
      onSelectionChange={onSelectionChange}
    />
  );
}

function QuickOpen({ state }: { state: FileNavigatorState }) {
  const { query, setQuery, openPath, refresh } = state.quickOpen;
  const matches = useMemo(
    () => quickOpenMatches(state.quickOpen.state, query),
    [state.quickOpen.state, query],
  );
  const [activeIndex, setActiveIndex] = useState(0);

  useEffect(() => {
    setActiveIndex(0);
  }, [query, state.quickOpen.state]);

  const onKeyDown = (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.key === 'ArrowDown' && matches.length > 0) {
      event.preventDefault();
      setActiveIndex((index) => Math.min(index + 1, matches.length - 1));
    } else if (event.key === 'ArrowUp' && matches.length > 0) {
      event.preventDefault();
      setActiveIndex((index) => Math.max(index - 1, 0));
    } else if (event.key === 'Enter' && matches[activeIndex]) {
      event.preventDefault();
      openPath(matches[activeIndex].path);
    } else if (event.key === 'Escape' && query) {
      event.preventDefault();
      setQuery('');
    }
  };

  return (
    <section className="plume-quick-open" aria-label="Open file">
      <div className="plume-quick-open-field">
        <span className="plume-quick-open-icon" aria-hidden />
        <input
          type="search"
          value={query}
          placeholder="Open file"
          aria-label="Open file"
          spellCheck={false}
          autoCapitalize="off"
          autoCorrect="off"
          onChange={(event) => setQuery(event.target.value)}
          onKeyDown={onKeyDown}
        />
        {query ? (
          <button
            type="button"
            className="plume-quick-open-clear"
            onClick={() => setQuery('')}
            aria-label="Clear file search"
          >
            Clear
          </button>
        ) : null}
      </div>
      <QuickOpenResults
        state={state.quickOpen.state}
        query={query}
        matches={matches}
        activeIndex={activeIndex}
        onActivate={(index) => setActiveIndex(index)}
        onOpen={openPath}
        onRefresh={refresh}
      />
    </section>
  );
}

function QuickOpenResults({
  state,
  query,
  matches,
  activeIndex,
  onActivate,
  onOpen,
  onRefresh,
}: {
  state: QuickOpenState;
  query: string;
  matches: QuickOpenFile[];
  activeIndex: number;
  onActivate: (index: number) => void;
  onOpen: (path: string) => void;
  onRefresh: () => void;
}) {
  if (state.kind === 'loading') {
    return <p className="plume-quick-open-status">Indexing files…</p>;
  }
  if (state.kind === 'error') {
    return (
      <div className="plume-quick-open-status plume-quick-open-error" role="alert">
        <span>{state.message}</span>
        <button type="button" onClick={onRefresh}>
          Retry
        </button>
      </div>
    );
  }
  if (matches.length === 0) {
    return (
      <p className="plume-quick-open-status">
        {query.trim() ? 'No matching files.' : 'No files indexed.'}
      </p>
    );
  }
  return (
    <>
      <div className="plume-quick-open-meta">
        <span>{query.trim() ? 'Matches' : 'Recent'}</span>
        {state.truncated ? <span>partial index</span> : null}
      </div>
      <ul className="plume-quick-open-list" role="listbox" aria-label="File matches">
        {matches.map((file, index) => (
          <li key={file.path}>
            <button
              type="button"
              className={`plume-quick-open-row${
                index === activeIndex ? ' plume-quick-open-row-active' : ''
              }`}
              role="option"
              aria-selected={index === activeIndex}
              title={file.path}
              onMouseEnter={() => onActivate(index)}
              onFocus={() => onActivate(index)}
              onClick={() => onOpen(file.path)}
            >
              <span className="plume-quick-open-file">{file.name}</span>
              <span className="plume-quick-open-path">{file.dir || 'project root'}</span>
              {file.size !== null ? (
                <span className="plume-quick-open-size">{formatBytes(file.size)}</span>
              ) : null}
            </button>
          </li>
        ))}
      </ul>
    </>
  );
}

function joinRel(prefix: string, segment: string): string {
  if (!prefix) return segment;
  return `${prefix}/${segment}`;
}

function normalizeRel(path: string): string {
  return path.replace(/^\.?\//, '').replace(/\\/g, '/');
}

function parentRel(path: string): string {
  const normalized = normalizeRel(path);
  const index = normalized.lastIndexOf('/');
  return index === -1 ? '' : normalized.slice(0, index);
}

function buildBreadcrumb(relDir: string): string[] {
  if (!relDir) return [];
  return relDir.split('/').filter(Boolean);
}

function lastSegment(absolutePath: string): string {
  const trimmed = absolutePath.replace(/[/\\]+$/, '');
  const parts = trimmed.split(/[/\\]/);
  return parts[parts.length - 1] || absolutePath;
}

function formatError(err: unknown): string {
  if (isIpcError(err)) return ipcErrorMessage(err);
  if (err instanceof Error) return err.message;
  return 'Unknown error.';
}

const QUICK_OPEN_FILE_LIMIT = 10000;
const QUICK_OPEN_DIR_LIMIT = 2000;
const QUICK_OPEN_DEPTH_LIMIT = 8;
const QUICK_OPEN_RESULT_LIMIT = 7;
const QUICK_OPEN_SKIP_DIRS = new Set([
  '.cache',
  '.git',
  '.next',
  '.plume',
  '.pytest_cache',
  '.tauri',
  '.turbo',
  '.venv',
  'build',
  'coverage',
  'dist',
  'node_modules',
  'target',
  'venv',
  '__pycache__',
]);

async function scanProjectFiles(
  isCancelled: () => boolean,
): Promise<{ files: QuickOpenFile[]; truncated: boolean }> {
  const files: QuickOpenFile[] = [];
  let dirCount = 0;
  let truncated = false;
  const queue: Array<{ relDir: string; depth: number }> = [{ relDir: '', depth: 0 }];

  while (queue.length > 0 && !isCancelled() && !truncated) {
    const next = queue.shift();
    if (!next) break;
    const { relDir, depth } = next;
    if (depth > QUICK_OPEN_DEPTH_LIMIT) {
      truncated = true;
      break;
    }
    dirCount += 1;
    if (dirCount > QUICK_OPEN_DIR_LIMIT) {
      truncated = true;
      break;
    }

    const entries = await listDir(relDir);
    for (const entry of entries) {
      if (isCancelled() || truncated) break;
      const entryRel = joinRel(relDir, entry.name);
      if (entry.kind === 'dir') {
        if (!shouldSkipQuickOpenDir(entry.name)) {
          queue.push({ relDir: entryRel, depth: depth + 1 });
        }
      } else if (entry.kind === 'file') {
        files.push({
          name: entry.name,
          path: entryRel,
          dir: relDir,
          size: entry.size,
          modifiedMs: entry.modifiedMs,
        });
        if (files.length >= QUICK_OPEN_FILE_LIMIT) {
          truncated = true;
          break;
        }
      }
    }
  }

  files.sort((a, b) => b.modifiedMs - a.modifiedMs || a.path.localeCompare(b.path));
  return { files, truncated };
}

function shouldSkipQuickOpenDir(name: string): boolean {
  return name.startsWith('.') || QUICK_OPEN_SKIP_DIRS.has(name);
}

function quickOpenMatches(state: QuickOpenState, query: string): QuickOpenFile[] {
  if (state.kind !== 'ready') return [];
  const trimmed = query.trim().toLowerCase();
  if (!trimmed) return state.files.slice(0, QUICK_OPEN_RESULT_LIMIT);
  const tokens = trimmed.split(/\s+/).filter(Boolean);
  return state.files
    .filter((file) => {
      const haystack = `${file.name} ${file.path}`.toLowerCase();
      return tokens.every((token) => haystack.includes(token));
    })
    .sort((a, b) => scoreQuickOpenFile(a, trimmed) - scoreQuickOpenFile(b, trimmed))
    .slice(0, QUICK_OPEN_RESULT_LIMIT);
}

function scoreQuickOpenFile(file: QuickOpenFile, query: string): number {
  const name = file.name.toLowerCase();
  const path = file.path.toLowerCase();
  if (name === query) return 0;
  if (name.startsWith(query)) return 1;
  if (path.startsWith(query)) return 2;
  const nameIndex = name.indexOf(query);
  if (nameIndex >= 0) return 3 + nameIndex / 100;
  const pathIndex = path.indexOf(query);
  return 5 + Math.max(pathIndex, 0) / 100;
}
