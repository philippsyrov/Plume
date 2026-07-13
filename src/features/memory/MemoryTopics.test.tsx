import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import { useChatContextPreview } from '../chat/useChatContextPreview';
import { __resetMemoryRevisionForTests } from './memoryRevision';
import { MemoryTopicsDisclosure } from './MemoryTopics';

const mocks = vi.hoisted(() => ({
  getMemoryTopics: vi.fn(),
  previewChatContext: vi.fn(),
}));

vi.mock('../../lib/api/memory', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../../lib/api/memory')>();
  return { ...actual, getMemoryTopics: mocks.getMemoryTopics };
});

vi.mock('../../lib/api/chat', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../../lib/api/chat')>();
  return { ...actual, previewChatContext: mocks.previewChatContext };
});

describe('MemoryTopicsDisclosure', () => {
  beforeEach(() => {
    __resetMemoryRevisionForTests();
    mocks.getMemoryTopics.mockResolvedValue(emptyTopics());
    mocks.previewChatContext.mockResolvedValue({
      instructions: null,
      attachment: null,
      memory: null,
      topics: null,
    });
  });

  it('refreshes the next-send context preview after a successful topic reread', async () => {
    const user = userEvent.setup();
    render(
      <>
        <PreviewProbe />
        <MemoryTopicsDisclosure />
      </>,
    );

    await waitFor(() => expect(mocks.previewChatContext).toHaveBeenCalledTimes(1));
    await user.click(screen.getByRole('button', { name: 'Topic files' }));
    await screen.findByRole('button', { name: 'Refresh' });
    await waitFor(() => expect(mocks.previewChatContext).toHaveBeenCalledTimes(2));

    await user.click(screen.getByRole('button', { name: 'Refresh' }));

    await waitFor(() => expect(mocks.getMemoryTopics).toHaveBeenCalledTimes(2));
    await waitFor(() => expect(mocks.previewChatContext).toHaveBeenCalledTimes(3));
  });
});

function PreviewProbe() {
  useChatContextPreview({
    relPath: null,
    startLine: null,
    endLine: null,
    projectHasInstructions: false,
  });
  return null;
}

function emptyTopics() {
  return {
    core: [
      { name: 'INDEX.md', exists: false, bytes: 0, truncated: false, content: null },
      { name: 'USER.md', exists: false, bytes: 0, truncated: false, content: null },
      { name: 'SOUL.md', exists: false, bytes: 0, truncated: false, content: null },
    ],
    topics: [],
    topicsTruncated: false,
    limits: { maxFileBytes: 32768, maxTopics: 50 },
  };
}
