// Read-only CodeMirror 6 editor pane.
//
// Slice C is display-only; nothing here writes back. The editor is
// created once per mount and content changes dispatch into the
// existing view, so swapping files doesn't tear down DOM nodes.

import { useEffect, useRef } from 'react';
import { defaultKeymap } from '@codemirror/commands';
import { EditorState } from '@codemirror/state';
import { EditorView, keymap, lineNumbers } from '@codemirror/view';

type Props = {
  content: string;
};

export function ReadOnlyEditor({ content }: Props) {
  const containerRef = useRef<HTMLDivElement | null>(null);
  const viewRef = useRef<EditorView | null>(null);

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
  }, [content]);

  return <div ref={containerRef} className="plume-editor" />;
}
