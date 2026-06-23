import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import { AgentDryRunPanel } from './AgentDryRunPanel';
import type { AgentEventEnvelope } from '../../lib/api/agentEvents';

const mocks = vi.hoisted(() => ({ runAgentDryRun: vi.fn() }));
vi.mock('../../lib/api/agent', () => ({ runAgentDryRun: mocks.runAgentDryRun }));

function stream(): AgentEventEnvelope[] {
  return [
    { seq: 0, tsMs: 1, kind: 'messageChunk', text: 'Looking at the project.' },
    { seq: 1, tsMs: 1, kind: 'toolProposed', callId: 't1', tool: 'search', summary: 'grep TODO' },
    { seq: 2, tsMs: 1, kind: 'toolStarted', callId: 't1', tool: 'search' },
    { seq: 3, tsMs: 1, kind: 'toolFinished', callId: 't1', tool: 'search', summary: '3 matches' },
    { seq: 4, tsMs: 1, kind: 'done', summary: 'dry run complete' },
  ];
}

describe('AgentDryRunPanel — D93', () => {
  beforeEach(() => vi.clearAllMocks());

  it('shows the empty event log before a run', () => {
    render(<AgentDryRunPanel />);
    expect(screen.getByText('No agent activity yet.')).toBeInTheDocument();
  });

  it('fetches and renders the typed event stream on Run', async () => {
    mocks.runAgentDryRun.mockResolvedValue({ events: stream() });
    render(<AgentDryRunPanel />);

    await userEvent.click(screen.getByRole('button', { name: 'Run dry-run' }));

    await waitFor(() => expect(screen.getAllByRole('listitem')).toHaveLength(5));
    expect(screen.getByText('Looking at the project.')).toBeInTheDocument();
    expect(screen.getByText('proposes search: grep TODO')).toBeInTheDocument();
    expect(screen.getByText('done — dry run complete')).toBeInTheDocument();
    expect(mocks.runAgentDryRun).toHaveBeenCalledTimes(1);
  });

  it('surfaces an IPC error without crashing', async () => {
    mocks.runAgentDryRun.mockRejectedValue({ kind: 'Internal', details: 'boom' });
    render(<AgentDryRunPanel />);
    await userEvent.click(screen.getByRole('button', { name: 'Run dry-run' }));
    expect(await screen.findByRole('alert')).toHaveTextContent('IPC error: Internal');
  });
});
