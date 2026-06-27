import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import { AgentSingleStepPanel } from './AgentSingleStepPanel';
import type { AgentEventEnvelope } from '../../lib/api/agentEvents';
import type { SelectionState } from '../file-tree/FileBrowser';
import type { SelectedModel } from '../model-picker/useSelectedModel';
import type { MlxServersApi } from '../providers/useMlxServers';

/** A "ready" inspector selection for a small UTF-8 file — eligible to attach. */
function readySelection(path: string, bytes = 32): SelectionState {
  return {
    kind: 'ready',
    path,
    content: { content: 'alpha\nbeta\ngamma\n', encoding: 'utf-8', bytes },
  };
}

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

  it('blocks Run and explains when the agent mode is chat', () => {
    const handle = { id: 'srv_1', port: 5005, pid: 42 };
    render(
      <AgentSingleStepPanel selected={mlxModel()} mlxServers={servers(handle)} agentMode="chat" />,
    );
    expect(screen.getByRole('button', { name: 'Run step' })).toBeDisabled();
    expect(
      screen.getByText('Switch Agent mode to Propose diff or higher to run a step.'),
    ).toBeInTheDocument();
  });

  it('blocks Run and explains when no MLX model is selected', () => {
    render(
      <AgentSingleStepPanel selected={null} mlxServers={servers(null)} agentMode="propose-diff" />,
    );
    expect(screen.getByRole('button', { name: 'Run step' })).toBeDisabled();
    expect(screen.getByText('Select a local (MLX) model to run a step.')).toBeInTheDocument();
  });

  it('blocks Run and explains when the selected model has no running server', () => {
    render(
      <AgentSingleStepPanel
        selected={mlxModel()}
        mlxServers={servers(null)}
        agentMode="propose-diff"
      />,
    );
    expect(screen.getByRole('button', { name: 'Run step' })).toBeDisabled();
    expect(screen.getByText('Start the selected model to run a step.')).toBeInTheDocument();
  });

  it('runs a step and renders the real event stream', async () => {
    mocks.runAgentSingleStep.mockResolvedValue({ events: stream() });
    const handle = { id: 'srv_1', port: 5005, pid: 42 };
    render(
      <AgentSingleStepPanel
        selected={mlxModel()}
        mlxServers={servers(handle)}
        agentMode="propose-diff"
      />,
    );

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
    render(
      <AgentSingleStepPanel
        selected={mlxModel()}
        mlxServers={servers(handle)}
        agentMode="propose-diff"
      />,
    );

    await userEvent.type(screen.getByLabelText('Step instruction'), 'do it');
    await userEvent.click(screen.getByRole('button', { name: 'Run step' }));

    expect(await screen.findByRole('alert')).toHaveTextContent('IPC error: ProviderDown');
  });

  it('attaches an eligible inspector file and folds it into the run payload (D99)', async () => {
    mocks.runAgentSingleStep.mockResolvedValue({ events: stream() });
    const handle = { id: 'srv_1', port: 5005, pid: 42 };
    render(
      <AgentSingleStepPanel
        selected={mlxModel()}
        mlxServers={servers(handle)}
        agentMode="propose-diff"
        inspectorSelection={readySelection('src/notes.ts')}
        inspectorLineRange={null}
      />,
    );

    await userEvent.type(screen.getByLabelText('Step instruction'), 'summarize');
    // The shared AttachBar offers a whole-file attach for a UTF-8 file.
    await userEvent.click(screen.getByRole('button', { name: 'Attach current file' }));
    expect(screen.getByText('src/notes.ts')).toBeInTheDocument();

    await userEvent.click(screen.getByRole('button', { name: 'Run step' }));

    await waitFor(() =>
      expect(mocks.runAgentSingleStep).toHaveBeenCalledWith({
        prompt: 'summarize',
        providerId: 'mlx-lm',
        modelId: 'qwen2.5-coder-3b',
        handleId: 'srv_1',
        attachment: { kind: 'projectFile', relPath: 'src/notes.ts' },
      }),
    );
    // One-shot: the chip clears after a successful run.
    await waitFor(() => expect(screen.queryByText('src/notes.ts')).toBeNull());
  });

  it('sends the selection line range when the inspector has one (D99)', async () => {
    mocks.runAgentSingleStep.mockResolvedValue({ events: stream() });
    const handle = { id: 'srv_1', port: 5005, pid: 42 };
    render(
      <AgentSingleStepPanel
        selected={mlxModel()}
        mlxServers={servers(handle)}
        agentMode="propose-diff"
        inspectorSelection={readySelection('src/notes.ts')}
        inspectorLineRange={{ startLine: 2, endLine: 3 }}
      />,
    );

    await userEvent.type(screen.getByLabelText('Step instruction'), 'explain');
    await userEvent.click(screen.getByRole('button', { name: 'Attach selection' }));
    await userEvent.click(screen.getByRole('button', { name: 'Run step' }));

    await waitFor(() =>
      expect(mocks.runAgentSingleStep).toHaveBeenCalledWith({
        prompt: 'explain',
        providerId: 'mlx-lm',
        modelId: 'qwen2.5-coder-3b',
        handleId: 'srv_1',
        attachment: {
          kind: 'projectFile',
          relPath: 'src/notes.ts',
          startLine: 2,
          endLine: 3,
        },
      }),
    );
  });
});
