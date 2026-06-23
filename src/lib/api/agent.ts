// Typed wrapper for the `agent.*` IPC family (D93).
//
// `agent.dryRun` returns a deterministic, dev-only sequence of typed
// agent events (D85) so the UI can prove the event protocol drives the
// `AgentEventLog` surface end to end. Nothing real runs — no model, no
// shell, no patch, no file writes. Pure read, not trust-gated.

import { invokeIpc } from './ipc';
import type { AgentEventEnvelope } from './agentEvents';

export type AgentDryRunResponse = {
  events: AgentEventEnvelope[];
};

export function runAgentDryRun(): Promise<AgentDryRunResponse> {
  return invokeIpc<Record<string, never>, AgentDryRunResponse>('agent_dry_run', {});
}
