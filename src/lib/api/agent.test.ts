import { describe, expect, it, vi } from 'vitest';

import { runAgentDryRun, runAgentSingleStep } from './agent';

const mocks = vi.hoisted(() => ({ invokeIpc: vi.fn() }));
vi.mock('./ipc', () => ({ invokeIpc: mocks.invokeIpc }));

describe('agent.* IPC wrappers', () => {
  it('runAgentDryRun invokes agent_dry_run with an empty payload', async () => {
    mocks.invokeIpc.mockResolvedValue({ events: [] });
    await runAgentDryRun();
    expect(mocks.invokeIpc).toHaveBeenCalledWith('agent_dry_run', {});
  });

  it('runAgentSingleStep forwards the prompt + model + handle', async () => {
    mocks.invokeIpc.mockResolvedValue({ events: [] });
    await runAgentSingleStep({
      prompt: 'use an f-string',
      providerId: 'mlx-lm',
      modelId: 'qwen',
      handleId: 'srv_1',
    });
    expect(mocks.invokeIpc).toHaveBeenCalledWith('agent_single_step', {
      prompt: 'use an f-string',
      providerId: 'mlx-lm',
      modelId: 'qwen',
      handleId: 'srv_1',
    });
  });
});
