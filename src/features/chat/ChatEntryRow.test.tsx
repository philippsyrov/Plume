import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';

import { ChatEntryRow } from './ChatEntryRow';

const mocks = vi.hoisted(() => ({ loadArtifact: vi.fn() }));

vi.mock('../../lib/api/research', () => ({
  loadResearchArtifact: mocks.loadArtifact,
}));

describe('ChatEntryRow', () => {
  it('keeps message accessibility without visible speaker labels', () => {
    const { rerender } = render(
      <ChatEntryRow entry={{ kind: 'message', message: { role: 'user', content: 'Hello' } }} />,
    );
    expect(screen.getByLabelText('user message')).toHaveTextContent('Hello');
    expect(screen.queryByText('You')).not.toBeInTheDocument();

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
    expect(screen.getByLabelText('assistant message')).toHaveTextContent('Hi');
    expect(screen.queryByText('Plume')).not.toBeInTheDocument();
    expect(screen.getByText(/served by Qwen Coder 1.5B/)).toBeInTheDocument();
  });

  it('shows Browser context as compact icon labels while preserving exact provenance', () => {
    render(
      <ChatEntryRow
        entry={{
          kind: 'message',
          message: { role: 'user', content: 'What is this?' },
          contextSources: [
            {
              kind: 'browserTextEvidence', evidenceId: `be_${'a'.repeat(32)}`,
              captureKind: 'page', sourceUrl: 'https://example.com/dinosaurs',
              title: 'A very long dinosaur page title', capturedAtMs: 1, bytes: 42,
              redactionCount: 0, truncated: false, preview: 'Dinosaurs had feathers.',
            },
            {
              kind: 'browserScreenshotEvidence', evidenceId: `bs_${'b'.repeat(32)}`,
              sourceUrl: 'https://example.com/dinosaurs', title: 'A very long screenshot title',
              capturedAtMs: 2, width: 1064, height: 1088, bytes: 84,
              sha256: 'ab'.repeat(32),
            },
          ],
        }}
      />,
    );

    expect(screen.getByText('Website')).toBeVisible();
    expect(screen.getByText('Screenshot')).toBeVisible();
    expect(screen.queryByText(/A very long|1064×1088|¶/)).not.toBeInTheDocument();
    expect(screen.getByLabelText('Website: A very long dinosaur page title')).toBeVisible();
    expect(screen.getByLabelText('Screenshot: A very long screenshot title')).toBeVisible();
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
    expect(screen.queryByText('Plume')).not.toBeInTheDocument();
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
    expect(screen.queryByText('Plume')).not.toBeInTheDocument();
  });

  it('keeps a citation-review warning visible without adding controls', async () => {
    mocks.loadArtifact.mockResolvedValueOnce({
      artifact: {
        artifactId: 'ra_2', version: 1, createdAtMs: 1, question: 'Dinosaurs',
        providerId: 'mlx-lm', modelId: 'qwen', citationStatus: 'needsReview', outcome: 'needsReview',
      },
      markdown: 'A small-model draft.', sources: [], logicalTurns: 1, providerCalls: 1, durationMs: 2,
    });
    render(<ChatEntryRow entry={{
      kind: 'researchArtifact', owner: { scope: 'local', sessionId: 's_1' },
      artifactId: 'ra_2', version: 1,
    }} />);

    expect(await screen.findByText('Draft — check citations.')).toBeVisible();
    expect(screen.queryByRole('button')).not.toBeInTheDocument();
  });

  it('keeps unsafe sources as text and reports source-action failures beside the sources', async () => {
    mocks.loadArtifact.mockResolvedValueOnce({
      artifact: {
        artifactId: 'ra_3', version: 1, createdAtMs: 1, question: 'Sources',
        providerId: 'mlx-lm', modelId: 'qwen', citationStatus: 'verified', outcome: 'complete',
      },
      markdown: 'A sourced note.',
      sources: [
        {
          sourceId: 'S1', evidenceId: 'be_1', sourceUrl: 'file:///private/note',
          title: 'Local note', capturedAtMs: 1, sha256: 'abc', bytes: 12,
          redactionCount: 0, truncated: false,
        },
        {
          sourceId: 'S2', evidenceId: 'be_2', sourceUrl: 'https://example.com',
          title: 'Web note', capturedAtMs: 1, sha256: 'def', bytes: 12,
          redactionCount: 0, truncated: false,
        },
      ],
      logicalTurns: 1, providerCalls: 1, durationMs: 2,
    });
    const onOpenResearchSource = vi.fn().mockRejectedValue(new Error('Could not open source.'));
    render(<ChatEntryRow entry={{
      kind: 'researchArtifact', owner: { scope: 'local', sessionId: 's_1' },
      artifactId: 'ra_3', version: 1,
    }} onOpenResearchSource={onOpenResearchSource} />);

    expect(await screen.findByText('Local note')).not.toHaveAttribute('role', 'button');
    expect(screen.queryByRole('button', { name: 'Local note' })).not.toBeInTheDocument();
    await userEvent.click(screen.getByRole('button', { name: 'Web note' }));
    expect(await screen.findByRole('alert')).toHaveTextContent('Could not open source.');
  });
});
