import { fireEvent, render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';

import type { ContextSourceRef } from '../../lib/api/chat';
import { PLUME_CONTEXT_MIME } from '../chat/contextDragPayload';
import { FileInspector, type FileNavigatorState } from './FileBrowser';

vi.mock('../editor/ReadOnlyEditor', () => ({
  ReadOnlyEditor: ({ content }: { content: string }) => <pre>{content}</pre>,
}));

function readyState(range: FileNavigatorState['currentLineRange']): FileNavigatorState {
  return {
    projectRoot: '/project',
    relDir: '',
    setRelDir: vi.fn(),
    listing: { kind: 'ready', entries: [] },
    selection: {
      kind: 'ready',
      path: 'src/App.tsx',
      content: { content: 'one\ntwo\nthree', encoding: 'utf-8', bytes: 13 },
    },
    onSelectEntry: vi.fn(),
    quickOpen: {
      query: '',
      setQuery: vi.fn(),
      state: { kind: 'ready', files: [], truncated: false },
      openPath: vi.fn(),
      refresh: vi.fn(),
    },
    currentLineRange: range,
    setCurrentLineRange: vi.fn(),
  };
}

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

describe('FileInspector context action', () => {
  it('clicks and drags the exact snapshotted file selection ref', () => {
    const source: ContextSourceRef = {
      kind: 'projectFile',
      relPath: 'src/App.tsx',
      startLine: 2,
      endLine: 3,
    };
    const onUseInChat = vi.fn();
    const onContextDragActiveChange = vi.fn();
    render(
      <FileInspector
        state={readyState({ startLine: 2, endLine: 3 })}
        contextSource={source}
        onUseInChat={onUseInChat}
        onContextDragActiveChange={onContextDragActiveChange}
      />,
    );

    const action = screen.getByRole('button', { name: 'Use selection in chat' });
    fireEvent.click(action);
    expect(onUseInChat).toHaveBeenCalledWith(source);

    const dataTransfer = fakeTransfer();
    fireEvent.dragStart(action, { dataTransfer });
    expect(JSON.parse(dataTransfer.getData(PLUME_CONTEXT_MIME))).toEqual(source);
    expect(onContextDragActiveChange).toHaveBeenCalledWith(true);
    fireEvent.dragEnd(action);
    expect(onContextDragActiveChange).toHaveBeenLastCalledWith(false);
  });

  it('renders no context action without an eligible source', () => {
    render(<FileInspector state={readyState(null)} contextSource={null} />);
    expect(screen.queryByRole('button', { name: /Use .* in chat/ })).not.toBeInTheDocument();
  });
});
