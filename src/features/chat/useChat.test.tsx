import { act, renderHook } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import type {
  ChatDoneEvent,
  ChatSendStartedResponse,
  ChatStreamHandlers,
} from '../../lib/api/chat';
import { useChat } from './useChat';

const mocks = vi.hoisted(() => ({
  mintStreamId: vi.fn(),
  subscribeChatStream: vi.fn(),
  startChatStream: vi.fn(),
  cancelChatStream: vi.fn(),
}));

vi.mock('../../lib/api/chat', async (importOriginal) => ({
  ...(await importOriginal<typeof import('../../lib/api/chat')>()),
  ...mocks,
}));

const source = { kind: 'memoryEntry' as const, entryId: 'm_0123456789abcdef0123456789abcdef' };
const manifest = {
  kind: 'memoryEntry' as const,
  entryId: source.entryId,
  createdAtMs: 7,
  bytes: 12,
  preview: 'remember this',
};

function acceptedResponse(): ChatSendStartedResponse {
  return {
    streamId: 'stream-1',
    providerId: 'ollama',
    modelId: 'qwen',
    instructionsIncluded: false,
    memory: null,
    topics: null,
    contextSources: [manifest],
  };
}

describe('useChat explicit context', () => {
  let handlers: ChatStreamHandlers;

  beforeEach(() => {
    vi.clearAllMocks();
    mocks.mintStreamId.mockReturnValue('stream-1');
    mocks.subscribeChatStream.mockImplementation(async (_id, nextHandlers) => {
      handlers = nextHandlers;
      return vi.fn();
    });
    mocks.startChatStream.mockResolvedValue(acceptedResponse());
    mocks.cancelChatStream.mockResolvedValue(undefined);
  });

  it('normalizes whole-file null ranges restored from the session wire', () => {
    const { result } = renderHook(() => useChat());
    const wireSource = {
      kind: 'projectFile',
      relPath: 'README.md',
      startLine: null,
      endLine: null,
    } as unknown as import('../../lib/api/chat').ContextSourceRef;

    act(() => result.current.restore([], [wireSource]));

    expect(result.current.contextSources).toEqual([
      { kind: 'projectFile', relPath: 'README.md' },
    ]);
  });

  it('keeps the shelf sticky, locks it during send, and stamps the exact accepted manifest', async () => {
    let resolveStart!: (response: ChatSendStartedResponse) => void;
    mocks.startChatStream.mockReturnValue(
      new Promise((resolve) => {
        resolveStart = resolve;
      }),
    );
    const { result } = renderHook(() => useChat());

    act(() => expect(result.current.addContextSource(source)).toBe('added'));
    expect(result.current.contextSources).toEqual([source]);

    let sendPromise!: Promise<string>;
    await act(async () => {
      sendPromise = result.current.send('ollama', 'qwen', 'hello');
      await Promise.resolve();
    });
    expect(mocks.startChatStream).toHaveBeenCalledWith(
      expect.objectContaining({ contextSources: [source] }),
    );
    expect(result.current.addContextSource({ kind: 'topicFile', name: 'topics/x.md' }))
      .toBe('unavailable');
    expect(result.current.removeContextSource(source)).toBe(false);

    await act(async () => {
      resolveStart(acceptedResponse());
      expect(await sendPromise).toBe('accepted');
    });
    expect(result.current.contextSources).toEqual([source]);
    expect(result.current.entries[0]).toMatchObject({
      kind: 'message',
      contextSources: [manifest],
    });
    expect(result.current.entries[0]).not.toHaveProperty('pendingContextStreamId');
  });

  it('handles a terminal event before acceptance without losing the user manifest', async () => {
    let resolveStart!: (response: ChatSendStartedResponse) => void;
    mocks.startChatStream.mockReturnValue(
      new Promise((resolve) => {
        resolveStart = resolve;
      }),
    );
    const { result } = renderHook(() => useChat());
    act(() => void result.current.addContextSource(source));

    let sendPromise!: Promise<string>;
    await act(async () => {
      sendPromise = result.current.send('ollama', 'qwen', 'fast');
      await Promise.resolve();
    });
    const done: ChatDoneEvent = {
      id: 'stream-1',
      seq: 0,
      finish: 'stop',
      modelId: 'qwen',
      durationMs: 1,
      error: null,
      stats: null,
    };
    act(() => handlers.onDone(done));
    expect(result.current.entries[1]).toMatchObject({
      kind: 'message',
      message: { role: 'assistant' },
    });

    await act(async () => {
      resolveStart(acceptedResponse());
      await sendPromise;
    });
    expect(result.current.entries[0]).toMatchObject({ contextSources: [manifest] });
    expect(result.current.entries[1]).toMatchObject({ kind: 'message' });
  });

  it('clears the pending guard when listener setup rejects', async () => {
    mocks.subscribeChatStream.mockRejectedValueOnce(new Error('listen failed'));
    const { result } = renderHook(() => useChat());
    act(() => void result.current.addContextSource(source));

    await act(async () => {
      expect(await result.current.send('ollama', 'qwen', 'hello')).toBe('rejected');
    });
    expect(mocks.startChatStream).not.toHaveBeenCalled();
    expect(result.current.entries[0]).not.toHaveProperty('pendingContextStreamId');
    expect(result.current.entries[1]).toMatchObject({ kind: 'error' });
    expect(result.current.contextSources).toEqual([source]);
  });

  it('sends a casual-chat Browser shelf only with its exact local owner', async () => {
    const browserSource = {
      kind: 'browserTextEvidence' as const,
      evidenceId: 'be_0123456789abcdef0123456789abcdef',
    };
    const { result } = renderHook(() => useChat());
    act(() => void result.current.addContextSource(browserSource));

    await act(async () => {
      await result.current.send('ollama', 'qwen', 'use this page', {
        includeProjectContext: false,
        contextOwner: {
          scope: 'local',
          sessionId: 's_0123456789abcdef0123456789abcdef',
        },
      });
    });

    expect(mocks.startChatStream).toHaveBeenCalledWith(
      expect.objectContaining({
        includeProjectContext: false,
        contextSources: [browserSource],
        contextOwner: {
          scope: 'local',
          sessionId: 's_0123456789abcdef0123456789abcdef',
        },
      }),
    );
  });
});
