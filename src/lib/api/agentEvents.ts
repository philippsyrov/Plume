// Agent event protocol scaffold (D85) — frontend mirror.
//
// The TypeScript twin of `src-tauri/src/agent/events.rs`. A future agent
// run will stream `AgentEventEnvelope`s (a monotonic `seq` + `tsMs`
// wrapping one `AgentEvent`); `AgentEventLog` renders them into a live
// transcript without parsing free text. See the Rust module for the
// authoritative shapes and the Hermes-stream rationale.
//
// Scaffold only — no IPC channel emits these yet. These types fix the
// wire vocabulary so the executing slice wires a channel into shapes
// both ends already agree on. The discriminated union keys on `kind`,
// matching the backend's internally-tagged serde.

/** Coarse category of a tool the agent proposes / runs. */
export type AgentToolKind = 'read' | 'write' | 'command' | 'search' | 'other';

/** One event in an agent run's stream. Keyed by `kind`. The tool
 *  lifecycle (`toolProposed` → optional `approvalRequired` → `toolStarted`
 *  → `toolFinished` | `toolFailed`) shares a `callId` so the UI can
 *  collapse it into one row. `paused` / `done` are run-level terminals. */
export type AgentEvent =
  | { kind: 'messageChunk'; text: string }
  | { kind: 'toolProposed'; callId: string; tool: AgentToolKind; summary: string }
  | { kind: 'approvalRequired'; callId: string; tool: AgentToolKind; prompt: string }
  | { kind: 'toolStarted'; callId: string; tool: AgentToolKind }
  | { kind: 'toolFinished'; callId: string; tool: AgentToolKind; summary: string }
  | { kind: 'toolFailed'; callId: string; tool: AgentToolKind; error: string }
  | { kind: 'paused'; reason: string }
  | { kind: 'done'; summary: string | null };

/** A stream frame: the event's fields flattened under a `seq` + `tsMs`.
 *  A gap in `seq` signals a dropped frame; a repeat signals a replay. */
export type AgentEventEnvelope = AgentEvent & {
  seq: number;
  tsMs: number;
};
