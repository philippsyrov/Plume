import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it } from 'vitest';

import type { ChatMemoryUsage, ChatTopicsUsage } from '../../lib/api/chat';
import { MemoryBadge, TopicsBadge } from './InstructionsBadge';

describe('prompt context manifest badges', () => {
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
    await user.click(screen.getByText(/Memory · 1 entry · 13 B/));

    expect(screen.getByText('Last send')).toBeInTheDocument();
    expect(screen.getByText('Actually sent')).toBeInTheDocument();
    expect(screen.getByText('Next send')).toBeInTheDocument();
    expect(screen.getByText('Preview only')).toBeInTheDocument();
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
