import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import type {
  MemoryEntry,
  MemoryIndex,
  MemoryTopicFile,
  MemoryTopics,
  UserMemoryIndex,
} from '../../lib/api/memory';

const mocks = vi.hoisted(() => ({
  refreshAll: vi.fn(),
  retryProjectMemory: vi.fn(),
  retryTopics: vi.fn(),
  retryUserMemory: vi.fn(),
  useLibraryData: vi.fn(),
}));

vi.mock('./useLibraryData', () => ({ useLibraryData: mocks.useLibraryData }));

import { LibraryPanel } from './LibraryPanel';
import { PLUME_CONTEXT_MIME } from '../chat/contextDragPayload';

const limits = { maxEntries: 100, maxBytesPerEntry: 1024, maxBytesTotal: 65_536 };
const topicLimits = { maxCoreBytes: 2048, maxTopicBytes: 8192, maxTopics: 32 };

function userIndex(): UserMemoryIndex {
  return {
    entries: [
      { id: 'm_user_one', createdMs: 3, text: 'Prefers plain English', redactionCount: 0 },
      { id: 'm_user_two', createdMs: 2, text: 'Likes worked examples', redactionCount: 1 },
    ],
    limits,
    totalBytes: 48,
  };
}

function projectEntry(
  id: string,
  text: string,
  links: string[] = [],
): MemoryEntry {
  return { id, createdMs: 1, text, redactionCount: 0, links };
}

function projectIndex(): MemoryIndex {
  return {
    entries: [
      projectEntry('m_project_one', 'Use Rust here', ['topics/alpha.md']),
      projectEntry('m_project_two', 'Old note', ['topics/missing.md']),
    ],
    limits,
    totalBytes: 32,
  };
}

function topic(name: string, content: string): MemoryTopicFile {
  return {
    name,
    kind: 'topic',
    exists: true,
    bytes: content.length,
    truncated: false,
    content,
  };
}

function topicData(): MemoryTopics {
  return {
    core: [],
    topics: [topic('topics/alpha.md', '# Alpha\nExact notes.')],
    topicsTruncated: false,
    limits: topicLimits,
  };
}

function readyData() {
  return {
    userMemory: { kind: 'ready' as const, data: userIndex() },
    projectMemory: { kind: 'ready' as const, data: projectIndex() },
    topics: { kind: 'ready' as const, data: topicData() },
    retryUserMemory: mocks.retryUserMemory,
    retryProjectMemory: mocks.retryProjectMemory,
    retryTopics: mocks.retryTopics,
    refreshAll: mocks.refreshAll,
  };
}

function fakeTransfer(): DataTransfer {
  const values = new Map<string, string>();
  return {
    effectAllowed: 'uninitialized',
    get types() {
      return [...values.keys()];
    },
    getData: (type: string) => values.get(type) ?? '',
    setData: (type: string, value: string) => values.set(type, value),
  } as unknown as DataTransfer;
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}

beforeEach(() => {
  vi.clearAllMocks();
  mocks.useLibraryData.mockReturnValue(readyData());
});

describe('LibraryPanel', () => {
  it('separates About you, This project, and Topics with plain scope labels', () => {
    render(<LibraryPanel projectIdentity="/project/a" />);

    expect(screen.getByRole('heading', { name: 'Library' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'About you 2' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'This project 2' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Topics 1' })).toBeInTheDocument();
    expect(screen.getByText('Stored on this Mac and available without opening a project.'))
      .toBeInTheDocument();
    expect(screen.getByText('Stored only for this trusted project.')).toBeInTheDocument();
  });

  it('keeps search inside the selected source and emits an opaque callback request', async () => {
    const user = userEvent.setup();
    const onUseInChat = vi.fn().mockResolvedValue('added');
    render(<LibraryPanel projectIdentity="/project/a" onUseInChat={onUseInChat} />);

    await user.click(screen.getByRole('button', { name: 'About you 2' }));
    const search = screen.getByRole('searchbox', { name: 'Search About you' });
    await user.type(search, 'examples');

    expect(screen.getByText('Likes worked examples')).toBeInTheDocument();
    expect(screen.queryByText('Prefers plain English')).not.toBeInTheDocument();
    expect(screen.getByText('Searching About you only.')).toBeInTheDocument();

    await user.click(screen.getByRole('button', { name: 'Use in chat' }));
    expect(onUseInChat).toHaveBeenCalledWith({ kind: 'userMemory', entryId: 'm_user_two' });
  });

  it('drags only exact opaque context refs through the private Plume MIME type', async () => {
    const onUseInChat = vi.fn().mockResolvedValue('added');
    const onContextDragActiveChange = vi.fn();
    const user = userEvent.setup();
    render(
      <LibraryPanel
        projectIdentity="/project/a"
        onUseInChat={onUseInChat}
        onContextDragActiveChange={onContextDragActiveChange}
      />,
    );

    await user.click(screen.getByRole('button', { name: 'About you 2' }));
    const userTransfer = fakeTransfer();
    fireEvent.dragStart(screen.getAllByRole('button', { name: 'Use in chat' })[0]!, {
      dataTransfer: userTransfer,
    });
    expect(userTransfer.types).toEqual([PLUME_CONTEXT_MIME]);
    expect(JSON.parse(userTransfer.getData(PLUME_CONTEXT_MIME))).toEqual({
      kind: 'userMemoryEntry',
      entryId: 'm_user_one',
    });
    expect(userTransfer.getData('text/plain')).toBe('');

    await user.click(screen.getByRole('button', { name: 'This project 2' }));
    const projectTransfer = fakeTransfer();
    fireEvent.dragStart(screen.getAllByRole('button', { name: 'Use in chat' })[0]!, {
      dataTransfer: projectTransfer,
    });
    expect(JSON.parse(projectTransfer.getData(PLUME_CONTEXT_MIME))).toEqual({
      kind: 'memoryEntry',
      entryId: 'm_project_one',
    });

    await user.click(screen.getByRole('button', { name: 'Topics 1' }));
    const topicTransfer = fakeTransfer();
    fireEvent.dragStart(screen.getByRole('button', { name: 'Use in chat' }), {
      dataTransfer: topicTransfer,
    });
    expect(JSON.parse(topicTransfer.getData(PLUME_CONTEXT_MIME))).toEqual({
      kind: 'topicFile',
      name: 'topics/alpha.md',
    });
    expect(onContextDragActiveChange).toHaveBeenCalledWith(true);
  });

  it('exposes Connections as exact metadata, never retrieval authority', async () => {
    const user = userEvent.setup();
    render(<LibraryPanel projectIdentity="/project/a" />);

    await user.click(screen.getByRole('button', { name: 'Connections 2' }));

    expect(screen.getByText('Use Rust here')).toBeInTheDocument();
    expect(screen.getByText('Old note')).toBeInTheDocument();
    expect(screen.getByText(/Connections organize information/)).toBeInTheDocument();
    expect(screen.queryByText(/semantic|automatically selected|graph/i)).not.toBeInTheDocument();
  });

  it('re-resolves a selected entry after refresh and clears it when removed', async () => {
    const user = userEvent.setup();
    const view = render(<LibraryPanel projectIdentity="/project/a" />);
    await user.click(screen.getByRole('button', { name: 'About you 2' }));
    await user.click(screen.getByRole('button', { name: 'Prefers plain English' }));
    expect(screen.getByRole('article')).toHaveTextContent('Prefers plain English');

    const refreshed = readyData();
    if (refreshed.userMemory.kind !== 'ready') throw new Error('test fixture');
    refreshed.userMemory.data.entries[0] = {
      ...refreshed.userMemory.data.entries[0]!,
      text: 'Prefers direct explanations',
    };
    mocks.useLibraryData.mockReturnValue(refreshed);
    view.rerender(<LibraryPanel projectIdentity="/project/a" />);
    await waitFor(() => {
      expect(screen.getByRole('article')).toHaveTextContent('Prefers direct explanations');
    });

    const removed = readyData();
    if (removed.userMemory.kind !== 'ready') throw new Error('test fixture');
    removed.userMemory.data.entries = removed.userMemory.data.entries.slice(1);
    mocks.useLibraryData.mockReturnValue(removed);
    view.rerender(<LibraryPanel projectIdentity="/project/a" />);
    await waitFor(() => expect(screen.queryByRole('article')).not.toBeInTheDocument());
    expect(screen.getByText('Select an item to read it.')).toBeInTheDocument();
  });

  it('never paints project A selection or a late A handoff notice under project B', async () => {
    const handoff = deferred<'full'>();
    const user = userEvent.setup();
    const view = render(
      <LibraryPanel
        projectIdentity="/project/a"
        onUseInChat={() => handoff.promise}
      />,
    );
    await user.click(screen.getByRole('button', { name: 'This project 2' }));
    await user.click(screen.getByRole('button', { name: 'Use Rust here' }));
    await user.click(screen.getAllByRole('button', { name: 'Use in chat' })[0]!);
    expect(screen.getByRole('article')).toHaveTextContent('Use Rust here');

    view.rerender(
      <LibraryPanel
        projectIdentity="/project/b"
        onUseInChat={() => handoff.promise}
      />,
    );
    expect(screen.queryByRole('article')).not.toBeInTheDocument();

    handoff.resolve('full');
    await waitFor(() => {
      expect(screen.queryByText(/Chat context is full/)).not.toBeInTheDocument();
    });
  });

  it('shows exact topic backlinks and says connections do not choose chat context', async () => {
    const user = userEvent.setup();
    render(<LibraryPanel projectIdentity="/project/a" />);

    await user.click(screen.getByRole('button', { name: 'Topics 1' }));
    await user.click(screen.getByRole('button', { name: 'Alpha' }));

    const detail = screen.getByRole('article', { name: 'Topic Alpha' });
    expect(detail).toHaveTextContent('Exact notes.');
    expect(detail).toHaveTextContent('Use Rust here');
    expect(detail).toHaveTextContent('topics/alpha.md');
    expect(detail).toHaveTextContent(
      'Connections organize information. They do not choose what goes into chat.',
    );
    expect(detail).not.toHaveTextContent(/semantic|automatically added/i);
  });

  it('keeps healthy sources usable when project memory fails', async () => {
    const user = userEvent.setup();
    mocks.useLibraryData.mockReturnValue({
      ...readyData(),
      projectMemory: { kind: 'error' as const, message: 'project memory unavailable' },
    });
    render(<LibraryPanel projectIdentity="/project/a" />);

    expect(screen.getByRole('button', { name: 'About you 2' })).toBeEnabled();
    expect(screen.getByRole('button', { name: 'Topics 1' })).toBeEnabled();
    await user.click(screen.getByRole('button', { name: /This project unavailable/ }));
    expect(screen.getByRole('alert')).toHaveTextContent('project memory unavailable');
    await user.click(screen.getByRole('button', { name: 'Retry project memory' }));
    expect(mocks.retryProjectMemory).toHaveBeenCalledTimes(1);
  });

  it('works projectless without showing project data as empty user memory', () => {
    mocks.useLibraryData.mockReturnValue({
      ...readyData(),
      projectMemory: { kind: 'unavailable' as const },
      topics: { kind: 'unavailable' as const },
    });
    render(<LibraryPanel projectIdentity={null} />);

    expect(screen.getByRole('button', { name: 'About you 2' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'This project unavailable' })).toBeDisabled();
    expect(screen.getByRole('button', { name: 'Topics unavailable' })).toBeDisabled();
    const overview = screen.getByRole('region', { name: 'Library overview' });
    expect(within(overview).getByText('Open a trusted project to see its memory and topics.'))
      .toBeInTheDocument();
  });
});
