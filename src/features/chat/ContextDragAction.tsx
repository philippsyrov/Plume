import type { ReactNode } from 'react';

import type { ContextSourceRef } from '../../lib/api/chat';
import { writeContextDrag } from './contextDragPayload';

export function ContextDragAction({
  source,
  onActivate,
  onDragActiveChange,
  children,
  className,
}: {
  source: ContextSourceRef;
  onActivate: (source: ContextSourceRef) => void | Promise<unknown>;
  onDragActiveChange: (active: boolean) => void;
  children: ReactNode;
  className?: string;
}) {
  return (
    <button
      type="button"
      draggable
      className={className}
      title="Drag to chat"
      onClick={() => void onActivate(source)}
      onDragStart={(event) => {
        writeContextDrag(event.dataTransfer, source);
        onDragActiveChange(true);
      }}
      onDragEnd={() => onDragActiveChange(false)}
    >
      {children}
    </button>
  );
}
