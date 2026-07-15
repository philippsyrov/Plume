import { useCallback, useEffect, useState, type ReactNode } from 'react';

import type { ContextSourceRef } from '../../lib/api/chat';
import type { AddContextSourceResult } from './contextSources';
import { PLUME_CONTEXT_MIME, readContextDrop } from './contextDragPayload';

export type ContextDragControls = {
  onDragActiveChange: (active: boolean) => void;
};

export function ContextDropSurface({
  onDropSource,
  disabled,
  children,
}: {
  onDropSource: (source: ContextSourceRef) => Promise<AddContextSourceResult>;
  disabled: boolean;
  children: (controls: ContextDragControls) => ReactNode;
}) {
  const [dragActive, setDragActive] = useState(false);
  const [overDepth, setOverDepth] = useState(0);
  const [notice, setNotice] = useState<string | null>(null);

  useEffect(() => {
    if (!disabled) return;
    setDragActive(false);
    setOverDepth(0);
  }, [disabled]);

  const onDragActiveChange = useCallback(
    (active: boolean) => {
      if (disabled) return;
      setDragActive(active);
      setOverDepth(0);
      if (active) setNotice(null);
    },
    [disabled],
  );

  const visible = dragActive && !disabled;
  const over = overDepth > 0;

  return (
    <div className="plume-context-drop-surface">
      {children({ onDragActiveChange })}
      {visible ? (
        <div
          className={`plume-context-drop-tray${over ? ' plume-context-drop-tray-over' : ''}`}
          aria-hidden="true"
          onDragEnter={(event) => {
            if (!hasContextType(event.dataTransfer)) return;
            event.preventDefault();
            setOverDepth((depth) => depth + 1);
          }}
          onDragOver={(event) => {
            if (!hasContextType(event.dataTransfer)) return;
            event.preventDefault();
            event.dataTransfer.dropEffect = 'copy';
          }}
          onDragLeave={(event) => {
            if (!hasContextType(event.dataTransfer)) return;
            setOverDepth((depth) => Math.max(0, depth - 1));
          }}
          onDrop={(event) => {
            const source = readContextDrop(event.dataTransfer);
            if (source === null) return;
            event.preventDefault();
            setDragActive(false);
            setOverDepth(0);
            void resolveDrop(onDropSource, source).then(setNotice);
          }}
        >
          <span>{over ? 'Release to add to chat' : 'Drop into chat'}</span>
        </div>
      ) : null}
      {notice ? (
        <p className="plume-context-drop-notice" role="status" aria-live="polite">
          {notice}
        </p>
      ) : null}
    </div>
  );
}

function hasContextType(dataTransfer: DataTransfer): boolean {
  return Array.from(dataTransfer.types).includes(PLUME_CONTEXT_MIME);
}

async function resolveDrop(
  onDropSource: (source: ContextSourceRef) => Promise<AddContextSourceResult>,
  source: ContextSourceRef,
): Promise<string | null> {
  try {
    const result = await onDropSource(source);
    if (result === 'full') return 'Context is full. Remove an item in chat, then try again.';
    if (result === 'unavailable') return 'That chat is unavailable right now.';
    return null;
  } catch {
    return 'That chat is unavailable right now.';
  }
}
