import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import { AgentSingleStepPanel } from './AgentSingleStepPanel';
import type { AgentEventEnvelope } from '../../lib/api/agentEvents';
import type { SelectedModel } from '../model-picker/useSelectedModel';
import type { MlxServersApi } from '../providers/useMlxServers';

const mocks = vi.hoisted(() => ({ runAgentSingleStep: vi.fn() }));
vi.mock('../../lib/api/agent', () => ({ runAgentSingleStep: mocks.runAgentSingleStep }));

function mlxModel(modelId = 'qwen2.5-coder-3b'): SelectedModel {
  return { providerId: 'mlx-lm', providerDisplayName: 'Local · MLX', modelId };
}

/** A minimal MlxServersApi whose `handleOf` returns the given handle. */
function servers(handle: { id: string; port: number; pid: number } | null): MlxServersApi {
  return {
    statuses: new Map(),
    statusOf: () => ({ kind: 'idle' }),
    handleOf: () => handle,
    start: vi.fn(),
    stop: vi.fn(),
    clearError: vi.fn(),
  };
}

function stream(): AgentEventEnvelope[] {
  return [
    { seq: 0, tsMs: 1, kind: 'messageChunk', text: '--- a/greet.py' },
    { seq: 1, tsMs: 1, kind: 'toolProposed', callId: 'validate-1', tool: 'read', summary: 'validate the proposed diff' },
    { seq: 2, tsMs: 1, kind: 'toolStarted', callId: 'validate-1', tool: 'read' },
    { seq: 3, tsMs: 1, kind: 'toolFinished', callId: 'validate-1', tool: 'read', summary: 'diff is valid — 1 file, 1 hunk' },
    { seq: 4, tsMs: 1, kind: 'toolProposed', callId: 'apply-1', tool: 'write', summary: 'apply the diff to greet.py' },
    { seq: 5, tsMs: 1, kind: 'approvalRequired', callId: 'apply-1', tool: 'write', prompt: 'Apply this diff to greet.py?' },
    { seq: 6, tsMs: 1, kind: 'paused', reason: 'waiting for approval to apply the proposed diff' },
  ];
}

describe('AgentSingleStepPanel — D96', () => {
  beforeEach(() => vi.clearAllMocks());

  it('blocks Run and explains when no MLX model is selected', () => {
    render(<AgentSingleStepPanel selected={null} mlxServers={servers(null)} />);
    expect(screen.getByRole('button', { name: 'Run step' })).toBeDisabled();
    expect(screen.getByText('Select a local (MLX) model to run a step.')).toBeInTheDocument();
  });

  it('blocks Run and explains when the selected model has no running server', () => {
    render(<AgentSingleStepPanel selected={mlxModel()} mlxServers={servers(null)} />);
    expect(screen.getByRole('button', { name: 'Run step' })).toBeDisabled();
    expect(screen.getByText('Start the selected model to run a step.')).toBeInTheDocument();
  });

  it('runs a step and renders the real event stream', async () => {
    mocks.runAgentSingleStep.mockResolvedValue({ events: stream() });
    const handle = { id: 'srv_1', port: 5005, pid: 42 };
    render(<AgentSingleStepPanel selected={mlxModel()} mlxServers={servers(handle)} />);

    await userEvent.type(screen.getByLabelText('Step instruction'), 'use an f-string');
    await userEvent.click(screen.getByRole('button', { name: 'Run step' }));

    await waitFor(() => expect(screen.getAllByRole('listitem')).toHaveLength(7));
    expect(screen.getByText('needs approval — Apply this diff to greet.py?')).toBeInTheDocument();
    expect(screen.getByText('paused — waiting for approval to apply the proposed diff')).toBeInTheDocument();

    expect(mocks.runAgentSingleStep).toHaveBeenCalledWith({
      prompt: 'use an f-string',
      providerId: 'mlx-lm',
      modelId: 'qwen2.5-coder-3b',
      handleId: 'srv_1',
    });
  });

  it('surfaces an IPC error without crashing', async () => {
    mocks.runAgentSingleStep.mockRejectedValue({ kind: 'ProviderDown', details: { provider: 'mlx-lm', reason: 'x' } });
    const handle = { id: 'srv_1', port: 5005, pid: 42 };
    render(<AgentSingleStepPanel selected={mlxModel()} mlxServers={servers(handle)} />);

    await userEvent.type(screen.getByLabelText('Step instruction'), 'do it');
    await userEvent.click(screen.getByRole('button', { name: 'Run step' }));

    expect(await screen.findByRole('alert')).toHaveTextContent('IPC error: ProviderDown');
  });
});
