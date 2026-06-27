// D93: agent event dry-run — plumbing proof that the typed D85 event
// stream drives the existing AgentEventLog surface.
//
// Dev-only: clicking "Run dry-run" fetches a deterministic, scripted
// sequence of typed agent events (`agent.dryRun`) and renders them in the
// AgentEventLog. Nothing real runs — no model, no shell, no patch, no
// file writes. This exists to prove the IPC → state → render path before
// the real loop controller emits these same shapes.

import { useCallback, useEffect, useRef, useState } from 'react';

import { runAgentDryRun } from '../../lib/api/agent';
import type { AgentEventEnvelope } from '../../lib/api/agentEvents';
import { isIpcError } from '../../lib/api/errors';
import { AgentEventLog } from './AgentEventLog';

export function AgentDryRunPanel() {
  const [events, setEvents] = useState<AgentEventEnvelope[]>([]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Skip post-await state writes if the panel unmounted mid-request (the
  // user hid the Agent panel / closed the view before IPC resolved).
  const mountedRef = useRef(true);
  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
    };
  }, []);

  const onRun = useCallback(async () => {
    setBusy(true);
    setError(null);
    try {
      const resp = await runAgentDryRun();
      if (mountedRef.current) setEvents(resp.events);
    } catch (err) {
      if (mountedRef.current) {
        setError(isIpcError(err) ? `IPC error: ${err.kind}` : 'Dry run failed.');
      }
    } finally {
      if (mountedRef.current) setBusy(false);
    }
  }, []);

  // D98: the dry-run is a dev plumbing proof, not a control a user reaches
  // for — it used to sit open as a third agent card and crowd the column.
  // It now lives behind a collapsed disclosure marked "dev", so the Agent
  // panel shows only the two real surfaces (settings + Run one step) by
  // default. The body is unchanged; expanding it reveals the same button +
  // event log.
  return (
    <details className="plume-agent-dryrun ink-panel">
      <summary className="plume-agent-dryrun-summary">
        <span>Event stream dry-run</span>
        <span className="plume-agent-dryrun-devtag">dev</span>
      </summary>
      <div className="plume-agent-dryrun-body">
        <div className="plume-agent-dryrun-head">
          <p className="plume-agent-dryrun-hint">
            Renders the typed agent event stream end to end. Dev plumbing
            proof — no model, no shell, no patches, nothing real runs.
          </p>
          <button
            type="button"
            className="ink-button"
            onClick={() => void onRun()}
            disabled={busy}
          >
            {busy ? 'Running…' : 'Run dry-run'}
          </button>
        </div>
        {error ? (
          <p className="plume-agent-dryrun-error" role="alert">
            {error}
          </p>
        ) : null}
        <AgentEventLog events={events} />
      </div>
    </details>
  );
}
