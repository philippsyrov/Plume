import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';

import { ChatEntryRow } from './ChatEntryRow';

const mocks = vi.hoisted(() => ({ loadArtifact: vi.fn() }));

vi.mock('../../lib/api/research', () => ({
  loadResearchArtifact: mocks.loadArtifact,
}));

describe('ChatEntryRow', () => {
  it('uses quiet human role labels without changing message accessibility', () => {
    const { rerender } = render(
      <ChatEntryRow entry={{ kind: 'message', message: { role: 'user', content: 'Hello' } }} />,
    );
    expect(screen.getByLabelText('user message')).toHaveTextContent('You');

    rerender(
      <ChatEntryRow
        entry={{
          kind: 'message',
          message: { role: 'assistant', content: 'Hi' },
          modelUsed: 'Qwen Coder 1.5B',
          durationMs: 564,
        }}
      />,
    );
    expect(screen.getByLabelText('assistant message')).toHaveTextContent('Plume');
    expect(screen.getByText(/served by Qwen Coder 1.5B/)).toBeInTheDocument();
  });

  it('renders research as a normal reply with source links and no artifact controls', async () => {
    mocks.loadArtifact.mockResolvedValueOnce({
      artifact: {
        artifactId: 'ra_1', version: 1, createdAtMs: 1, question: 'Dinosaurs',
        providerId: 'mlx-lm', modelId: 'qwen', citationStatus: 'verified', outcome: 'complete',
      },
      markdown: '# Dinosaurs\n\nDinosaurs lived millions of years ago. [^S1]\n\n## Sources\n\n[^S1]: Dinosaur guide',
      sources: [{
        sourceId: 'S1', evidenceId: 'be_1', sourceUrl: 'https://example.com/dinosaurs',
        title: 'Dinosaur guide', capturedAtMs: 1, sha256: 'abc', bytes: 12,
        redactionCount: 0, truncated: false,
      }],
      logicalTurns: 2, providerCalls: 2, durationMs: 5,
    });
    const onOpenResearchSource = vi.fn();

    render(
      <ChatEntryRow
        entry={{
          kind: 'researchArtifact',
          owner: { scope: 'local', sessionId: 's_1' },
          artifactId: 'ra_1',
          version: 1,
        }}
        onOpenResearchSource={onOpenResearchSource}
      />,
    );

    expect(await screen.findByRole('heading', { name: 'Dinosaurs' })).toBeVisible();
    expect(mocks.loadArtifact).toHaveBeenCalledWith({
      owner: { scope: 'local', sessionId: 's_1' },
      artifactId: 'ra_1',
      version: 1,
    });
    expect(screen.getByText(/Dinosaurs lived millions/)).toBeVisible();
    expect(screen.queryByRole('heading', { name: 'Sources' })).not.toBeInTheDocument();
    expect(screen.queryByText(/\[\^S1\]/)).not.toBeInTheDocument();
    const source = screen.getByRole('button', { name: 'Dinosaur guide' });
    await userEvent.click(source);
    expect(onOpenResearchSource).toHaveBeenCalledWith('https://example.com/dinosaurs');
    expect(screen.queryByText(/Citations verified|Sources linked|Details/)).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: /Export|Preview|Sources/i })).not.toBeInTheDocument();
  });

  it('renders an exported Markdown file as one quiet transcript attachment', async () => {
    const onOpenResearchExport = vi.fn();
    render(
      <ChatEntryRow
        entry={{
          kind: 'researchExport',
          owner: { scope: 'local', sessionId: 's_1' },
          artifactId: 'ra_1',
          version: 1,
          fileName: 'dinosaurs.md',
        }}
        onOpenResearchExport={onOpenResearchExport}
      />,
    );

    await userEvent.click(screen.getByRole('button', { name: 'dinosaurs.md' }));
    expect(onOpenResearchExport).toHaveBeenCalledOnce();
    expect(screen.queryByText(/Export Markdown|Details/)).not.toBeInTheDocument();
  });
});
