import { render, screen, waitFor, within } from '@testing-library/react';
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
/** A second, distinguishable diff (different file + lines) for run-history
 *  tests, so "which run am I looking at" is observable from the rendered diff. */
const DIFF2 = '--- a/y.txt\n+++ b/y.txt\n@@ -1 +1 @@\n-c\n+d\n';

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

  it('drops the prior diff when a new run starts, and keeps it gone if that run fails (D100)', async () => {
    // First run yields a validated diff; the second run never resolves until
    // we fail it — letting us observe the in-flight window deterministically.
    let failSecond: (reason?: unknown) => void = () => {};
    mocks.runAgentSingleStep
      .mockResolvedValueOnce({ events: stream(), applicableDiff: DIFF })
      .mockReturnValueOnce(
        new Promise((_resolve, reject) => {
          failSecond = reject;
        }),
      );
    render(
      <AgentSingleStepPanel
        selected={mlxModel()}
        mlxServers={servers(stepHandle)}
        agentMode="propose-diff"
      />,
    );
    await userEvent.type(screen.getByLabelText('Step instruction'), 'edit it');
    await userEvent.click(screen.getByRole('button', { name: 'Run step' }));
    expect(await screen.findByRole('button', { name: 'Apply diff' })).toBeInTheDocument();

    // Start a second run; while it is in flight the prior run's Apply is gone —
    // a write now would lie about which run it belongs to.
    await userEvent.click(screen.getByRole('button', { name: 'Run step' }));
    expect(screen.queryByRole('button', { name: 'Apply diff' })).toBeNull();
    expect(screen.getByRole('button', { name: 'Running…' })).toBeDisabled();

    // Fail that run — the stale Apply must NOT come back on error.
    failSecond({ kind: 'ProviderDown', details: { provider: 'mlx-lm', reason: 'x' } });
    expect(await screen.findByRole('alert')).toHaveTextContent('IPC error: ProviderDown');
    expect(screen.queryByRole('button', { name: 'Apply diff' })).toBeNull();
    expect(mocks.applyPatch).not.toHaveBeenCalled();
  });

  // ─── D101: diff preview + changed-files summary ─────────────────────────

  it('renders the proposed diff body and a changed-files summary for a valid diff (D101)', async () => {
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

    // The proposed-change card groups this run's diff + actions.
    expect(
      await screen.findByRole('group', { name: 'Proposed change from this run' }),
    ).toBeInTheDocument();
    // The shared DiffBody renders the change with accessible add/del labels…
    expect(screen.getByLabelText('Added: b')).toBeInTheDocument();
    expect(screen.getByLabelText('Removed: a')).toBeInTheDocument();
    // …and a tiny changed-files summary names the touched file.
    expect(screen.getByText('1 file · x.txt')).toBeInTheDocument();
  });

  it('renders no diff preview, summary, or Apply when the diff did not validate (D101)', async () => {
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
    expect(screen.queryByRole('group', { name: 'Proposed change from this run' })).toBeNull();
    expect(screen.queryByLabelText('Added: b')).toBeNull();
    expect(screen.queryByRole('button', { name: 'Apply diff' })).toBeNull();
  });

  it('clears the previous diff preview and actions when a new run starts (D101)', async () => {
    mocks.runAgentSingleStep
      .mockResolvedValueOnce({ events: stream(), applicableDiff: DIFF })
      .mockResolvedValueOnce({ events: stream(), applicableDiff: undefined });
    render(
      <AgentSingleStepPanel
        selected={mlxModel()}
        mlxServers={servers(stepHandle)}
        agentMode="propose-diff"
      />,
    );
    await userEvent.type(screen.getByLabelText('Step instruction'), 'edit it');
    await userEvent.click(screen.getByRole('button', { name: 'Run step' }));
    // First run shows the proposed change.
    expect(
      await screen.findByRole('group', { name: 'Proposed change from this run' }),
    ).toBeInTheDocument();
    expect(screen.getByLabelText('Added: b')).toBeInTheDocument();

    // A second run that yields no applicable diff clears the whole card —
    // the preview, the summary, and the Apply action all go with it.
    await userEvent.click(screen.getByRole('button', { name: 'Run step' }));
    await waitFor(() =>
      expect(screen.queryByRole('group', { name: 'Proposed change from this run' })).toBeNull(),
    );
    expect(screen.queryByLabelText('Added: b')).toBeNull();
    expect(screen.queryByText('1 file · x.txt')).toBeNull();
    expect(screen.queryByRole('button', { name: 'Apply diff' })).toBeNull();
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

  // ─── D102: window-local run history ─────────────────────────────────────

  /** Clear the prompt box, type `text`, and Run. Waits only on the click. */
  async function typeAndRun(text: string) {
    const box = screen.getByLabelText('Step instruction');
    await userEvent.clear(box);
    await userEvent.type(box, text);
    await userEvent.click(screen.getByRole('button', { name: 'Run step' }));
  }

  it('keeps recent runs in a switcher once there is more than one (D102)', async () => {
    mocks.runAgentSingleStep
      .mockResolvedValueOnce({ events: stream(), applicableDiff: DIFF })
      .mockResolvedValueOnce({ events: stream(), applicableDiff: DIFF2 });
    render(
      <AgentSingleStepPanel
        selected={mlxModel()}
        mlxServers={servers(stepHandle)}
        agentMode="propose-diff"
      />,
    );

    await typeAndRun('make A');
    expect(await screen.findByLabelText('Added: b')).toBeInTheDocument();
    // One run alone: nothing to compare against, so no switcher yet.
    expect(screen.queryByRole('group', { name: 'Recent runs' })).toBeNull();

    await typeAndRun('make B');
    expect(await screen.findByLabelText('Added: d')).toBeInTheDocument();

    const runs = await screen.findByRole('group', { name: 'Recent runs' });
    expect(within(runs).getByRole('button', { name: /make A/ })).toBeInTheDocument();
    expect(within(runs).getByRole('button', { name: /make B/ })).toBeInTheDocument();
    // The live (current) run is B; its chip is marked live and pressed.
    const liveChip = within(runs).getByRole('button', { name: /make B/ });
    expect(liveChip).toHaveTextContent('live');
    expect(liveChip).toHaveAttribute('aria-pressed', 'true');
  });

  it("restores a past run's diff preview read-only when selected (D102)", async () => {
    mocks.runAgentSingleStep
      .mockResolvedValueOnce({ events: stream(), applicableDiff: DIFF })
      .mockResolvedValueOnce({ events: stream(), applicableDiff: DIFF2 });
    render(
      <AgentSingleStepPanel
        selected={mlxModel()}
        mlxServers={servers(stepHandle)}
        agentMode="propose-diff"
      />,
    );

    await typeAndRun('make A');
    expect(await screen.findByLabelText('Added: b')).toBeInTheDocument();
    await typeAndRun('make B');
    // Live view is run B's diff, and it is interactive.
    expect(await screen.findByLabelText('Added: d')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Apply diff' })).toBeInTheDocument();

    // Select run A from the switcher → its diff returns, read-only.
    const runs = screen.getByRole('group', { name: 'Recent runs' });
    await userEvent.click(within(runs).getByRole('button', { name: /make A/ }));

    expect(await screen.findByLabelText('Added: b')).toBeInTheDocument();
    expect(screen.queryByLabelText('Added: d')).toBeNull();
    expect(screen.getByText('read-only · past run')).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Apply diff' })).toBeNull();
  });

  it('returns the view to the live run when a new run starts (D102)', async () => {
    mocks.runAgentSingleStep
      .mockResolvedValueOnce({ events: stream(), applicableDiff: DIFF })
      .mockResolvedValueOnce({ events: stream(), applicableDiff: DIFF2 })
      .mockResolvedValueOnce({ events: stream(), applicableDiff: DIFF });
    render(
      <AgentSingleStepPanel
        selected={mlxModel()}
        mlxServers={servers(stepHandle)}
        agentMode="propose-diff"
      />,
    );

    await typeAndRun('make A');
    await screen.findByLabelText('Added: b');
    await typeAndRun('make B');
    await screen.findByLabelText('Added: d');

    // Look back at run A (read-only)…
    await userEvent.click(
      within(screen.getByRole('group', { name: 'Recent runs' })).getByRole('button', {
        name: /make A/,
      }),
    );
    expect(await screen.findByText('read-only · past run')).toBeInTheDocument();

    // …then a new run snaps the view back to live and shows it interactively.
    await typeAndRun('make C');
    expect(await screen.findByLabelText('Added: b')).toBeInTheDocument();
    expect(screen.queryByText('read-only · past run')).toBeNull();
    expect(screen.getByRole('button', { name: 'Apply diff' })).toBeInTheDocument();
  });

  // ─── D123: run-state / boundary legibility ──────────────────────────────

  it("clears the superseded run's event log the moment a new run starts (D123)", async () => {
    // First run resolves normally; the second stays in flight until we fail
    // it, so we can observe the live view during the in-flight window.
    let failSecond: (reason?: unknown) => void = () => {};
    mocks.runAgentSingleStep
      .mockResolvedValueOnce({ events: stream(), applicableDiff: DIFF })
      .mockReturnValueOnce(
        new Promise((_resolve, reject) => {
          failSecond = reject;
        }),
      );
    render(
      <AgentSingleStepPanel
        selected={mlxModel()}
        mlxServers={servers(stepHandle)}
        agentMode="propose-diff"
      />,
    );
    await typeAndRun('make A');
    await waitFor(() => expect(screen.getAllByRole('listitem')).toHaveLength(7));

    // The new run's live view must not show run A's events as its own —
    // they were snapshotted into history, not carried forward.
    await typeAndRun('make B');
    expect(screen.queryAllByRole('listitem')).toHaveLength(0);
    expect(screen.getByText('No agent activity yet.')).toBeInTheDocument();

    // A failed run keeps the log empty rather than resurrecting run A's.
    failSecond({ kind: 'ProviderDown', details: { provider: 'mlx-lm', reason: 'x' } });
    expect(await screen.findByRole('alert')).toHaveTextContent('IPC error: ProviderDown');
    expect(screen.queryAllByRole('listitem')).toHaveLength(0);
  });

  it('tracks the live run in a head status line from the first run onward (D123)', async () => {
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
    // Before any run there is nothing to report.
    expect(screen.queryByRole('status', { name: 'Run status' })).toBeNull();

    await typeAndRun('edit it');
    await waitFor(() =>
      expect(screen.getByRole('status', { name: 'Run status' })).toHaveTextContent('diff ready'),
    );

    await userEvent.click(screen.getByRole('button', { name: 'Apply diff' }));
    await waitFor(() =>
      expect(screen.getByRole('status', { name: 'Run status' })).toHaveTextContent('applied'),
    );

    await userEvent.click(screen.getByRole('button', { name: 'Revert' }));
    await waitFor(() =>
      expect(screen.getByRole('status', { name: 'Run status' })).toHaveTextContent('reverted'),
    );
  });

  it('says so explicitly when a completed run produced no applicable diff (D123)', async () => {
    mocks.runAgentSingleStep.mockResolvedValue({ events: stream(), applicableDiff: undefined });
    render(
      <AgentSingleStepPanel
        selected={mlxModel()}
        mlxServers={servers(stepHandle)}
        agentMode="propose-diff"
      />,
    );
    await typeAndRun('edit it');
    expect(
      await screen.findByText(/This run produced no applicable diff — there is nothing to apply/),
    ).toBeInTheDocument();
  });

  it('does not show the no-diff note when the run produced an applicable diff (D123)', async () => {
    mocks.runAgentSingleStep.mockResolvedValue({ events: stream(), applicableDiff: DIFF });
    render(
      <AgentSingleStepPanel
        selected={mlxModel()}
        mlxServers={servers(stepHandle)}
        agentMode="propose-diff"
      />,
    );
    await typeAndRun('edit it');
    await screen.findByRole('button', { name: 'Apply diff' });
    expect(screen.queryByText(/produced no applicable diff/)).toBeNull();
  });

  it('shows revert-failure copy and keeps Revert available (D123)', async () => {
    mocks.runAgentSingleStep.mockResolvedValue({ events: stream(), applicableDiff: DIFF });
    mocks.applyPatch.mockResolvedValue({
      applied: true,
      checkpoint: 'abcd1234ef',
      touched: [{ path: 'x.txt', changeType: 'modify', bytesWritten: 2 }],
    });
    mocks.revertPatch.mockResolvedValue({
      reverted: false,
      reason: 'drift',
      details: [{ path: 'x.txt', message: 'file changed on disk' }],
    });
    render(
      <AgentSingleStepPanel
        selected={mlxModel()}
        mlxServers={servers(stepHandle)}
        agentMode="propose-diff"
      />,
    );
    await typeAndRun('edit it');
    await userEvent.click(await screen.findByRole('button', { name: 'Apply diff' }));
    await userEvent.click(await screen.findByRole('button', { name: 'Revert' }));

    // The dedicated failure copy appears (pre-D123 this silently fell back
    // to the applied-state note), and Revert stays available to retry.
    expect(
      await screen.findByText(/Revert failed — the applied files were left as they are/),
    ).toBeInTheDocument();
    const revert = screen.getByRole('button', { name: 'Revert' });
    expect(revert).toBeEnabled();
  });

  it('banners a past-run view and returns to live via Back to live run (D123)', async () => {
    mocks.runAgentSingleStep
      .mockResolvedValueOnce({ events: stream(), applicableDiff: DIFF })
      .mockResolvedValueOnce({ events: stream(), applicableDiff: DIFF2 });
    render(
      <AgentSingleStepPanel
        selected={mlxModel()}
        mlxServers={servers(stepHandle)}
        agentMode="propose-diff"
      />,
    );
    await typeAndRun('make A');
    await screen.findByLabelText('Added: b');
    await typeAndRun('make B');
    await screen.findByLabelText('Added: d');

    await userEvent.click(
      within(screen.getByRole('group', { name: 'Recent runs' })).getByRole('button', {
        name: /make A/,
      }),
    );
    // The banner names the run being viewed and marks it read-only…
    expect(await screen.findByText(/Viewing a past run \(read-only\)/)).toBeInTheDocument();
    expect(screen.getByText(/make A/, { selector: '.plume-agent-singlestep-viewing-text' }))
      .toBeInTheDocument();
    // …the head status line goes quiet (it describes only the live run)…
    expect(screen.queryByRole('status', { name: 'Run status' })).toBeNull();

    // …and one click returns to the interactive live run.
    await userEvent.click(screen.getByRole('button', { name: 'Back to live run' }));
    expect(screen.queryByText(/Viewing a past run/)).toBeNull();
    expect(await screen.findByLabelText('Added: d')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Apply diff' })).toBeInTheDocument();
  });

  // ─── D124: pins for the remaining user-visible copy/state ───────────────

  it('shows running… in the head status line while the step is in flight (D124)', async () => {
    // Hold the run in flight so the pre-resolution window is observable.
    let resolveRun: (value: unknown) => void = () => {};
    mocks.runAgentSingleStep.mockReturnValueOnce(
      new Promise((resolve) => {
        resolveRun = resolve;
      }),
    );
    render(
      <AgentSingleStepPanel
        selected={mlxModel()}
        mlxServers={servers(stepHandle)}
        agentMode="propose-diff"
      />,
    );
    await typeAndRun('edit it');

    // The status line tracks the live run from the moment it starts.
    expect(screen.getByRole('status', { name: 'Run status' })).toHaveTextContent('running…');
    expect(screen.getByRole('button', { name: 'Running…' })).toBeDisabled();

    resolveRun({ events: stream(), applicableDiff: DIFF });
    await waitFor(() =>
      expect(screen.getByRole('status', { name: 'Run status' })).toHaveTextContent('diff ready'),
    );
  });

  it('walks the apply-note copy through ready → applied → reverted (D124)', async () => {
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
    await typeAndRun('edit it');

    // Before any write the note explains what Apply will do — and that a
    // checkpoint makes it undoable. (The note copy is deliberately case-
    // distinct from the lowercase event-log frames, so these regexes can't
    // accidentally match the log.)
    expect(
      await screen.findByText(/Writes this diff to your project files\. A checkpoint is saved/),
    ).toBeInTheDocument();

    await userEvent.click(screen.getByRole('button', { name: 'Apply diff' }));
    expect(
      await screen.findByText(/Applied — a checkpoint was saved first, so Revert can undo this\./),
    ).toBeInTheDocument();

    await userEvent.click(screen.getByRole('button', { name: 'Revert' }));
    expect(
      await screen.findByText(/Reverted — your files are back to the pre-apply state\./),
    ).toBeInTheDocument();
  });

  it('shows the apply-failure note naming the nothing-changed guarantee (D124)', async () => {
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
    await typeAndRun('edit it');
    await userEvent.click(await screen.findByRole('button', { name: 'Apply diff' }));

    expect(
      await screen.findByText(/Apply failed — nothing changed on disk\. See the log; you can try/),
    ).toBeInTheDocument();
    expect(screen.getByRole('status', { name: 'Run status' })).toHaveTextContent('apply failed');
  });

  it('offers no apply control for a non-current run (D102)', async () => {
    mocks.runAgentSingleStep
      .mockResolvedValueOnce({ events: stream(), applicableDiff: DIFF })
      .mockResolvedValueOnce({ events: stream(), applicableDiff: DIFF2 });
    render(
      <AgentSingleStepPanel
        selected={mlxModel()}
        mlxServers={servers(stepHandle)}
        agentMode="propose-diff"
      />,
    );

    await typeAndRun('make A');
    await screen.findByLabelText('Added: b');
    await typeAndRun('make B');
    await screen.findByLabelText('Added: d');

    await userEvent.click(
      within(screen.getByRole('group', { name: 'Recent runs' })).getByRole('button', {
        name: /make A/,
      }),
    );
    await screen.findByText('read-only · past run');

    // A non-current run exposes no write affordance at all.
    expect(screen.queryByRole('button', { name: 'Apply diff' })).toBeNull();
    expect(screen.queryByRole('button', { name: 'Revert' })).toBeNull();
    expect(mocks.applyPatch).not.toHaveBeenCalled();
  });
});
