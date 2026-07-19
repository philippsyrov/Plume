import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { expect, it, vi } from 'vitest';

import type { ResearchLoadArtifactResponse } from '../../lib/api/research';
import { ResearchArtifactCard } from './ResearchArtifactCard';

function artifact(citationStatus: 'verified' | 'needsReview'): ResearchLoadArtifactResponse {
  return {
    artifact: {
      artifactId: 'ra_1',
      version: 1,
      createdAtMs: 1,
      question: 'What changed?',
      providerId: 'mlx-lm',
      modelId: 'qwen-coder-1.5b-mlx-4bit',
      citationStatus,
      outcome: citationStatus === 'verified' ? 'complete' : 'needsReview',
    },
    markdown: '# Note\n\nA claim. [^S1]\n\n## Sources\n\n[^S1]: Example',
    sources: [
      {
        sourceId: 'S1',
        evidenceId: 'be_1',
        sourceUrl: 'https://example.com',
        title: 'Example',
        capturedAtMs: 1,
        sha256: 'abc',
        bytes: 12,
        redactionCount: 0,
        truncated: false,
      },
    ],
    logicalTurns: 2,
    providerCalls: 2,
    durationMs: 5,
  };
}

it('shows verified provenance without claiming facts or relevance were checked', async () => {
  const onExport = vi.fn();
  onExport.mockResolvedValue({ status: 'cancelled' });
  render(<ResearchArtifactCard artifact={artifact('verified')} onExport={onExport} />);

  expect(screen.getByText('Citations verified')).toBeVisible();
  expect(screen.queryByText(/Facts verified/i)).not.toBeInTheDocument();
  expect(screen.getByText(/does not verify relevance or factual accuracy/i)).toBeVisible();
  await userEvent.click(screen.getByRole('button', { name: 'Sources' }));
  expect(screen.getByText('Example')).toBeVisible();
  expect(screen.getByText('https://example.com')).toBeVisible();
  await userEvent.click(screen.getByRole('button', { name: 'Export Markdown' }));
  expect(onExport).toHaveBeenCalledOnce();
  await waitFor(() => expect(screen.getByRole('button', { name: 'Export Markdown' })).toHaveFocus());
});

it('keeps a review-needed draft eligible for Preview, Sources, and Export', () => {
  render(
    <ResearchArtifactCard
      artifact={artifact('needsReview')}
      onExport={vi.fn().mockResolvedValue({ status: 'saved', fileName: 'note.md' })}
    />,
  );

  expect(screen.getByText('Draft — citations need review')).toBeVisible();
  expect(screen.getByRole('button', { name: 'Preview' })).toBeEnabled();
  expect(screen.getByRole('button', { name: 'Sources' })).toBeEnabled();
  expect(screen.getByRole('button', { name: 'Export Markdown' })).toBeEnabled();
});

it('keeps the artifact visible and reports an export failure inline', async () => {
  render(
    <ResearchArtifactCard
      artifact={artifact('verified')}
      onExport={vi.fn().mockRejectedValue(new Error('Disk is full'))}
    />,
  );
  await userEvent.click(screen.getByRole('button', { name: 'Export Markdown' }));

  expect(await screen.findByRole('alert')).toHaveTextContent('Disk is full');
  expect(screen.getByRole('heading', { name: 'Note' })).toBeVisible();
  expect(screen.getByRole('button', { name: 'Export Markdown' })).toHaveFocus();
});
