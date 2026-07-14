import { act, renderHook, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import type { MemoryIndex, MemoryTopics, UserMemoryIndex } from '../../lib/api/memory';

const mocks = vi.hoisted(() => ({
  getMemoryIndex: vi.fn(),
  getMemoryTopics: vi.fn(),
  getUserMemoryIndex: vi.fn(),
}));

vi.mock('../../lib/api/memory', async (importOriginal) => ({
  ...(await importOriginal<typeof import('../../lib/api/memory')>()),
  getMemoryIndex: mocks.getMemoryIndex,
  getMemoryTopics: mocks.getMemoryTopics,
  getUserMemoryIndex: mocks.getUserMemoryIndex,
}));

import { useLibraryData } from './useLibraryData';

const limits = { maxEntries: 100, maxBytesPerEntry: 1024, maxBytesTotal: 65_536 };
const topicLimits = { maxCoreBytes: 2048, maxTopicBytes: 8192, maxTopics: 32 };

function userIndex(text = 'Prefers plain English'): UserMemoryIndex {
  return {
    entries: [{ id: 'm_user', createdMs: 1, text, redactionCount: 0 }],
    limits,
    totalBytes: text.length,
  };
}

function projectIndex(text = 'Use Rust in this project'): MemoryIndex {
  return {
    entries: [{ id: 'm_project', createdMs: 2, text, redactionCount: 0, links: [] }],
    limits,
    totalBytes: text.length,
  };
}

function topics(content = 'Alpha'): MemoryTopics {
  return {
    core: [],
    topics: [{
      name: 'topics/alpha.md',
      kind: 'topic',
      exists: true,
      bytes: content.length,
      truncated: false,
      content,
    }],
    topicsTruncated: false,
    limits: topicLimits,
  };
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((onResolve, onReject) => {
    resolve = onResolve;
    reject = onReject;
  });
  return { promise, resolve, reject };
}

beforeEach(() => {
  vi.clearAllMocks();
  mocks.getUserMemoryIndex.mockResolvedValue(userIndex());
  mocks.getMemoryIndex.mockResolvedValue(projectIndex());
  mocks.getMemoryTopics.mockResolvedValue(topics());
});

describe('useLibraryData', () => {
  it('loads app-private memory without a project and keeps project sources unavailable', async () => {
    const { result } = renderHook(() => useLibraryData({ projectIdentity: null }));

    await waitFor(() => expect(result.current.userMemory.kind).toBe('ready'));

    expect(result.current.projectMemory).toEqual({ kind: 'unavailable' });
    expect(result.current.topics).toEqual({ kind: 'unavailable' });
    expect(mocks.getUserMemoryIndex).toHaveBeenCalledTimes(1);
    expect(mocks.getMemoryIndex).not.toHaveBeenCalled();
    expect(mocks.getMemoryTopics).not.toHaveBeenCalled();
  });

  it('keeps source failures independent and retries only the failed source', async () => {
    mocks.getMemoryIndex.mockRejectedValueOnce(new Error('project memory offline'));
    const { result } = renderHook(() => useLibraryData({ projectIdentity: '/project/a' }));

    await waitFor(() => expect(result.current.projectMemory.kind).toBe('error'));
    expect(result.current.userMemory.kind).toBe('ready');
    expect(result.current.topics.kind).toBe('ready');

    act(() => result.current.retryProjectMemory());
    await waitFor(() => expect(result.current.projectMemory.kind).toBe('ready'));

    expect(mocks.getMemoryIndex).toHaveBeenCalledTimes(2);
    expect(mocks.getMemoryTopics).toHaveBeenCalledTimes(1);
    expect(mocks.getUserMemoryIndex).toHaveBeenCalledTimes(1);
  });

  it('clears project A synchronously and ignores its late response after switching to B', async () => {
    const aMemory = deferred<MemoryIndex>();
    const aTopics = deferred<MemoryTopics>();
    const bMemory = deferred<MemoryIndex>();
    const bTopics = deferred<MemoryTopics>();
    mocks.getMemoryIndex
      .mockReturnValueOnce(aMemory.promise)
      .mockReturnValueOnce(bMemory.promise);
    mocks.getMemoryTopics
      .mockReturnValueOnce(aTopics.promise)
      .mockReturnValueOnce(bTopics.promise);

    const { result, rerender } = renderHook(
      ({ projectIdentity }) => useLibraryData({ projectIdentity }),
      { initialProps: { projectIdentity: '/project/a' as string | null } },
    );

    rerender({ projectIdentity: '/project/b' });
    expect(result.current.projectMemory).toEqual({ kind: 'loading' });
    expect(result.current.topics).toEqual({ kind: 'loading' });

    act(() => {
      bMemory.resolve(projectIndex('Project B'));
      bTopics.resolve(topics('Topic B'));
    });
    await waitFor(() => {
      expect(result.current.projectMemory.kind === 'ready'
        ? result.current.projectMemory.data.entries[0]?.text
        : null).toBe('Project B');
    });

    act(() => {
      aMemory.resolve(projectIndex('Project A'));
      aTopics.resolve(topics('Topic A'));
    });
    await act(async () => Promise.resolve());

    expect(result.current.projectMemory.kind === 'ready'
      ? result.current.projectMemory.data.entries[0]?.text
      : null).toBe('Project B');
    expect(result.current.topics.kind === 'ready'
      ? result.current.topics.data.topics[0]?.content
      : null).toBe('Topic B');
  });

  it('never renders project A data under project B identity, even before effects run', async () => {
    const bMemory = deferred<MemoryIndex>();
    const bTopics = deferred<MemoryTopics>();
    mocks.getMemoryIndex
      .mockResolvedValueOnce(projectIndex('Project A'))
      .mockReturnValueOnce(bMemory.promise);
    mocks.getMemoryTopics
      .mockResolvedValueOnce(topics('Topic A'))
      .mockReturnValueOnce(bTopics.promise);
    const renders: Array<{ identity: string; text: string | null }> = [];
    const { result, rerender } = renderHook(
      ({ projectIdentity }) => {
        const data = useLibraryData({ projectIdentity });
        renders.push({
          identity: projectIdentity,
          text: data.projectMemory.kind === 'ready'
            ? data.projectMemory.data.entries[0]?.text ?? null
            : null,
        });
        return data;
      },
      { initialProps: { projectIdentity: '/project/a' } },
    );
    await waitFor(() => expect(result.current.projectMemory.kind).toBe('ready'));

    renders.length = 0;
    rerender({ projectIdentity: '/project/b' });

    expect(renders).not.toContainEqual({ identity: '/project/b', text: 'Project A' });
    expect(result.current.projectMemory).toEqual({ kind: 'loading' });
  });

  it('ignores every late response after unmount', async () => {
    const user = deferred<UserMemoryIndex>();
    const project = deferred<MemoryIndex>();
    const topicData = deferred<MemoryTopics>();
    mocks.getUserMemoryIndex.mockReturnValue(user.promise);
    mocks.getMemoryIndex.mockReturnValue(project.promise);
    mocks.getMemoryTopics.mockReturnValue(topicData.promise);
    const { unmount } = renderHook(() => useLibraryData({ projectIdentity: '/project/a' }));

    unmount();
    act(() => {
      user.resolve(userIndex());
      project.resolve(projectIndex());
      topicData.resolve(topics());
    });

    await act(async () => Promise.resolve());
  });
});
