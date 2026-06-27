// Typed wrappers for the `agent.*` IPC family (D93 + D96).
//
// `agent.dryRun` returns a deterministic, dev-only sequence of typed
// agent events (D85) so the UI can prove the event protocol drives the
// `AgentEventLog` surface end to end. Nothing real runs.
//
// `agent.singleStep` (D96) is the first executing step: it sends the
// user's prompt to the selected, running local MLX model, classifies the
// reply, runs the one safe action (read-only `patch.validate`), gates
// applying behind approval, and returns the real D85 event stream. It
// never applies a diff, runs a shell command, or recurses.

import { invokeIpc } from './ipc';
import type { AgentEventEnvelope } from './agentEvents';
import type { ChatAttachment } from './chat';

export type AgentDryRunResponse = {
  events: AgentEventEnvelope[];
};

export function runAgentDryRun(): Promise<AgentDryRunResponse> {
  return invokeIpc<Record<string, never>, AgentDryRunResponse>('agent_dry_run', {});
}

export type AgentSingleStepPayload = {
  /** The user's instruction for this one step. */
  prompt: string;
  /** Must be `'mlx-lm'` — the only provider wired for execution. */
  providerId: string;
  /** Pretty inventory id of the selected model (echoed for parity). */
  modelId: string;
  /** Server handle from `providers.startServer` for the running model. */
  handleId: string;
  /** D99 (optional): a single read-only project-file attachment folded
   *  into the propose-diff prompt as context — same shape and backend
   *  guards as the chat panel's `chat.send` attachment (redaction, size
   *  cap, optional 1-based line range). Omit for no context. */
  attachment?: ChatAttachment;
};

export type AgentSingleStepResponse = {
  events: AgentEventEnvelope[];
  /** D100: the model's diff, present ONLY when it validated — i.e. the diff
   *  the user may now apply. `undefined` for an invalid diff, a blocked tool
   *  request, or no diff. Carried separately from the (truncated) message
   *  event so an explicit Apply can run the full diff through `patch.apply`. */
  applicableDiff?: string;
};

export function runAgentSingleStep(
  payload: AgentSingleStepPayload,
): Promise<AgentSingleStepResponse> {
  return invokeIpc<AgentSingleStepPayload, AgentSingleStepResponse>('agent_single_step', payload);
}
