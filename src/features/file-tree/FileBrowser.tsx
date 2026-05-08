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

import { useCallback, useEffect, useMemo, useState } from 'react';

import { listDir, readFile, type FileContent, type FileEntry } from '../../lib/api/fs';
import { ipcErrorMessage, isIpcError } from '../../lib/api/errors';
import { ReadOnlyEditor } from '../editor/ReadOnlyEditor';

type ListingState =
  | { kind: 'loading' }
  | { kind: 'ready'; entries: FileEntry[] }
  | { kind: 'error'; message: string };

type SelectionState =
  | { kind: 'empty' }
  | { kind: 'loading'; path: string }
  | { kind: 'ready'; path: string; content: FileContent }
  | { kind: 'error'; path: string; message: string };

/// State the navigator and inspector share. The shape is intentionally
/// small: a relative directory cursor, the current listing, the current
/// file selection, and the actions the navigator needs to drive both.
export type FileNavigatorState = {
  projectRoot: string;
  relDir: string;
  setRelDir: (relDir: string) => void;
  listing: ListingState;
  selection: SelectionState;
  onSelectEntry: (entry: FileEntry) => void;
};

/// Hook that owns directory + selection state. Identical IPC behavior
/// to the pre-D1.5 FileBrowser; only the rendering moves.
export function useFileNavigator(projectRoot: string): FileNavigatorState {
  // Path relative to the project root. Empty string is root itself.
  const [relDir, setRelDir] = useState('');
  const [listing, setListing] = useState<ListingState>({ kind: 'loading' });
  const [selection, setSelection] = useState<SelectionState>({ kind: 'empty' });

  // Reset everything when the project root changes (open a different
  // project, switch trust state, etc.).
  useEffect(() => {
    setRelDir('');
    setSelection({ kind: 'empty' });
  }, [projectRoot]);

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
        return;
      }
      if (entry.kind === 'symlink') {
        setSelection({
          kind: 'error',
          path: entry.path,
          message:
            'Symlinks are not followed for display reads. Open the link target directly if it lives inside the project.',
        });
        return;
      }
      const targetRel = joinRel(relDir, entry.name);
      setSelection({ kind: 'loading', path: targetRel });
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

  return { projectRoot, relDir, setRelDir, listing, selection, onSelectEntry };
}

/// Left-zone view: breadcrumb on top, listing below. Owns no state.
export function FileNavigator({ state }: { state: FileNavigatorState }) {
  const breadcrumb = useMemo(() => buildBreadcrumb(state.relDir), [state.relDir]);
  return (
    <section className="plume-navigator ink-panel" aria-label="Project files">
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
export function FileInspector({ state }: { state: FileNavigatorState }) {
  return (
    <section className="plume-inspector ink-panel" aria-label="File inspector">
      <InspectorHeader selection={state.selection} />
      <div className="plume-inspector-body">
        <SelectionPane selection={state.selection} />
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

function InspectorHeader({ selection }: { selection: SelectionState }) {
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
      <h3>Inspector</h3>
      <span className="plume-inspector-detail" title={detail}>
        {detail}
      </span>
    </header>
  );
}

function SelectionPane({ selection }: { selection: SelectionState }) {
  if (selection.kind === 'empty') {
    return (
      <div className="plume-selection-empty">
        <p>Select a file from the navigator to inspect it here.</p>
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
  return <ReadOnlyEditor content={selection.content.content} />;
}

function joinRel(prefix: string, segment: string): string {
  if (!prefix) return segment;
  return `${prefix}/${segment}`;
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

function formatBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  return `${(n / (1024 * 1024)).toFixed(1)} MB`;
}

function formatError(err: unknown): string {
  if (isIpcError(err)) return ipcErrorMessage(err);
  if (err instanceof Error) return err.message;
  return 'Unknown error.';
}
