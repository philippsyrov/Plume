import { render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it } from 'vitest';

import type {
  ChatContextInstructionsPreview,
  ChatMemoryUsage,
  ChatTopicsUsage,
} from '../../lib/api/chat';
import { InstructionsBadge, MemoryBadge, TopicsBadge } from './InstructionsBadge';

describe('prompt context manifest badges', () => {
  it('does not promise next-send instructions when the current preview is unavailable', async () => {
    render(
      <InstructionsBadge
        projectHasInstructions
        lastIncluded={null}
        preview={null}
        previewStatus="ready"
      />,
    );

    expect(
      screen.getByRole('status', { name: 'Project instructions are unavailable for the next send.' }),
    ).toBeVisible();
    expect(screen.queryByText(/will use these instructions/i)).not.toBeInTheDocument();

    await userEvent.click(screen.getByText('Project instructions'));

    const nextSend = screen.getByText('Next send').closest('section');
    expect(nextSend).not.toBeNull();
    expect(within(nextSend!).getByText(/Unavailable/)).toBeVisible();
    expect(within(nextSend!).queryByText(/AGENTS\.md/)).not.toBeInTheDocument();
  });

  it('separates confirmed last-send inclusion from the current instructions preview', async () => {
    render(
      <InstructionsBadge
        projectHasInstructions
        lastIncluded
        preview={{ source: 'AGENTS.md', originalBytes: 777, redactionCount: 1 }}
        previewStatus="ready"
      />,
    );

    await userEvent.click(screen.getByText('Project instructions'));

    const lastSend = screen.getByText('Last send').closest('section');
    const nextSend = screen.getByText('Next send').closest('section');
    expect(lastSend).not.toBeNull();
    expect(nextSend).not.toBeNull();
    expect(within(lastSend!).getByText('Included')).toBeVisible();
    expect(within(lastSend!).queryByText(/777 B/)).not.toBeInTheDocument();
    expect(within(nextSend!).getByText('AGENTS.md')).toBeVisible();
    expect(within(nextSend!).getByText(/777 B/)).toBeVisible();
    expect(within(nextSend!).getByText(/1 redaction/)).toBeVisible();
  });

  it('keeps the project instructions filename and exact facts inside Details', async () => {
    const preview = {
      source: 'AGENTS.md',
      originalBytes: 420,
      redactionCount: 2,
    } satisfies ChatContextInstructionsPreview;

    render(
      <InstructionsBadge
        projectHasInstructions
        lastIncluded={null}
        preview={preview}
        previewStatus="ready"
      />,
    );

    expect(screen.getByText('Project instructions')).toBeVisible();
    expect(screen.queryByText(/¶/)).not.toBeInTheDocument();
    expect(screen.getByText('AGENTS.md')).not.toBeVisible();
    expect(screen.getByText(/420 B/)).not.toBeVisible();

    await userEvent.click(screen.getByText('Project instructions'));

    expect(screen.getByText('AGENTS.md')).toBeVisible();
    expect(screen.getByText(/420 B/)).toBeVisible();
    expect(screen.getByText(/2 redactions/)).toBeVisible();
  });

  it('shows neutral Checking while the instructions preview is idle or loading', () => {
    const { rerender } = render(
      <InstructionsBadge
        projectHasInstructions
        lastIncluded={null}
        preview={null}
        previewStatus="idle"
      />,
    );

    expect(screen.getByRole('status', { name: 'Checking project instructions.' })).toBeVisible();
    expect(screen.getByText('Project instructions').closest('.plume-chat-instructions-badge')).not.toHaveClass(
      'plume-chat-instructions-badge-skipped',
    );

    rerender(
      <InstructionsBadge
        projectHasInstructions
        lastIncluded={null}
        preview={null}
        previewStatus="loading"
      />,
    );
    expect(screen.getByRole('status', { name: 'Checking project instructions.' })).toBeVisible();
  });

  it('distinguishes a preview transport error from a ready unavailable result', async () => {
    render(
      <InstructionsBadge
        projectHasInstructions
        lastIncluded={null}
        preview={null}
        previewStatus="error"
      />,
    );

    expect(screen.getByRole('status', { name: 'Unable to check project instructions.' })).toBeVisible();
    await userEvent.click(screen.getByText('Project instructions'));
    expect(screen.getByText('Unable to check — the context preview request failed.')).toBeVisible();
    expect(screen.queryByText(/could not read the current project instructions/i)).not.toBeInTheDocument();
  });

  it('discloses the exact memory selected for the next send', async () => {
    const user = userEvent.setup();
    const preview = {
      entryCount: 1,
      bytes: 23,
      byteCap: 4096,
      truncated: false,
      entries: [
        {
          id: 'm_0123456789abcdef0123456789abcdef',
          createdAtMs: 1_700_000_000_000,
          textBytes: 23,
          preview: 'Prefer short explanations.',
        },
      ],
    } satisfies ChatMemoryUsage;

    render(<MemoryBadge preview={preview} lastUsed={null} />);

    await user.click(screen.getByText(/Memory · 1 entry/));

    expect(screen.getByText('Next send')).toBeInTheDocument();
    expect(screen.getByText('Prefer short explanations.')).toBeInTheDocument();
    expect(screen.getByText(/…cdef · 23 B/)).toBeInTheDocument();
  });

  it('shows the confirmed last send beside the refreshed next-send manifest', async () => {
    const user = userEvent.setup();
    const preview = memoryUsage('m_0000000000000000000000000000aaaa', 'Preview only');
    const lastUsed = memoryUsage('m_0000000000000000000000000000bbbb', 'Actually sent');

    render(<MemoryBadge preview={preview} lastUsed={lastUsed} />);
    await user.click(screen.getByText(/Memory · 1 entry/));

    expect(screen.getByText('Last send')).toBeInTheDocument();
    expect(screen.getByText('Actually sent')).toBeInTheDocument();
    expect(screen.getByText('Next send')).toBeInTheDocument();
    expect(screen.getByText('Preview only')).toBeInTheDocument();
  });

  it('shows complete memory aggregates and truncation for last and next send', async () => {
    const preview = {
      ...memoryUsage('m_0000000000000000000000000000aaaa', 'Preview kept'),
      bytes: 200,
      byteCap: 500,
      truncated: true,
    } satisfies ChatMemoryUsage;
    const lastUsed = {
      ...memoryUsage('m_0000000000000000000000000000bbbb', 'Last kept'),
      bytes: 100,
      byteCap: 400,
      truncated: true,
    } satisfies ChatMemoryUsage;

    render(<MemoryBadge preview={preview} lastUsed={lastUsed} />);
    await userEvent.click(screen.getByText(/Memory · 1 entry/));

    const lastSend = screen.getByText('Last send').closest('section');
    const nextSend = screen.getByText('Next send').closest('section');
    expect(within(lastSend!).getByText('100 B used · 400 B limit · older content omitted')).toBeVisible();
    expect(within(lastSend!).getByText('Last kept')).toBeVisible();
    expect(within(nextSend!).getByText('200 B used · 500 B limit · older content omitted')).toBeVisible();
    expect(within(nextSend!).getByText('Preview kept')).toBeVisible();
  });

  it('discloses exact topic files with a constrained-width-safe list', async () => {
    const user = userEvent.setup();
    const preview = {
      fileCount: 2,
      bytes: 87,
      byteCap: 6144,
      truncated: false,
      files: [
        { name: 'INDEX.md', bytes: 50 },
        { name: 'USER.md', bytes: 37 },
      ],
    } satisfies ChatTopicsUsage;

    const { container } = render(<TopicsBadge preview={preview} lastUsed={null} />);
    await user.click(screen.getByText(/Topics · 2 files/));

    expect(screen.getByText('Next send')).toBeInTheDocument();
    expect(screen.getByText('INDEX.md')).toBeInTheDocument();
    expect(screen.getByText('50 B')).toBeInTheDocument();
    expect(container.querySelector('.plume-chat-context-manifest-list')).toBeInTheDocument();
  });

  it('shows complete topic aggregates and truncation for last and next send', async () => {
    const preview = topicUsage('USER.md', 80, 90, 6144);
    const lastUsed = topicUsage('SOUL.md', 40, 50, 4096);

    render(<TopicsBadge preview={preview} lastUsed={lastUsed} />);
    await userEvent.click(screen.getByText(/Topics · 1 file/));

    const lastSend = screen.getByText('Last send').closest('section');
    const nextSend = screen.getByText('Next send').closest('section');
    expect(within(lastSend!).getByText('50 B used · 4.0 KB limit · content omitted to fit')).toBeVisible();
    expect(within(lastSend!).getByText('SOUL.md')).toBeVisible();
    expect(within(nextSend!).getByText('90 B used · 6.0 KB limit · content omitted to fit')).toBeVisible();
    expect(within(nextSend!).getByText('USER.md')).toBeVisible();
  });
});

function memoryUsage(id: string, preview: string): ChatMemoryUsage {
  return {
    entryCount: 1,
    bytes: 13,
    byteCap: 4096,
    truncated: false,
    entries: [{ id, createdAtMs: 1_700_000_000_000, textBytes: 13, preview }],
  };
}

function topicUsage(name: string, fileBytes: number, bytes: number, byteCap: number): ChatTopicsUsage {
  return {
    fileCount: 1,
    bytes,
    byteCap,
    truncated: true,
    files: [{ name, bytes: fileBytes }],
  };
}
