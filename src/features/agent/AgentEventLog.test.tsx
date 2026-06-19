import { render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';

import { AgentEventLog } from './AgentEventLog';
import type { AgentEventEnvelope } from '../../lib/api/agentEvents';

function stream(): AgentEventEnvelope[] {
  return [
    { seq: 0, tsMs: 1, kind: 'messageChunk', text: 'Let me run the tests.' },
    { seq: 1, tsMs: 2, kind: 'toolProposed', callId: 'c1', tool: 'command', summary: 'cargo test' },
    { seq: 2, tsMs: 3, kind: 'approvalRequired', callId: 'c1', tool: 'command', prompt: 'run cargo test?' },
    { seq: 3, tsMs: 4, kind: 'toolStarted', callId: 'c1', tool: 'command' },
    { seq: 4, tsMs: 5, kind: 'toolFinished', callId: 'c1', tool: 'command', summary: 'exit 0' },
    { seq: 5, tsMs: 6, kind: 'done', summary: 'all green' },
  ];
}

describe('AgentEventLog — D85 skeleton', () => {
  it('renders an empty state when there are no events', () => {
    render(<AgentEventLog events={[]} />);
    expect(screen.getByText('No agent activity yet.')).toBeInTheDocument();
  });

  it('renders one row per stream frame in order, labeled by kind', () => {
    render(<AgentEventLog events={stream()} />);
    const items = screen.getAllByRole('listitem');
    expect(items).toHaveLength(6);

    expect(items[0]).toHaveTextContent('Let me run the tests.');
    expect(items[1]).toHaveTextContent('proposes command: cargo test');
    expect(items[2]).toHaveTextContent('needs approval — run cargo test?');
    expect(items[3]).toHaveTextContent('running command…');
    expect(items[4]).toHaveTextContent('command done: exit 0');
    expect(items[5]).toHaveTextContent('done — all green');
  });

  it('tints a failed tool row and renders the error', () => {
    const events: AgentEventEnvelope[] = [
      { seq: 0, tsMs: 1, kind: 'toolFailed', callId: 'c9', tool: 'command', error: 'exit 1' },
    ];
    render(<AgentEventLog events={events} />);
    const row = screen.getByRole('listitem');
    expect(row).toHaveClass('plume-agent-event-toolFailed');
    expect(row).toHaveTextContent('command failed: exit 1');
  });

  it('renders a bare done event without a summary', () => {
    const events: AgentEventEnvelope[] = [{ seq: 0, tsMs: 1, kind: 'done', summary: null }];
    render(<AgentEventLog events={events} />);
    expect(screen.getByRole('listitem')).toHaveTextContent('done');
  });
});
