// Agent event renderer skeleton (D85).
//
// Presentational only: given the `AgentEventEnvelope` stream a future
// agent run will emit, render a compact transcript. No IPC, no state, no
// model — the executing slice owns the channel that feeds `events`. This
// slice fixes the rendering vocabulary (one row per event, keyed by
// `seq`, labeled by `kind`) against the wire types in
// `src/lib/api/agentEvents.ts`.
//
// Consecutive `messageChunk` events are NOT merged here — that's the
// executing slice's concern (it owns the assistant-message buffer). The
// skeleton renders each frame faithfully so the shape stays debuggable.

import type { AgentEvent, AgentEventEnvelope, AgentToolKind } from '../../lib/api/agentEvents';

const TOOL_LABEL: Record<AgentToolKind, string> = {
  read: 'read',
  write: 'write',
  command: 'command',
  search: 'search',
  other: 'tool',
};

/** Stable per-kind class suffix so the stylesheet can tint a row by
 *  lifecycle stage (proposed/started/finished/failed). */
function kindClass(event: AgentEvent): string {
  return `plume-agent-event-${event.kind}`;
}

/** One-line human summary of an event. Kept here (not on the wire) so the
 *  copy can change without a protocol bump. */
function describe(event: AgentEvent): string {
  switch (event.kind) {
    case 'messageChunk':
      return event.text;
    case 'toolProposed':
      return `proposes ${TOOL_LABEL[event.tool]}: ${event.summary}`;
    case 'approvalRequired':
      return `needs approval — ${event.prompt}`;
    case 'toolStarted':
      return `running ${TOOL_LABEL[event.tool]}…`;
    case 'toolFinished':
      return `${TOOL_LABEL[event.tool]} done: ${event.summary}`;
    case 'toolFailed':
      return `${TOOL_LABEL[event.tool]} failed: ${event.error}`;
    case 'paused':
      return `paused — ${event.reason}`;
    case 'done':
      return event.summary ? `done — ${event.summary}` : 'done';
  }
}

/** Short tag shown at the left of each row. */
function tag(event: AgentEvent): string {
  switch (event.kind) {
    case 'messageChunk':
      return '·';
    case 'toolProposed':
      return 'proposed';
    case 'approvalRequired':
      return 'approve?';
    case 'toolStarted':
      return 'run';
    case 'toolFinished':
      return 'ok';
    case 'toolFailed':
      return 'fail';
    case 'paused':
      return 'paused';
    case 'done':
      return 'done';
  }
}

export type AgentEventLogProps = {
  events: AgentEventEnvelope[];
};

export function AgentEventLog({ events }: AgentEventLogProps) {
  if (events.length === 0) {
    return (
      <div className="plume-agent-event-log plume-agent-event-log-empty" aria-label="Agent transcript">
        <p className="plume-agent-event-empty">No agent activity yet.</p>
      </div>
    );
  }
  return (
    <ol className="plume-agent-event-log" aria-label="Agent transcript">
      {events.map((envelope) => (
        <li key={envelope.seq} className={`plume-agent-event ${kindClass(envelope)}`}>
          <span className="plume-agent-event-tag">{tag(envelope)}</span>
          <span className="plume-agent-event-text">{describe(envelope)}</span>
        </li>
      ))}
    </ol>
  );
}
