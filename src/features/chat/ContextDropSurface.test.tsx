import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';

import { ContextDropSurface } from './ContextDropSurface';
import { PLUME_CONTEXT_MIME } from './contextDragPayload';

function transfer(payload?: unknown, type = PLUME_CONTEXT_MIME): DataTransfer {
  const values = new Map<string, string>();
  if (payload !== undefined) values.set(type, JSON.stringify(payload));
  return {
    dropEffect: 'none',
    effectAllowed: 'copy',
    get types() {
      return [...values.keys()];
    },
    getData: (key: string) => values.get(key) ?? '',
    setData: (key: string, value: string) => values.set(key, value),
  } as unknown as DataTransfer;
}

const memorySource = { kind: 'memoryEntry', entryId: `m_${'b'.repeat(32)}` } as const;

describe('ContextDropSurface', () => {
  it('appears only for an enabled internal drag and tracks nested hover depth', () => {
    render(
      <ContextDropSurface onDropSource={vi.fn()} disabled={false}>
        {({ onDragActiveChange }) => (
          <button type="button" onClick={() => onDragActiveChange(true)}>
            Start drag
          </button>
        )}
      </ContextDropSurface>,
    );

    expect(screen.queryByText('Drop into project chat')).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Start drag' }));
    const tray = screen.getByText('Drop into project chat').closest('div')!;
    const dataTransfer = transfer(memorySource);

    fireEvent.dragEnter(tray, { dataTransfer });
    fireEvent.dragEnter(tray, { dataTransfer });
    expect(screen.getByText('Release to add to chat')).toBeInTheDocument();

    fireEvent.dragLeave(tray, { dataTransfer });
    expect(screen.getByText('Release to add to chat')).toBeInTheDocument();
    fireEvent.dragLeave(tray, { dataTransfer });
    expect(screen.getByText('Drop into project chat')).toBeInTheDocument();
  });

  it('ignores foreign drops and adds one parsed source on a Plume drop', async () => {
    const onDropSource = vi.fn().mockResolvedValue('added');
    render(
      <ContextDropSurface onDropSource={onDropSource} disabled={false}>
        {({ onDragActiveChange }) => (
          <button type="button" onClick={() => onDragActiveChange(true)}>
            Start drag
          </button>
        )}
      </ContextDropSurface>,
    );

    fireEvent.click(screen.getByRole('button', { name: 'Start drag' }));
    let tray = screen.getByText('Drop into project chat').closest('div')!;
    fireEvent.drop(tray, { dataTransfer: transfer({ nope: true }, 'text/plain') });
    expect(onDropSource).not.toHaveBeenCalled();

    tray = screen.getByText('Drop into project chat').closest('div')!;
    fireEvent.drop(tray, { dataTransfer: transfer(memorySource) });
    await waitFor(() => expect(onDropSource).toHaveBeenCalledWith(memorySource));
    expect(screen.queryByText('Drop into project chat')).not.toBeInTheDocument();
  });

  it.each([
    ['full', 'Context is full. Remove an item in chat, then try again.'],
    ['unavailable', 'Project chat is unavailable right now.'],
  ] as const)('announces a %s result and leaves the source surface usable', async (result, copy) => {
    const onDropSource = vi.fn().mockResolvedValue(result);
    render(
      <ContextDropSurface onDropSource={onDropSource} disabled={false}>
        {({ onDragActiveChange }) => (
          <button type="button" onClick={() => onDragActiveChange(true)}>
            Start drag
          </button>
        )}
      </ContextDropSurface>,
    );

    fireEvent.click(screen.getByRole('button', { name: 'Start drag' }));
    const tray = screen.getByText('Drop into project chat').closest('div')!;
    fireEvent.drop(tray, { dataTransfer: transfer(memorySource) });

    expect(await screen.findByRole('status')).toHaveTextContent(copy);
    expect(screen.getByRole('button', { name: 'Start drag' })).toBeInTheDocument();
  });

  it('does not reveal a destination when disabled', () => {
    render(
      <ContextDropSurface onDropSource={vi.fn()} disabled>
        {({ onDragActiveChange }) => (
          <button type="button" onClick={() => onDragActiveChange(true)}>
            Start drag
          </button>
        )}
      </ContextDropSurface>,
    );

    fireEvent.click(screen.getByRole('button', { name: 'Start drag' }));
    expect(screen.queryByText('Drop into project chat')).not.toBeInTheDocument();
  });
});
