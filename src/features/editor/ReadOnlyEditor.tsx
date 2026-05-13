// Read-only CodeMirror 6 editor pane.
//
// Slice C is display-only; nothing here writes back. The editor is
// created once per mount and content changes dispatch into the
// existing view, so swapping files doesn't tear down DOM nodes.
//
// D10 layered a selection-tracking callback on top. The editor
// observes its own selection via an `updateListener` and reports
// the 1-based inclusive line range when the user has a non-empty
// text selection (or `null` when the selection collapses back to a
// cursor). The component never reads or transmits the selected
// text — only the line numbers cross the prop boundary.

import { useEffect, useRef } from 'react';
import { defaultKeymap } from '@codemirror/commands';
import { EditorState } from '@codemirror/state';
import { EditorView, keymap, lineNumbers } from '@codemirror/view';

/// 1-based inclusive line range. Mirrors the backend's `LineRange`
/// shape. Exported so callers (the chat panel's Attach control)
/// can describe it without re-defining the contract.
export type EditorLineRange = {
  startLine: number;
  endLine: number;
};

type Props = {
  content: string;
  /// Fires when the editor's text selection changes.
  /// `null` means there is no non-empty selection — either the
  /// cursor is a point, or the document was just replaced.
  /// Receivers should treat consecutive `null` reports as
  /// idempotent.
  onSelectionChange?: (range: EditorLineRange | null) => void;
};

export function ReadOnlyEditor({ content, onSelectionChange }: Props) {
  const containerRef = useRef<HTMLDivElement | null>(null);
  const viewRef = useRef<EditorView | null>(null);
  // The listener closure captures `onSelectionChange` once at mount
  // time; if the caller hands us a new function on the next render
  // we read it through the ref so we don't have to recreate the
  // EditorView. (Re-creating the view on every parent re-render
  // would tear down the gutter, scroll position, etc.)
  const callbackRef = useRef(onSelectionChange);
  callbackRef.current = onSelectionChange;

  // Create once. The view's doc is mutated below when `content` changes.
  useEffect(() => {
    const host = containerRef.current;
    if (!host) return;
    const view = new EditorView({
      state: EditorState.create({
        doc: content,
        extensions: [
          EditorState.readOnly.of(true),
          EditorView.editable.of(false),
          lineNumbers(),
          keymap.of(defaultKeymap),
          EditorView.updateListener.of((update) => {
            // Only fire when the selection actually changed; a
            // pure scroll or focus change isn't interesting.
            if (!update.selectionSet) return;
            const fn = callbackRef.current;
            if (!fn) return;
            fn(rangeFromState(update.state));
          }),
          EditorView.theme({
            '&': { height: '100%', fontSize: '13px' },
            '.cm-scroller': {
              fontFamily: "'JetBrains Mono', 'SF Mono', Menlo, monospace",
              overflow: 'auto',
            },
            '.cm-gutters': {
              backgroundColor: 'transparent',
              borderRight: '1px solid var(--ink-soft)',
              color: 'var(--pencil)',
            },
          }),
        ],
      }),
      parent: host,
    });
    viewRef.current = view;
    return () => {
      view.destroy();
      viewRef.current = null;
    };
    // Intentionally empty: see content-update effect below.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    const view = viewRef.current;
    if (!view) return;
    view.dispatch({
      changes: { from: 0, to: view.state.doc.length, insert: content },
    });
    // Replacing the document collapses the selection — fire a
    // `null` report so callers reset their "what is selected"
    // state instead of holding a stale range that pointed at the
    // previous file.
    callbackRef.current?.(null);
  }, [content]);

  return <div ref={containerRef} className="plume-editor" />;
}

/// Pull the main selection out of an `EditorState` and map it to
/// 1-based inclusive line numbers. Returns `null` for an empty
/// (cursor-only) selection.
///
/// CodeMirror selection ranges are half-open `[from, to)`. When the
/// user selects all of line N and the trailing newline, `to` lands
/// at column 0 of line N+1; treating that line as part of the
/// selection would surprise the user. Anchoring `endLine` on
/// `to - 1` makes the boundary intuitive: "the last character the
/// user actually touched lives on this line".
function rangeFromState(state: EditorState): EditorLineRange | null {
  const sel = state.selection.main;
  if (sel.empty) return null;
  const startLine = state.doc.lineAt(sel.from).number;
  // `to` is always > 0 here because the range is non-empty.
  const endLine = state.doc.lineAt(sel.to - 1).number;
  return { startLine, endLine };
}
