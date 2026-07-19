import { beforeEach, describe, expect, it, vi } from 'vitest';

const mocks = vi.hoisted(() => ({
  invokeIpc: vi.fn(),
  listen: vi.fn(),
}));

vi.mock('./ipc', () => ({ invokeIpc: mocks.invokeIpc }));
vi.mock('@tauri-apps/api/event', () => ({ listen: mocks.listen }));

import {
  cancelResearch,
  exportResearchArtifact,
  listResearchArtifacts,
  loadResearchArtifact,
  startResearch,
  subscribeResearchRun,
  type ResearchEventEnvelope,
} from './research';

describe('research API', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.invokeIpc.mockResolvedValue({});
    mocks.listen.mockResolvedValue(vi.fn());
  });

  it('uses exact IPC verbs and keeps owner, model, handle, and source identities opaque', async () => {
    const owner = { scope: 'project' as const, sessionId: 'session_1' };
    const source = {
      kind: 'browserTextEvidence' as const,
      evidenceId: 'be_0123456789abcdef0123456789abcdef',
    };
    const start = {
      runId: 'run_1',
      owner,
      question: 'Synthesize this evidence',
      providerId: 'mlx-lm',
      modelId: 'qwen-coder-1.5b-mlx-4bit',
      handleId: 'server_1',
      sources: [source],
    };

    await startResearch(start);
    await cancelResearch({ runId: 'run_1' });
    await listResearchArtifacts({ owner });
    await loadResearchArtifact({ owner, artifactId: 'ra_1', version: 2 });
    await exportResearchArtifact({ owner, artifactId: 'ra_1', version: 2 });

    expect(mocks.invokeIpc.mock.calls).toEqual([
      ['research_start', start],
      ['research_cancel', { runId: 'run_1' }],
      ['research_list_artifacts', { owner }],
      ['research_load_artifact', { owner, artifactId: 'ra_1', version: 2 }],
      ['research_export_artifact', { owner, artifactId: 'ra_1', version: 2 }],
    ]);
  });

  it('subscribes to one channel and filters envelopes by the client-minted run id', async () => {
    let listener!: (event: { payload: ResearchEventEnvelope }) => void;
    const unlisten = vi.fn();
    mocks.listen.mockImplementation(async (_channel, next) => {
      listener = next;
      return unlisten;
    });
    const onEvent = vi.fn();

    const detach = await subscribeResearchRun('run_1', onEvent);
    const terminal: ResearchEventEnvelope = {
      runId: 'run_1',
      seq: 0,
      tsMs: 1,
      kind: 'terminal',
      status: 'stopped',
      artifactId: null,
      citationStatus: null,
      diagnostic: null,
    };
    listener({ payload: { ...terminal, runId: 'other' } });
    listener({ payload: terminal });
    detach();

    expect(mocks.listen).toHaveBeenCalledWith('research/event', expect.any(Function));
    expect(onEvent).toHaveBeenCalledTimes(1);
    expect(onEvent).toHaveBeenCalledWith(terminal);
    expect(unlisten).toHaveBeenCalledOnce();
  });
});
