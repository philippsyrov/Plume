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

const mocks = vi.hoisted(() => ({
  runAgentSingleStep: vi.fn(),
  applyPatch: vi.fn(),
  revertPatch: vi.fn(),
}));
vi.mock('../../lib/api/agent', () => ({ runAgentSingleStep: mocks.runAgentSingleStep }));
vi.mock('../../lib/api/patch', async (importOriginal) => {
  // Keep validatePatch + the type surface real; only stub the two write verbs.
  const real = await importOriginal<typeof import('../../lib/api/patch')>();
  return { ...real, applyPatch: mocks.applyPatch, revertPatch: mocks.revertPatch };
});

/** A small validated diff the backend would hand back as `applicableDiff`. */
const DIFF = '--- a/x.txt\n+++ b/x.txt\n@@ -1 +1 @@\n-a\n+b\n';

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

  // ─── D100: explicit apply / revert ──────────────────────────────────────

  const stepHandle = { id: 'srv_1', port: 5005, pid: 42 };

  it('offers Apply for a validated diff but writes nothing until clicked (D100)', async () => {
    mocks.runAgentSingleStep.mockResolvedValue({ events: stream(), applicableDiff: DIFF });
    render(
      <AgentSingleStepPanel
        selected={mlxModel()}
        mlxServers={servers(stepHandle)}
        agentMode="propose-diff"
      />,
    );
    await userEvent.type(screen.getByLabelText('Step instruction'), 'edit it');
    await userEvent.click(screen.getByRole('button', { name: 'Run step' }));

    // Apply appears for the validated diff…
    expect(await screen.findByRole('button', { name: 'Apply diff' })).toBeInTheDocument();
    // …but the run itself never wrote: no apply before the explicit click.
    expect(mocks.applyPatch).not.toHaveBeenCalled();
  });

  it('offers no Apply when the diff did not validate (D100)', async () => {
    mocks.runAgentSingleStep.mockResolvedValue({ events: stream(), applicableDiff: undefined });
    render(
      <AgentSingleStepPanel
        selected={mlxModel()}
        mlxServers={servers(stepHandle)}
        agentMode="propose-diff"
      />,
    );
    await userEvent.type(screen.getByLabelText('Step instruction'), 'edit it');
    await userEvent.click(screen.getByRole('button', { name: 'Run step' }));

    await waitFor(() => expect(screen.getAllByRole('listitem').length).toBeGreaterThan(0));
    expect(screen.queryByRole('button', { name: 'Apply diff' })).toBeNull();
  });

  it('applies via patch.apply, logs the result, and then reverts (D100)', async () => {
    mocks.runAgentSingleStep.mockResolvedValue({ events: stream(), applicableDiff: DIFF });
    mocks.applyPatch.mockResolvedValue({
      applied: true,
      checkpoint: 'abcd1234ef',
      touched: [{ path: 'x.txt', changeType: 'modify', bytesWritten: 2 }],
    });
    mocks.revertPatch.mockResolvedValue({
      reverted: true,
      restored: [{ path: 'x.txt', changeType: 'modify' }],
    });
    render(
      <AgentSingleStepPanel
        selected={mlxModel()}
        mlxServers={servers(stepHandle)}
        agentMode="propose-diff"
      />,
    );
    await userEvent.type(screen.getByLabelText('Step instruction'), 'edit it');
    await userEvent.click(screen.getByRole('button', { name: 'Run step' }));

    await userEvent.click(await screen.findByRole('button', { name: 'Apply diff' }));
    expect(mocks.applyPatch).toHaveBeenCalledWith({ diff: DIFF });
    // The real apply result is reflected in the event log.
    expect(await screen.findByText(/checkpoint abcd1234/)).toBeInTheDocument();

    // Apply flips to Revert; reverting goes through patch.revert.
    await userEvent.click(await screen.findByRole('button', { name: 'Revert' }));
    expect(mocks.revertPatch).toHaveBeenCalledWith({ checkpoint: 'abcd1234ef' });
    expect(await screen.findByText(/1 file restored/)).toBeInTheDocument();
  });

  it('surfaces an apply failure in the log and keeps Apply available (D100)', async () => {
    mocks.runAgentSingleStep.mockResolvedValue({ events: stream(), applicableDiff: DIFF });
    mocks.applyPatch.mockResolvedValue({
      applied: false,
      reason: 'preImageMismatch',
      details: [{ path: 'x.txt', message: 'pre-image differs' }],
    });
    render(
      <AgentSingleStepPanel
        selected={mlxModel()}
        mlxServers={servers(stepHandle)}
        agentMode="propose-diff"
      />,
    );
    await userEvent.type(screen.getByLabelText('Step instruction'), 'edit it');
    await userEvent.click(screen.getByRole('button', { name: 'Run step' }));

    await userEvent.click(await screen.findByRole('button', { name: 'Apply diff' }));
    expect(
      await screen.findByText(/apply failed \(preImageMismatch\): pre-image differs/),
    ).toBeInTheDocument();
    // Recoverable: Apply stays (no flip to Revert, no terminal lock).
    expect(screen.getByRole('button', { name: 'Apply diff' })).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Revert' })).toBeNull();
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
