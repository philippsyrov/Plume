// D96: single-step local agent panel — the first *executing* slice.
//
// Sends the user's instruction to the selected, running local MLX model
// (propose-diff prompt), then renders the real D85 event stream the
// backend returns: the model's reply, the read-only `patch.validate` it
// ran, and — if the diff is valid — the apply step held behind the
// approval gate. Nothing is written: applying a diff always pauses for the
// user, and an unsupported tool request shows as a blocked event.
//
// Mirrors AgentDryRunPanel's shape (busy/error/mountedRef + AgentEventLog),
// but the events are real and it needs the selected model + its running
// server handle to send.

import { useCallback, useEffect, useMemo, useRef, useState } from 'react';

import { runAgentSingleStep } from '../../lib/api/agent';
import type { AgentEventEnvelope } from '../../lib/api/agentEvents';
import { isIpcError } from '../../lib/api/errors';
import type { AgentMode } from '../../lib/api/session';
import type { SelectedModel } from '../model-picker/useSelectedModel';
import type { MlxServersApi } from '../providers/useMlxServers';
import { AgentEventLog } from './AgentEventLog';

export type AgentSingleStepPanelProps = {
  selected: SelectedModel | null;
  mlxServers: MlxServersApi;
  /** The session's agentMode, mirrored from AgentSettingsPanel. A step is
   *  only allowed in `propose-diff` or higher — `chat` is talk-only. `null`
   *  while the mode is still loading (the backend stays authoritative). */
  agentMode?: AgentMode | null;
};

export function AgentSingleStepPanel({
  selected,
  mlxServers,
  agentMode = null,
}: AgentSingleStepPanelProps) {
  const [prompt, setPrompt] = useState('');
  const [events, setEvents] = useState<AgentEventEnvelope[]>([]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Skip post-await state writes if the panel unmounted mid-request.
  const mountedRef = useRef(true);
  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
    };
  }, []);

  // Resolve the running MLX server for the selected model, if any. Only
  // an MLX model with a live server can run a step.
  const isMlx = selected?.providerId === 'mlx-lm';
  const handle = useMemo(
    () => (selected && isMlx ? mlxServers.handleOf(selected.modelId) : null),
    [selected, isMlx, mlxServers],
  );

  // The agentMode axis: `chat` is talk-only. We only block when we know the
  // mode is chat; while it's still loading (`null`) we defer to the backend,
  // which rejects a chat-mode step authoritatively.
  const modeBlocked = agentMode === 'chat';
  const trimmed = prompt.trim();
  const canRun = !busy && !modeBlocked && isMlx && handle != null && trimmed.length > 0;

  // One honest line for why Run is unavailable, in priority order — the
  // mode gate is the most fundamental, so it comes first.
  const blockedReason = modeBlocked
    ? 'Switch Agent mode to Propose diff or higher to run a step.'
    : !isMlx
      ? 'Select a local (MLX) model to run a step.'
      : handle == null
        ? 'Start the selected model to run a step.'
        : trimmed.length === 0
          ? 'Type an instruction to run a step.'
          : null;

  const onRun = useCallback(async () => {
    if (!selected || handle == null) return;
    setBusy(true);
    setError(null);
    try {
      const resp = await runAgentSingleStep({
        prompt: trimmed,
        providerId: 'mlx-lm',
        modelId: selected.modelId,
        handleId: handle.id,
      });
      if (mountedRef.current) setEvents(resp.events);
    } catch (err) {
      if (mountedRef.current) {
        setError(isIpcError(err) ? `IPC error: ${err.kind}` : 'Single step failed.');
      }
    } finally {
      if (mountedRef.current) setBusy(false);
    }
  }, [selected, handle, trimmed]);

  return (
    <section className="plume-agent-singlestep ink-panel" aria-label="Single-step agent">
      <div className="plume-agent-singlestep-head">
        <h3>Run one step</h3>
        <button
          type="button"
          className="ink-button"
          onClick={() => void onRun()}
          disabled={!canRun}
        >
          {busy ? 'Running…' : 'Run step'}
        </button>
      </div>
      <p className="plume-agent-singlestep-hint">
        Sends your instruction to the selected local model and asks for a diff. Plume validates it
        and shows the real event stream — applying stays behind approval, so nothing is written.
      </p>
      <textarea
        className="plume-agent-singlestep-prompt"
        aria-label="Step instruction"
        placeholder="e.g. make greet() return an f-string"
        value={prompt}
        onChange={(e) => setPrompt(e.target.value)}
        rows={3}
        disabled={busy}
      />
      {blockedReason ? <p className="plume-agent-singlestep-note">{blockedReason}</p> : null}
      {error ? (
        <p className="plume-agent-singlestep-error" role="alert">
          {error}
        </p>
      ) : null}
      <AgentEventLog events={events} />
    </section>
  );
}
