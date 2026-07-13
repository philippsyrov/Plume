import { act, renderHook, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import type { MemoryIndex, MemoryTopics } from '../../lib/api/memory';
import {
  __resetMemoryRevisionForTests,
  bumpMemoryRevision,
} from '../memory/memoryRevision';
import { useKnowledgeData, type KnowledgeData } from './useKnowledgeData';

const mocks = vi.hoisted(() => ({
  getMemoryIndex: vi.fn(),
  getMemoryTopics: vi.fn(),
}));

vi.mock('../../lib/api/memory', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../../lib/api/memory')>();
  return {
    ...actual,
    getMemoryIndex: mocks.getMemoryIndex,
    getMemoryTopics: mocks.getMemoryTopics,
  };
});

describe('useKnowledgeData', () => {
  beforeEach(() => {
    __resetMemoryRevisionForTests();
    vi.clearAllMocks();
    mocks.getMemoryIndex.mockResolvedValue(indexFixture());
    mocks.getMemoryTopics.mockResolvedValue(topicsFixture());
  });

  it('keeps topics usable when memory fails and retries only memory', async () => {
    mocks.getMemoryIndex.mockRejectedValueOnce(new Error('entries unreadable'));
    const { result } = renderHook(() => useKnowledgeData());

    await waitFor(() => expect(result.current.memory.kind).toBe('error'));
    await waitFor(() => expect(result.current.topics.kind).toBe('ready'));
    expect(errorMessage(result.current.memory)).toBe('entries unreadable');

    mocks.getMemoryIndex.mockResolvedValueOnce(indexFixture('recovered'));
    act(() => result.current.retryMemory());

    await waitFor(() => expect(readyMemory(result.current).entries[0]?.id).toBe('recovered'));
    expect(mocks.getMemoryIndex).toHaveBeenCalledTimes(2);
    expect(mocks.getMemoryTopics).toHaveBeenCalledTimes(1);
  });

  it('keeps memory usable when topics fail and retries only topics', async () => {
    mocks.getMemoryTopics.mockRejectedValueOnce(new Error('topics unreadable'));
    const { result } = renderHook(() => useKnowledgeData());

    await waitFor(() => expect(result.current.topics.kind).toBe('error'));
    await waitFor(() => expect(result.current.memory.kind).toBe('ready'));
    expect(errorMessage(result.current.topics)).toBe('topics unreadable');

    mocks.getMemoryTopics.mockResolvedValueOnce(topicsFixture('topics/recovered.md'));
    act(() => result.current.retryTopics());

    await waitFor(() =>
      expect(readyTopics(result.current).topics[0]?.name).toBe('topics/recovered.md'),
    );
    expect(mocks.getMemoryIndex).toHaveBeenCalledTimes(1);
    expect(mocks.getMemoryTopics).toHaveBeenCalledTimes(2);
  });

  it('ignores an older memory response after Retry resolves', async () => {
    const first = deferred<MemoryIndex>();
    const second = deferred<MemoryIndex>();
    mocks.getMemoryIndex.mockReturnValueOnce(first.promise).mockReturnValueOnce(second.promise);
    const { result } = renderHook(() => useKnowledgeData());

    act(() => result.current.retryMemory());
    second.resolve(indexFixture('new'));
    await waitFor(() => expect(readyMemory(result.current).entries[0]?.id).toBe('new'));

    first.resolve(indexFixture('old'));
    await act(async () => Promise.resolve());
    expect(readyMemory(result.current).entries[0]?.id).toBe('new');
  });

  it('ignores an older topic response after Retry resolves', async () => {
    const first = deferred<MemoryTopics>();
    const second = deferred<MemoryTopics>();
    mocks.getMemoryTopics.mockReturnValueOnce(first.promise).mockReturnValueOnce(second.promise);
    const { result } = renderHook(() => useKnowledgeData());

    act(() => result.current.retryTopics());
    second.resolve(topicsFixture('topics/new.md'));
    await waitFor(() => expect(readyTopics(result.current).topics[0]?.name).toBe('topics/new.md'));

    first.resolve(topicsFixture('topics/old.md'));
    await act(async () => Promise.resolve());
    expect(readyTopics(result.current).topics[0]?.name).toBe('topics/new.md');
  });

  it("keeps a new project's state isolated from the previous project's late responses", async () => {
    const oldMemory = deferred<MemoryIndex>();
    const oldTopics = deferred<MemoryTopics>();
    mocks.getMemoryIndex.mockReturnValueOnce(oldMemory.promise);
    mocks.getMemoryTopics.mockReturnValueOnce(oldTopics.promise);
    const firstProject = renderHook(() => useKnowledgeData());
    firstProject.unmount();

    mocks.getMemoryIndex.mockResolvedValueOnce(indexFixture('new-project'));
    mocks.getMemoryTopics.mockResolvedValueOnce(topicsFixture('topics/new-project.md'));
    const secondProject = renderHook(() => useKnowledgeData());
    await waitFor(() =>
      expect(readyMemory(secondProject.result.current).entries[0]?.id).toBe('new-project'),
    );
    await waitFor(() =>
      expect(readyTopics(secondProject.result.current).topics[0]?.name).toBe(
        'topics/new-project.md',
      ),
    );

    oldMemory.resolve(indexFixture('old-project'));
    oldTopics.resolve(topicsFixture('topics/old-project.md'));
    await act(async () => Promise.resolve());
    expect(readyMemory(secondProject.result.current).entries[0]?.id).toBe('new-project');
    expect(readyTopics(secondProject.result.current).topics[0]?.name).toBe(
      'topics/new-project.md',
    );
  });

  it('loads both sources after the StrictMode effect replay', async () => {
    const memory = deferred<MemoryIndex>();
    const topics = deferred<MemoryTopics>();
    mocks.getMemoryIndex.mockReturnValue(memory.promise);
    mocks.getMemoryTopics.mockReturnValue(topics.promise);
    const { result } = renderHook(() => useKnowledgeData(), { reactStrictMode: true });

    memory.resolve(indexFixture('strict-memory'));
    topics.resolve(topicsFixture('topics/strict.md'));

    await waitFor(() =>
      expect(readyMemory(result.current).entries[0]?.id).toBe('strict-memory'),
    );
    await waitFor(() =>
      expect(readyTopics(result.current).topics[0]?.name).toBe('topics/strict.md'),
    );
  });

  it('refreshAll starts one fresh request for each source', async () => {
    const { result } = renderHook(() => useKnowledgeData());
    await waitFor(() => expect(result.current.memory.kind).toBe('ready'));
    await waitFor(() => expect(result.current.topics.kind).toBe('ready'));

    mocks.getMemoryIndex.mockResolvedValueOnce(indexFixture('refreshed'));
    mocks.getMemoryTopics.mockResolvedValueOnce(topicsFixture('topics/refreshed.md'));
    act(() => result.current.refreshAll());

    await waitFor(() => expect(readyMemory(result.current).entries[0]?.id).toBe('refreshed'));
    await waitFor(() =>
      expect(readyTopics(result.current).topics[0]?.name).toBe('topics/refreshed.md'),
    );
    expect(mocks.getMemoryIndex).toHaveBeenCalledTimes(2);
    expect(mocks.getMemoryTopics).toHaveBeenCalledTimes(2);
  });

  it('shows source-specific trust copy for NeedsApproval', async () => {
    mocks.getMemoryIndex.mockRejectedValueOnce({ kind: 'NeedsApproval' });
    mocks.getMemoryTopics.mockRejectedValueOnce({ kind: 'NeedsApproval' });
    const { result } = renderHook(() => useKnowledgeData());

    await waitFor(() => expect(result.current.memory.kind).toBe('error'));
    await waitFor(() => expect(result.current.topics.kind).toBe('error'));
    expect(errorMessage(result.current.memory)).toBe('Trust the project to read memory entries.');
    expect(errorMessage(result.current.topics)).toBe('Trust the project to read memory topics.');
  });

  it('refreshes both sources after one memory revision bump without remounting', async () => {
    const { result } = renderHook(() => useKnowledgeData());
    await waitFor(() => expect(result.current.memory.kind).toBe('ready'));
    await waitFor(() => expect(result.current.topics.kind).toBe('ready'));

    mocks.getMemoryIndex.mockResolvedValueOnce(indexFixture('revision'));
    mocks.getMemoryTopics.mockResolvedValueOnce(topicsFixture('topics/revision.md'));
    act(() => bumpMemoryRevision());

    await waitFor(() => expect(readyMemory(result.current).entries[0]?.id).toBe('revision'));
    await waitFor(() =>
      expect(readyTopics(result.current).topics[0]?.name).toBe('topics/revision.md'),
    );
    expect(mocks.getMemoryIndex).toHaveBeenCalledTimes(2);
    expect(mocks.getMemoryTopics).toHaveBeenCalledTimes(2);
  });
});

function indexFixture(id = 'memory'): MemoryIndex {
  return {
    entries: [{ id, createdMs: 1, text: id, redactionCount: 0, links: [] }],
    limits: { maxEntries: 100, maxBytesPerEntry: 1024, maxBytesTotal: 65536 },
    totalBytes: id.length,
  };
}

function topicsFixture(name = 'topics/alpha.md'): MemoryTopics {
  return {
    core: [],
    topics: [
      { name, kind: 'topic', exists: true, bytes: name.length, truncated: false, content: name },
    ],
    topicsTruncated: false,
    limits: { maxCoreBytes: 32768, maxTopicBytes: 32768, maxTopics: 50 },
  };
}

function readyMemory(data: KnowledgeData): MemoryIndex {
  if (data.memory.kind !== 'ready') throw new Error('memory is not ready');
  return data.memory.data;
}

function readyTopics(data: KnowledgeData): MemoryTopics {
  if (data.topics.kind !== 'ready') throw new Error('topics are not ready');
  return data.topics.data;
}

function errorMessage(state: KnowledgeData['memory'] | KnowledgeData['topics']): string {
  if (state.kind !== 'error') throw new Error('source is not in error');
  return state.message;
}

function deferred<T>(): {
  promise: Promise<T>;
  resolve: (value: T) => void;
} {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}
