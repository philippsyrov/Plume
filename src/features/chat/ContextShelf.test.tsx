import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';

import { ContextShelf } from './ContextShelf';
import type { ContextSourcePreviewItem, ContextSourceRef } from '../../lib/api/chat';

describe('ContextShelf', () => {
  it('renders ordered ready and blocked sources and removes the exact ref', async () => {
    const sources: ContextSourceRef[] = [
      { kind: 'projectFile', relPath: 'src/App.tsx', startLine: 4, endLine: 8 },
      { kind: 'topicFile', name: 'topics/missing.md' },
    ];
    const preview: ContextSourcePreviewItem[] = [
      {
        status: 'ready',
        source: {
          kind: 'projectFile',
          relPath: 'src/App.tsx',
          startLine: 4,
          endLine: 8,
          bytes: 96,
          originalBytes: 120,
          redactionCount: 1,
        },
      },
      {
        status: 'blocked',
        ref: sources[1],
        reason: 'notFound',
        message: 'topic file no longer exists',
      },
    ];
    const onRemove = vi.fn();

    render(
      <ContextShelf
        sources={sources}
        preview={preview}
        loading={false}
        disabled={false}
        onRemove={onRemove}
      />,
    );

    const items = screen.getAllByRole('listitem');
    expect(items[0]).toHaveTextContent('File');
    expect(items[0]).toHaveTextContent('src/App.tsx:4–8');
    expect(items[0]).toHaveTextContent('96 B');
    expect(items[1]).toHaveTextContent('Topic');
    expect(items[1]).toHaveTextContent('blocked');
    expect(items[1]).toHaveAttribute('title', 'topic file no longer exists');

    await userEvent.click(
      screen.getByRole('button', { name: 'Remove topics/missing.md from context' }),
    );
    expect(onRemove).toHaveBeenCalledWith(sources[1]);
  });

  it('emphasizes only the exact matching source key', () => {
    const sources: ContextSourceRef[] = [
      { kind: 'memoryEntry', entryId: `m_${'a'.repeat(32)}` },
      { kind: 'topicFile', name: 'topics/alpha.md' },
    ];
    render(
      <ContextShelf
        sources={sources}
        preview={[]}
        loading={false}
        disabled={false}
        emphasizedContextKey={`memory:m_${'a'.repeat(32)}`}
        onRemove={vi.fn()}
      />,
    );

    const items = screen.getAllByRole('listitem');
    expect(items[0]).toHaveClass('plume-context-shelf-item-emphasized');
    expect(items[1]).not.toHaveClass('plume-context-shelf-item-emphasized');
  });
});
