import { fireEvent, render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';

import { PLUME_CONTEXT_MIME } from './contextDragPayload';
import { ContextDragAction } from './ContextDragAction';

function fakeTransfer(): DataTransfer {
  const values = new Map<string, string>();
  return {
    effectAllowed: 'uninitialized',
    get types() {
      return [...values.keys()];
    },
    getData: (type: string) => values.get(type) ?? '',
    setData: (type: string, value: string) => values.set(type, value),
  } as unknown as DataTransfer;
}

describe('ContextDragAction', () => {
  it('keeps click activation and writes only the opaque drag reference', () => {
    const source = { kind: 'memoryEntry', entryId: `m_${'a'.repeat(32)}` } as const;
    const onActivate = vi.fn();
    const onDragActiveChange = vi.fn();
    const dataTransfer = fakeTransfer();

    render(
      <ContextDragAction
        source={source}
        onActivate={onActivate}
        onDragActiveChange={onDragActiveChange}
      >
        Use in chat
      </ContextDragAction>,
    );

    const action = screen.getByRole('button', { name: 'Use in chat' });
    expect(action).toHaveAttribute('draggable', 'true');
    expect(action).toHaveAttribute('title', 'Drag to chat');

    fireEvent.click(action);
    expect(onActivate).toHaveBeenCalledWith(source);

    fireEvent.dragStart(action, { dataTransfer });
    expect(onDragActiveChange).toHaveBeenCalledWith(true);
    expect(JSON.parse(dataTransfer.getData(PLUME_CONTEXT_MIME))).toEqual(source);
    expect(dataTransfer.getData('text/plain')).toBe('');

    fireEvent.dragEnd(action);
    expect(onDragActiveChange).toHaveBeenLastCalledWith(false);
  });
});
