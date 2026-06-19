// Typed wrappers for the `session.*` IPC family (D77 backend, D84 UI).
//
// Four verbs, the agent-autonomy config substrate from
// `docs/SAFETY.md § "Agent autonomy is two independent axes"`:
//   - `session.setMode` — flip `agentMode`.
//   - `session.setApprovalPolicy` — flip `approvalPolicy`.
//   - `session.setAllowlist` — replace fileAllowlist / commandAllowlist /
//     iterationCap together.
//   - `session.state` — read the current config.
//
// These are window-scoped session state and touch no disk, so unlike the
// memory / patch verbs they are NOT trust-gated. The backend resets the
// config to its least-privilege default on every `project.open`, so the
// config is effectively per-project for the lifetime of the open project.
//
// Surface rule: each setter validates the RESULTING config server-side
// and returns an in-band `AgentConfigResponse` — `{ ok: true, state }`
// on success or `{ ok: false, reasons }` listing every broken invariant
// (the stored config is left untouched). The Promise only rejects on
// IPC-shape (`Version`) problems, never on a refused config.

import { invokeIpc } from './ipc';

/** What the model may do. Independent of {@link ApprovalPolicy}. Kebab
 *  values match the backend `AgentMode` serde rename. */
export type AgentMode = 'chat' | 'propose-diff' | 'scoped-edit' | 'agent-loop';

/** When the user is asked. Independent of {@link AgentMode}. */
export type ApprovalPolicy = 'ask-each' | 'ask-on-write' | 'ask-on-fail';

export type AgentConfig = {
  mode: AgentMode;
  approvalPolicy: ApprovalPolicy;
  /** Project-relative path prefixes the agent may write under in
   *  scoped-edit / agent-loop. Empty means "no writes". */
  fileAllowlist: string[];
  /** Approved argv vectors the agent may run. Each entry is a full argv
   *  (`["cargo", "test"]`). Empty means "no commands". */
  commandAllowlist: string[][];
  /** Maximum agent-loop iterations. `null` until set; required before
   *  `agent-loop` mode validates. */
  iterationCap: number | null;
};

/** In-band setter outcome: the new config, or the list of broken
 *  invariants that left the stored config unchanged. */
export type AgentConfigResponse =
  | { ok: true; state: AgentConfig }
  | { ok: false; reasons: string[] };

/** Backend `MAX_ITERATION_CAP` — a request above this is rejected, not
 *  clamped. Mirrored so the input can guard before round-tripping. */
export const AGENT_MAX_ITERATION_CAP = 100;

/** Backend `MAX_ALLOWLIST_ENTRIES`. */
export const AGENT_MAX_ALLOWLIST_ENTRIES = 64;

export const AGENT_MODES: AgentMode[] = ['chat', 'propose-diff', 'scoped-edit', 'agent-loop'];
export const APPROVAL_POLICIES: ApprovalPolicy[] = ['ask-each', 'ask-on-write', 'ask-on-fail'];

export function getSessionState(): Promise<AgentConfig> {
  return invokeIpc<Record<string, never>, AgentConfig>('session_state', {});
}

export function setAgentMode(mode: AgentMode): Promise<AgentConfigResponse> {
  return invokeIpc<{ mode: AgentMode }, AgentConfigResponse>('session_set_mode', { mode });
}

export function setApprovalPolicy(
  approvalPolicy: ApprovalPolicy,
): Promise<AgentConfigResponse> {
  return invokeIpc<{ approvalPolicy: ApprovalPolicy }, AgentConfigResponse>(
    'session_set_approval_policy',
    { approvalPolicy },
  );
}

export type SetAllowlistInput = {
  fileAllowlist: string[];
  commandAllowlist: string[][];
  iterationCap: number | null;
};

export function setAllowlist(input: SetAllowlistInput): Promise<AgentConfigResponse> {
  return invokeIpc<SetAllowlistInput, AgentConfigResponse>('session_set_allowlist', input);
}
