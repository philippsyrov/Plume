// D101: the shared unified-diff body renderer — the colored `<pre>` that
// both the chat `DiffPreview` (D15) and the single-step agent panel use to
// show a proposed diff. Extracted from `DiffPreview` so the agent path can
// reuse the exact same line coloring without duplicating `classifyDiffLine`
// or the markup.
//
// It emits the historical `plume-chat-diff-*` class names (see
// `styles/layout/chat.css`): those predate the extraction and are the
// visual contract for "a rendered diff", shared now rather than
// chat-specific. The renderer stays deliberately dumb — it colors lines by
// their first character and does NOT validate, match hunks, or syntax-
// highlight.

import { useMemo } from 'react';

export type DiffLineKind = 'add' | 'del' | 'hunk' | 'header' | 'context';

export function classifyDiffLine(line: string): DiffLineKind {
  if (line.startsWith('+++') || line.startsWith('---')) return 'header';
  if (line.startsWith('@@')) return 'hunk';
  if (line.startsWith('+')) return 'add';
  if (line.startsWith('-')) return 'del';
  return 'context';
}

export function DiffBody({ diff }: { diff: string }) {
  const lines = useMemo(() => diff.split('\n'), [diff]);
  return (
    <pre className="plume-chat-diff-body">
      {lines.map((line, i) => {
        const kind = classifyDiffLine(line);
        return (
          <span
            key={i}
            className={`plume-chat-diff-line plume-chat-diff-line-${kind}`}
            role={kind === 'add' || kind === 'del' ? 'text' : undefined}
            aria-label={
              kind === 'add'
                ? `Added: ${line.slice(1)}`
                : kind === 'del'
                  ? `Removed: ${line.slice(1)}`
                  : undefined
            }
          >
            {line}
            {'\n'}
          </span>
        );
      })}
    </pre>
  );
}
