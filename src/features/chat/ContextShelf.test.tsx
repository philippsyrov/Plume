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

  it('shows captured browser text as ordinary provenance-bearing context', () => {
    const source: ContextSourceRef = {
      kind: 'browserTextEvidence',
      evidenceId: `be_${'b'.repeat(32)}`,
    };
    render(
      <ContextShelf
        sources={[source]}
        preview={[
          {
            status: 'ready',
            source: {
              kind: 'browserTextEvidence',
              evidenceId: source.evidenceId,
              captureKind: 'page',
              sourceUrl: 'https://example.com/research',
              title: 'Research',
              capturedAtMs: 7,
              bytes: 42,
              redactionCount: 2,
              truncated: true,
              preview: 'A short redacted research excerpt.',
            },
          },
        ]}
        loading={false}
        disabled={false}
        onRemove={vi.fn()}
      />,
    );
    expect(screen.getByRole('listitem')).toHaveTextContent('Web');
    expect(screen.getByRole('listitem')).toHaveTextContent('Page · Research · example.com');
    expect(screen.getByRole('listitem')).toHaveTextContent(
      '42 B · 2 redacted · shortened',
    );
    expect(screen.getByRole('listitem')).toHaveTextContent(
      'A short redacted research excerpt.',
    );
  });

  it('shows screenshot provenance and an honest model block', () => {
    const source: ContextSourceRef = {
      kind: 'browserScreenshotEvidence',
      evidenceId: `bs_${'d'.repeat(32)}`,
    };
    const { rerender } = render(
      <ContextShelf
        sources={[source]}
        preview={[{
          status: 'ready',
          source: {
            kind: 'browserScreenshotEvidence',
            evidenceId: source.evidenceId,
            sourceUrl: 'https://example.com/page',
            title: 'Example',
            capturedAtMs: 9,
            width: 800,
            height: 600,
            bytes: 1234,
            sha256: 'ab'.repeat(32),
          },
        }]}
        loading={false}
        disabled={false}
        onRemove={vi.fn()}
      />,
    );
    expect(screen.getByRole('listitem')).toHaveTextContent('Image');
    expect(screen.getByRole('listitem')).toHaveTextContent('Screenshot · Example · example.com');
    expect(screen.getByRole('listitem')).toHaveTextContent('800×600 · 1.2 KB');

    rerender(
      <ContextShelf
        sources={[source]}
        preview={[{
          status: 'blocked',
          ref: source,
          reason: 'blocked',
          message: 'This model cannot use screenshots.',
        }]}
        loading={false}
        disabled={false}
        onRemove={vi.fn()}
      />,
    );
    expect(screen.getByRole('listitem')).toHaveTextContent('blocked');
    expect(screen.getByRole('listitem')).toHaveAttribute('title', 'This model cannot use screenshots.');
  });
});
