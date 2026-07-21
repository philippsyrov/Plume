import { act, renderHook, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import type {
  ResearchEventEnvelope,
  ResearchEventHandler,
  ResearchLoadArtifactResponse,
  ResearchOwner,
} from '../../lib/api/research';

const mocks = vi.hoisted(() => ({
  mintResearchRunId: vi.fn(),
  subscribeResearchRun: vi.fn(),
  startResearch: vi.fn(),
  cancelResearch: vi.fn(),
  listResearchArtifacts: vi.fn(),
  loadResearchArtifact: vi.fn(),
}));

vi.mock('../../lib/api/research', async (importOriginal) => ({
  ...(await importOriginal<typeof import('../../lib/api/research')>()),
  ...mocks,
}));

import { useResearchRun } from './useResearchRun';

const localOwner: ResearchOwner = { scope: 'local', sessionId: 'session_1' };
const otherOwner: ResearchOwner = { scope: 'local', sessionId: 'session_2' };
const source = {
  kind: 'browserTextEvidence' as const,
  evidenceId: 'be_0123456789abcdef0123456789abcdef',
};
const request = {
  question: 'Write a note',
  providerId: 'mlx-lm',
  modelId: 'qwen-coder-1.5b-mlx-4bit',
  handleId: 'server_1',
  sources: [source],
};

function artifact(status: 'verified' | 'needsReview' = 'verified'): ResearchLoadArtifactResponse {
  return {
    artifact: {
      artifactId: 'ra_1',
      version: 1,
      createdAtMs: 10,
      question: 'Write a note',
      providerId: 'mlx-lm',
      modelId: 'qwen-coder-1.5b-mlx-4bit',
      citationStatus: status,
      outcome: status === 'verified' ? 'complete' : 'needsReview',
    },
    markdown: 'A note.\n\n## Sources\n',
    sources: [],
    logicalTurns: 2,
    providerCalls: 2,
    durationMs: 5,
  };
}

function progress(seq = 0): ResearchEventEnvelope {
  return {
    runId: 'run_1',
    seq,
    tsMs: seq + 1,
    kind: 'progress',
    phase: 'summarizing',
    toolId: 'evidence.source.summarize',
    current: 1,
    total: 2,
    logicalTurns: 1,
    providerCalls: 1,
    summary: 'Reading source 1 of 2',
  };
}

function terminal(
  status: 'complete' | 'needsReview' | 'stopped' | 'failed',
  seq = 1,
): ResearchEventEnvelope {
  return {
    runId: 'run_1',
    seq,
    tsMs: seq + 1,
    kind: 'terminal',
    status,
    artifactId: status === 'complete' || status === 'needsReview' ? 'ra_1' : null,
    citationStatus:
      status === 'complete' ? 'verified' : status === 'needsReview' ? 'needsReview' : null,
    diagnostic: status === 'failed' ? 'provider failed' : null,
  };
}

describe('useResearchRun', () => {
  let handler: ResearchEventHandler;
  let unlisten: ReturnType<typeof vi.fn>;

  beforeEach(() => {
    vi.clearAllMocks();
    unlisten = vi.fn();
    mocks.mintResearchRunId.mockReturnValue('run_1');
    mocks.subscribeResearchRun.mockImplementation(async (_runId, next) => {
      handler = next;
      return unlisten;
    });
    mocks.startResearch.mockResolvedValue({
      runId: 'run_1',
      providerId: request.providerId,
      modelId: request.modelId,
    });
    mocks.cancelResearch.mockResolvedValue({ cancelled: true });
    mocks.listResearchArtifacts.mockResolvedValue({ artifacts: [] });
    mocks.loadResearchArtifact.mockResolvedValue(artifact());
  });

  it('subscribes before start and sends the exact owner, model, handle, and ordered sources', async () => {
    const order: string[] = [];
    mocks.subscribeResearchRun.mockImplementation(async (_runId, next) => {
      order.push('subscribe');
      handler = next;
      return unlisten;
    });
    mocks.startResearch.mockImplementation(async () => {
      order.push('start');
      return { runId: 'run_1', providerId: request.providerId, modelId: request.modelId };
    });
    const { result } = renderHook(() => useResearchRun(localOwner));

    await act(async () => {
      expect(await result.current.start(request)).toBe('started');
    });

    expect(order).toEqual(['subscribe', 'start']);
    expect(mocks.startResearch).toHaveBeenCalledWith({
      runId: 'run_1',
      owner: localOwner,
      ...request,
    });
    expect(result.current.status).toBe('running');
  });

  it('drops duplicate events and fails closed on a sequence gap', async () => {
    const { result } = renderHook(() => useResearchRun(localOwner));
    await act(async () => void (await result.current.start(request)));

    act(() => handler(progress(0)));
    act(() => handler(progress(0)));
    expect(result.current.steps).toHaveLength(1);
    act(() => handler(progress(2)));

    expect(result.current.status).toBe('error');
    expect(result.current.error).toContain('event sequence');
    expect(mocks.cancelResearch).toHaveBeenCalledWith({ runId: 'run_1' });
    act(() => handler(terminal('complete', 3)));
    expect(result.current.status).toBe('error');
  });

  it('accepts exactly one review-needed terminal as a usable non-error artifact', async () => {
    mocks.loadResearchArtifact.mockResolvedValue(artifact('needsReview'));
    const { result } = renderHook(() => useResearchRun(localOwner));
    await act(async () => void (await result.current.start(request)));
    act(() => handler(progress(0)));
    await act(async () => handler(terminal('needsReview', 1)));

    await waitFor(() => expect(result.current.artifact?.artifact.citationStatus).toBe('needsReview'));
    expect(result.current.status).toBe('needsReview');
    expect(result.current.error).toBeNull();
    act(() => handler(terminal('failed', 2)));
    expect(result.current.status).toBe('needsReview');
    expect(mocks.loadResearchArtifact).toHaveBeenCalledTimes(1);
  });

  it('cancels and fences late events when the owning session changes', async () => {
    const { result, rerender } = renderHook(
      ({ owner }: { owner: ResearchOwner }) => useResearchRun(owner),
      { initialProps: { owner: localOwner } },
    );
    await act(async () => void (await result.current.start(request)));
    const staleHandler = handler;

    rerender({ owner: otherOwner });
    await waitFor(() => expect(mocks.cancelResearch).toHaveBeenCalledWith({ runId: 'run_1' }));
    expect(unlisten).toHaveBeenCalledOnce();
    act(() => staleHandler(progress(0)));
    expect(result.current.steps).toEqual([]);
    expect(mocks.listResearchArtifacts).not.toHaveBeenCalled();
  });

  it('cancels and detaches on unmount', async () => {
    const { result, unmount } = renderHook(() => useResearchRun(localOwner));
    await act(async () => void (await result.current.start(request)));
    unmount();

    expect(mocks.cancelResearch).toHaveBeenCalledWith({ runId: 'run_1' });
    expect(unlisten).toHaveBeenCalledOnce();
  });

  it('does not resurrect an artifact outside persisted transcript chronology', () => {
    const summary = artifact().artifact;
    mocks.listResearchArtifacts.mockResolvedValue({ artifacts: [summary] });
    mocks.loadResearchArtifact.mockResolvedValue(artifact());

    const { result } = renderHook(() => useResearchRun(localOwner));

    expect(result.current.artifact).toBeNull();
    expect(mocks.listResearchArtifacts).not.toHaveBeenCalled();
    expect(mocks.loadResearchArtifact).not.toHaveBeenCalled();
  });
});
