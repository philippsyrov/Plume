import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import type { MemoryEntry, MemoryIndex, MemoryTopicFile, MemoryTopics } from '../../lib/api/memory';

const mocks = vi.hoisted(() => ({
  refreshAll: vi.fn(),
  retryMemory: vi.fn(),
  retryTopics: vi.fn(),
  useKnowledgeData: vi.fn(),
}));

vi.mock('./useKnowledgeData', () => ({ useKnowledgeData: mocks.useKnowledgeData }));

import { KnowledgePanel } from './KnowledgePanel';

const limits = { maxEntries: 100, maxBytesPerEntry: 1024, maxBytesTotal: 65_536 };
const topicLimits = { maxCoreBytes: 2048, maxTopicBytes: 8192, maxTopics: 32 };

function entry(
  id: string,
  text: string,
  createdMs: number,
  links: string[] = [],
  redactionCount = 0,
): MemoryEntry {
  return { id, text, createdMs, links, redactionCount };
}

function topic(
  name: string,
  content: string,
  overrides: Partial<MemoryTopicFile> = {},
): MemoryTopicFile {
  return {
    name,
    content,
    kind: name.startsWith('topics/') ? 'topic' : 'index',
    exists: true,
    bytes: content.length,
    truncated: false,
    ...overrides,
  };
}

function indexFixture(entries: MemoryEntry[] = fixtureEntries()): MemoryIndex {
  return { entries, limits, totalBytes: 256 };
}

function topicsFixture(files: MemoryTopicFile[] = fixtureTopics()): MemoryTopics {
  return { core: [], topics: files, topicsTruncated: false, limits: topicLimits };
}

function fixtureEntries(): MemoryEntry[] {
  return [
    entry('m_alpha', 'Prefer Rust for the local core', Date.UTC(2026, 6, 12), ['topics/alpha.md'], 2),
    entry('m_beta', 'Use TypeScript for the UI', Date.UTC(2026, 6, 11), ['topics/beta.md']),
    entry('m_stale', 'Old deployment note', Date.UTC(2026, 6, 10), ['topics/removed.md']),
    entry('m_unlinked', 'Unsorted scratch note', Date.UTC(2026, 6, 9)),
  ];
}

function fixtureTopics(): MemoryTopicFile[] {
  return [
    topic('topics/alpha.md', 'Alpha topic body'),
    topic('topics/beta.md', 'Beta topic body', { truncated: true, bytes: 10_000 }),
  ];
}

function readyData(entries = fixtureEntries(), topics = fixtureTopics()) {
  return {
    memory: { kind: 'ready' as const, data: indexFixture(entries) },
    topics: { kind: 'ready' as const, data: topicsFixture(topics) },
    retryMemory: mocks.retryMemory,
    retryTopics: mocks.retryTopics,
    refreshAll: mocks.refreshAll,
  };
}

beforeEach(() => {
  vi.clearAllMocks();
  mocks.useKnowledgeData.mockReturnValue(readyData());
});

describe('KnowledgePanel', () => {
  it('renders exact counts, topic content, and memory provenance from both ready sources', () => {
    render(<KnowledgePanel />);

    expect(screen.getByRole('button', { name: 'All memories 4' })).toHaveAttribute(
      'aria-current',
      'page',
    );
    expect(screen.getByRole('button', { name: 'Unlinked 1' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Stale links 1' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'topics/alpha.md 1 backlink' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'topics/beta.md 1 backlink' })).toBeInTheDocument();
    expect(screen.getByText('Alpha topic body')).toBeInTheDocument();
    expect(screen.getByText('Beta topic body')).toBeInTheDocument();
    expect(screen.getByText('Content truncated by the backend.')).toBeInTheDocument();

    const card = screen.getByRole('article', { name: 'Memory m_alpha' });
    expect(card).toHaveTextContent('Prefer Rust for the local core');
    expect(card).toHaveTextContent('m_alpha');
    expect(card).toHaveTextContent('2 redacted');
    expect(card).toHaveTextContent('topics/alpha.md');
    expect(card.querySelector('time')).toHaveAttribute('dateTime', '2026-07-12T00:00:00.000Z');
    const staleCard = screen.getByRole('article', { name: 'Memory m_stale' });
    expect(staleCard).toHaveTextContent(
      'topics/removed.md · missing topic',
    );
    expect(staleCard.querySelector('.is-stale')).toHaveTextContent('topics/removed.md');
  });

  it('reports truncated topic coverage without counting capped-out canonical refs as stale', () => {
    const cappedTopics = topicsFixture([topic('topics/alpha.md', 'Alpha topic body')]);
    cappedTopics.topicsTruncated = true;
    mocks.useKnowledgeData.mockReturnValue({
      ...readyData(),
      memory: {
        kind: 'ready',
        data: indexFixture([
          entry('m_capped', 'Valid link beyond cap', 2, ['topics/zeta.md']),
          entry('m_missing', 'Definitely missing link', 1, ['not-a-topic.md']),
        ]),
      },
      topics: { kind: 'ready', data: cappedTopics },
    });

    render(<KnowledgePanel />);

    expect(screen.getByRole('button', { name: 'Stale links 1' })).toBeInTheDocument();
    expect(screen.getByText(/topic coverage is partial/i)).toHaveTextContent(
      'Topic coverage is partial: only the first 32 topic files are shown.',
    );
    expect(screen.getByRole('article', { name: 'Memory m_capped' })).toHaveTextContent(
      'topics/zeta.md · not verified (topic list capped)',
    );
    expect(screen.getByRole('article', { name: 'Memory m_capped' })).not.toHaveTextContent(
      'missing topic',
    );
    expect(screen.getByRole('article', { name: 'Memory m_missing' })).toHaveTextContent(
      'not-a-topic.md · missing topic',
    );
  });

  it('does not project backlinks from surfaced files whose refs are noncanonical', () => {
    const noncanonicalTopics = topicsFixture([topic('topics/.hidden.md', 'Hidden topic body')]);
    noncanonicalTopics.core = [topic('INDEX.md', 'Index body', { kind: 'index' })];
    mocks.useKnowledgeData.mockReturnValue({
      ...readyData(),
      memory: {
        kind: 'ready',
        data: indexFixture([
          entry('m_core', 'Legacy core ref', 2, ['INDEX.md']),
          entry('m_hidden', 'Legacy hidden ref', 1, ['topics/.hidden.md']),
        ]),
      },
      topics: { kind: 'ready', data: noncanonicalTopics },
    });

    render(<KnowledgePanel />);

    expect(screen.getByRole('button', { name: 'Stale links 2' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'INDEX.md 0 backlinks' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'topics/.hidden.md 0 backlinks' })).toBeInTheDocument();
    expect(screen.getByRole('article', { name: 'Memory m_core' })).toHaveTextContent(
      'INDEX.md · missing topic',
    );
    expect(screen.getByRole('article', { name: 'Memory m_hidden' })).toHaveTextContent(
      'topics/.hidden.md · missing topic',
    );
  });

  it('shows only exact backlinks after selecting a topic', async () => {
    const user = userEvent.setup();
    render(<KnowledgePanel />);

    await user.click(screen.getByRole('button', { name: 'topics/alpha.md 1 backlink' }));

    expect(screen.getByRole('button', { name: 'topics/alpha.md 1 backlink' })).toHaveAttribute(
      'aria-current',
      'page',
    );
    expect(screen.getByRole('heading', { name: 'topics/alpha.md' })).toBeInTheDocument();
    expect(screen.getByRole('article', { name: 'Memory m_alpha' })).toBeInTheDocument();
    expect(screen.queryByRole('article', { name: 'Memory m_beta' })).not.toBeInTheDocument();
  });

  it('labels lexical search, filters loaded text case-insensitively, and clearing restores selection', async () => {
    const user = userEvent.setup();
    render(<KnowledgePanel />);
    await user.click(screen.getByRole('button', { name: 'Unlinked 1' }));

    await user.type(screen.getByRole('searchbox', { name: 'Search memories' }), 'rUsT');
    expect(screen.getByText('Lexical matches in loaded memory text')).toBeInTheDocument();
    expect(screen.getByRole('article', { name: 'Memory m_alpha' })).toBeInTheDocument();
    expect(screen.queryByRole('article', { name: 'Memory m_unlinked' })).not.toBeInTheDocument();

    await user.clear(screen.getByRole('searchbox', { name: 'Search memories' }));
    expect(screen.queryByText('Lexical matches in loaded memory text')).not.toBeInTheDocument();
    expect(screen.getByRole('article', { name: 'Memory m_unlinked' })).toBeInTheDocument();
    expect(screen.queryByRole('article', { name: 'Memory m_alpha' })).not.toBeInTheDocument();
  });

  it('keeps memory views usable when topics fail and retries only topics', async () => {
    const user = userEvent.setup();
    mocks.useKnowledgeData.mockReturnValue({
      ...readyData(),
      topics: { kind: 'error', message: 'topics unreadable' },
    });
    render(<KnowledgePanel />);

    expect(screen.getByRole('article', { name: 'Memory m_alpha' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Unlinked 1' })).toBeInTheDocument();
    expect(screen.getByRole('alert')).toHaveTextContent('topics unreadable');
    expect(screen.getByRole('article', { name: 'Memory m_stale' })).not.toHaveTextContent('missing topic');
    await user.click(screen.getByRole('button', { name: 'Unlinked 1' }));
    expect(screen.getByRole('article', { name: 'Memory m_unlinked' })).toBeInTheDocument();
    expect(screen.queryByRole('article', { name: 'Memory m_alpha' })).not.toBeInTheDocument();
    await user.click(screen.getByRole('button', { name: 'Retry memory topics' }));
    expect(mocks.retryTopics).toHaveBeenCalledOnce();
    expect(mocks.retryMemory).not.toHaveBeenCalled();
  });

  it('keeps topic navigation and content visible when memory entries fail', async () => {
    const user = userEvent.setup();
    mocks.useKnowledgeData.mockReturnValue({
      ...readyData(),
      memory: { kind: 'error', message: 'entries unreadable' },
    });
    render(<KnowledgePanel />);

    const alpha = screen.getByRole('button', { name: 'topics/alpha.md backlinks unavailable' });
    expect(screen.getByText('Alpha topic body')).toBeInTheDocument();
    await user.click(alpha);
    expect(screen.getByRole('alert')).toHaveTextContent('entries unreadable');
    await user.click(screen.getByRole('button', { name: 'Retry memory entries' }));
    expect(mocks.retryMemory).toHaveBeenCalledOnce();
    expect(mocks.retryTopics).not.toHaveBeenCalled();
  });

  it('renders explicit loading and empty states instead of a blank panel', () => {
    mocks.useKnowledgeData.mockReturnValue({
      ...readyData([], []),
      memory: { kind: 'loading' },
      topics: { kind: 'loading' },
    });
    const { rerender } = render(<KnowledgePanel />);
    expect(screen.getAllByRole('status').map((node) => node.textContent)).toEqual(
      expect.arrayContaining(['Loading memory entries…', 'Loading memory topics…']),
    );

    mocks.useKnowledgeData.mockReturnValue(readyData([], []));
    rerender(<KnowledgePanel />);
    expect(screen.getByText('No memory entries yet.')).toBeInTheDocument();
    expect(screen.getByText('No topic files yet.')).toBeInTheDocument();
  });

  it('uses native button keyboard behavior and tracks the selected view', async () => {
    const user = userEvent.setup();
    render(<KnowledgePanel />);
    const beta = screen.getByRole('button', { name: 'topics/beta.md 1 backlink' });

    beta.focus();
    await user.keyboard('{Enter}');

    expect(beta).toHaveAttribute('aria-current', 'page');
    expect(screen.getByRole('button', { name: 'All memories 4' })).not.toHaveAttribute(
      'aria-current',
    );
  });

  it('resets a removed selected topic to All memories after refresh data arrives', async () => {
    const user = userEvent.setup();
    const { rerender } = render(<KnowledgePanel />);
    await user.click(screen.getByRole('button', { name: 'topics/alpha.md 1 backlink' }));

    mocks.useKnowledgeData.mockReturnValue(readyData(fixtureEntries(), [fixtureTopics()[1]!]));
    rerender(<KnowledgePanel />);

    expect(screen.getByRole('button', { name: 'All memories 4' })).toHaveAttribute(
      'aria-current',
      'page',
    );
    expect(screen.queryByRole('heading', { name: 'topics/alpha.md' })).not.toBeInTheDocument();
    expect(screen.getByRole('article', { name: 'Memory m_alpha' })).toBeInTheDocument();
  });

  it.each(['loading', 'error'] as const)(
    'resets a selected topic when topics become %s without hiding ready memory',
    async (topicState) => {
      const user = userEvent.setup();
      const { rerender } = render(<KnowledgePanel />);
      await user.click(screen.getByRole('button', { name: 'topics/alpha.md 1 backlink' }));

      mocks.useKnowledgeData.mockReturnValue({
        ...readyData(),
        topics:
          topicState === 'loading'
            ? { kind: 'loading' }
            : { kind: 'error', message: 'topics unreadable' },
      });
      rerender(<KnowledgePanel />);

      expect(screen.getByRole('button', { name: 'All memories 4' })).toHaveAttribute(
        'aria-current',
        'page',
      );
      expect(screen.getByRole('article', { name: 'Memory m_alpha' })).toBeInTheDocument();
      expect(screen.queryByText('No memories link to this exact topic ref.')).not.toBeInTheDocument();
    },
  );

  it.each(['loading', 'error'] as const)(
    'resets stale-link selection when topics become %s without hiding ready memory',
    async (topicState) => {
      const user = userEvent.setup();
      const { rerender } = render(<KnowledgePanel />);
      await user.click(screen.getByRole('button', { name: 'Stale links 1' }));

      mocks.useKnowledgeData.mockReturnValue({
        ...readyData(),
        topics:
          topicState === 'loading'
            ? { kind: 'loading' }
            : { kind: 'error', message: 'topics unreadable' },
      });
      rerender(<KnowledgePanel />);

      expect(screen.getByRole('button', { name: 'All memories 4' })).toHaveAttribute(
        'aria-current',
        'page',
      );
      expect(screen.getByRole('article', { name: 'Memory m_alpha' })).toBeInTheDocument();
      expect(screen.queryByText('No stale topic links.')).not.toBeInTheDocument();
    },
  );

  it('refreshes both read sources without exposing mutation or context controls', async () => {
    const user = userEvent.setup();
    render(<KnowledgePanel />);
    await user.click(screen.getByRole('button', { name: 'Refresh knowledge' }));

    expect(mocks.refreshAll).toHaveBeenCalledOnce();
    expect(
      screen.queryByRole('button', { name: /use in chat|remember|forget|edit/i }),
    ).not.toBeInTheDocument();
  });

  it('offers exact memory and topic refs and surfaces a full shelf without navigating silently', async () => {
    const user = userEvent.setup();
    const onUseInChat = vi.fn().mockResolvedValue('full');
    render(<KnowledgePanel onUseInChat={onUseInChat} />);

    const useButtons = screen.getAllByRole('button', { name: 'Use in chat' });
    await user.click(useButtons[0]!);
    expect(onUseInChat).toHaveBeenCalledWith({
      kind: 'topicFile',
      name: 'topics/alpha.md',
    });
    expect(screen.getByRole('alert')).toHaveTextContent(
      'Context is full. Remove an item in chat, then try again.',
    );

    await user.click(useButtons.at(-1)!);
    expect(onUseInChat).toHaveBeenLastCalledWith({
      kind: 'memoryEntry',
      entryId: 'm_unlinked',
    });
  });
});
