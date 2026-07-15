import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';

import { ContextShelf } from './ContextShelf';
import type { ContextSourcePreviewItem, ContextSourceRef } from '../../lib/api/chat';

describe('ContextShelf', () => {
  it('uses a readable memory title and keeps the opaque id and bytes in Details', async () => {
    const source: ContextSourceRef = {
      kind: 'memoryEntry',
      entryId: `m_${'a'.repeat(32)}`,
    };
    render(
      <ContextShelf
        sources={[source]}
        preview={[{
          status: 'ready',
          source: {
            kind: 'memoryEntry',
            entryId: source.entryId,
            createdAtMs: 1_700_000_000_000,
            bytes: 32,
            preview: 'Keep explanations concrete.',
          },
        }]}
        loading={false}
        disabled={false}
        onRemove={vi.fn()}
      />,
    );

    expect(screen.getByText('Keep explanations concrete.')).toBeVisible();
    expect(screen.getByText(source.entryId)).not.toBeVisible();
    expect(screen.getByText('32 B')).not.toBeVisible();

    await userEvent.click(screen.getByText('Details'));

    expect(screen.getByText(source.entryId)).toBeVisible();
    expect(screen.getByText('32 B')).toBeVisible();
  });

  it('labels app-private user memory as memory in ready and loading states', () => {
    const source: ContextSourceRef = {
      kind: 'userMemoryEntry',
      entryId: `m_${'b'.repeat(32)}`,
    };
    const { rerender } = render(
      <ContextShelf
        sources={[source]}
        preview={[{
          status: 'ready',
          source: {
            kind: 'userMemoryEntry',
            entryId: source.entryId,
            createdAtMs: 1_700_000_000_000,
            bytes: 28,
            preview: 'Prefers worked examples.',
          },
        }]}
        loading={false}
        disabled={false}
        onRemove={vi.fn()}
      />,
    );

    expect(screen.getByRole('listitem')).toHaveTextContent('Prefers worked examples.');
    expect(screen.getByRole('listitem')).not.toHaveTextContent('Captured screenshot');

    rerender(
      <ContextShelf
        sources={[source]}
        preview={[]}
        loading
        disabled={false}
        onRemove={vi.fn()}
      />,
    );

    expect(screen.getByRole('listitem')).toHaveTextContent('Saved user memory');
    expect(screen.getByRole('listitem')).not.toHaveTextContent('Captured screenshot');
  });

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
    expect(items[0]).toHaveTextContent('App.tsx · lines 4–8');
    expect(items[1]).toHaveTextContent('Topic');
    expect(items[1]).toHaveTextContent('blocked');
    expect(items[1]).toHaveAttribute('title', 'topic file no longer exists');

    await userEvent.click(screen.getAllByText('Details')[0]);
    expect(screen.getByText('src/App.tsx:4–8')).toBeVisible();
    expect(screen.getByText(/96 B/)).toBeVisible();

    await userEvent.click(
      screen.getByRole('button', { name: 'Remove missing.md from context' }),
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

  it('shows captured browser text as ordinary provenance-bearing context', async () => {
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
      'A short redacted research excerpt.',
    );
    await userEvent.click(screen.getByText('Details'));
    expect(screen.getByRole('listitem')).toHaveTextContent(
      '42 B · 2 redacted · shortened',
    );
  });

  it('shows screenshot provenance and an honest model block', async () => {
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
    await userEvent.click(screen.getByText('Details'));
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
