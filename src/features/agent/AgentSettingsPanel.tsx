// D84: agent autonomy settings.
//
// A compact, project-scoped surface for the agent-autonomy config the
// D77 backend already holds (`session.*` IPC). Two independent axes plus
// the explicit gates the higher modes require, straight from
// `docs/SAFETY.md § "Agent autonomy is two independent axes"`:
//
//   * mode            — chat < propose-diff < scoped-edit < agent-loop
//   * approvalPolicy  — ask-each / ask-on-write / ask-on-fail
//   * fileAllowlist   — project-relative path prefixes writes may touch
//   * commandAllowlist— approved argv vectors the agent may run
//   * iterationCap    — agent-loop iteration budget
//
// No tool execution here — this only declares intent. The actions the
// config gates (writes, commands, the loop) are trust- and
// approval-checked when they actually run, in later slices.
//
// The backend is the source of truth: it validates the RESULTING config
// on every setter and resets to the least-privilege default on each
// project open, so this panel reads `session.state` on mount and mirrors
// whatever the backend commits. Mode and policy apply immediately (one
// verb each); the allowlists + cap are edited as text and committed
// together with Apply. Any rejected config surfaces its broken
// invariants inline and leaves the stored config untouched — the
// fail-closed rule means flipping to agent-loop without gates is refused,
// not silently downgraded.

import { useCallback, useEffect, useRef, useState } from 'react';

import {
  AGENT_MAX_ITERATION_CAP,
  AGENT_MODES,
  APPROVAL_POLICIES,
  getSessionState,
  setAgentMode,
  setAllowlist,
  setApprovalPolicy,
  type AgentConfig,
  type AgentConfigResponse,
  type AgentMode,
  type ApprovalPolicy,
} from '../../lib/api/session';
import { isIpcError } from '../../lib/api/errors';

type LoadState =
  | { kind: 'loading' }
  | { kind: 'ready'; config: AgentConfig }
  | { kind: 'error'; message: string };

const MODE_LABEL: Record<AgentMode, string> = {
  chat: 'Chat',
  'propose-diff': 'Propose diff',
  'scoped-edit': 'Scoped edit',
  'agent-loop': 'Agent loop',
};

const POLICY_LABEL: Record<ApprovalPolicy, string> = {
  'ask-each': 'Ask each',
  'ask-on-write': 'Ask on write',
  'ask-on-fail': 'Ask on fail',
};

/** One command per line; whitespace splits a line into an argv. Simple
 *  by design — v1 allowlist commands are plain (`cargo test`); exact
 *  quoting / embedded spaces are a later concern, and the backend stores
 *  argv either way. */
function parseCommandAllowlist(text: string): string[][] {
  return text
    .split('\n')
    .map((line) => line.trim())
    .filter((line) => line.length > 0)
    .map((line) => line.split(/\s+/));
}

function formatCommandAllowlist(argvs: string[][]): string {
  return argvs.map((argv) => argv.join(' ')).join('\n');
}

function parsePathAllowlist(text: string): string[] {
  return text
    .split('\n')
    .map((line) => line.trim())
    .filter((line) => line.length > 0);
}

export type AgentSettingsPanelProps = {
  /** Mirror the committed agentMode upward so sibling surfaces (the
   *  single-step panel) can gate on it without a second source of truth.
   *  Fires on initial load and after every committed mode change. */
  onModeChange?: (mode: AgentMode) => void;
};

export function AgentSettingsPanel({ onModeChange }: AgentSettingsPanelProps = {}) {
  const [state, setState] = useState<LoadState>({ kind: 'loading' });
  // Pending validation reasons from the most recent rejected setter.
  const [reasons, setReasons] = useState<string[]>([]);
  const [busy, setBusy] = useState(false);

  // Draft text for the allowlist block — edited freely, committed on
  // Apply. Kept separate from the committed config so typing doesn't
  // round-trip the backend on every keystroke.
  const [fileDraft, setFileDraft] = useState('');
  const [commandDraft, setCommandDraft] = useState('');
  const [capDraft, setCapDraft] = useState('');

  const mountedRef = useRef(true);
  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
    };
  }, []);

  // Mirror a committed config into local state + the editable drafts, and
  // surface the committed mode upward (initial load + every change).
  const adopt = useCallback(
    (config: AgentConfig) => {
      setState({ kind: 'ready', config });
      setFileDraft(config.fileAllowlist.join('\n'));
      setCommandDraft(formatCommandAllowlist(config.commandAllowlist));
      setCapDraft(config.iterationCap === null ? '' : String(config.iterationCap));
      onModeChange?.(config.mode);
    },
    [onModeChange],
  );

  useEffect(() => {
    let cancelled = false;
    setState({ kind: 'loading' });
    getSessionState()
      .then((config) => {
        if (cancelled) return;
        adopt(config);
      })
      .catch((err) => {
        if (cancelled) return;
        setState({
          kind: 'error',
          message: isIpcError(err) ? err.kind : 'Could not load agent settings.',
        });
      });
    return () => {
      cancelled = true;
    };
  }, [adopt]);

  // Funnel every setter response through one place: commit + clear
  // reasons on ok, surface reasons (config untouched) on refusal.
  const handle = useCallback(
    (resp: AgentConfigResponse) => {
      if (!mountedRef.current) return;
      if (resp.ok) {
        adopt(resp.state);
        setReasons([]);
      } else {
        setReasons(resp.reasons);
      }
    },
    [adopt],
  );

  const run = useCallback(
    async (op: () => Promise<AgentConfigResponse>) => {
      setBusy(true);
      try {
        const resp = await op();
        handle(resp);
      } catch (err) {
        if (mountedRef.current) {
          setReasons([isIpcError(err) ? `IPC error: ${err.kind}` : 'Request failed.']);
        }
      } finally {
        if (mountedRef.current) setBusy(false);
      }
    },
    [handle],
  );

  if (state.kind === 'loading') {
    return (
      <section className="plume-agent-settings ink-panel" aria-label="Agent settings">
        <h3>Agent</h3>
        <p className="plume-agent-settings-hint">Loading…</p>
      </section>
    );
  }
  if (state.kind === 'error') {
    return (
      <section className="plume-agent-settings ink-panel" aria-label="Agent settings">
        <h3>Agent</h3>
        <p className="plume-agent-settings-error" role="alert">
          {state.message}
        </p>
      </section>
    );
  }

  const { config } = state;
  // The cap field is only an error if it's non-empty and not a valid
  // 1..=MAX integer; empty is the legitimate "no cap" value.
  const capTrimmed = capDraft.trim();
  const capInvalid = capTrimmed.length > 0 && !/^\d+$/.test(capTrimmed);
  const onApplyAllowlist = () => {
    const iterationCap = capTrimmed.length === 0 ? null : Number(capTrimmed);
    void run(() =>
      setAllowlist({
        fileAllowlist: parsePathAllowlist(fileDraft),
        commandAllowlist: parseCommandAllowlist(commandDraft),
        iterationCap,
      }),
    );
  };

  return (
    <section className="plume-agent-settings ink-panel" aria-label="Agent settings">
      <h3>Agent</h3>
      <p className="plume-agent-settings-hint">
        Autonomy is two axes. Nothing here runs tools — it only sets what a
        future agent run is allowed to do. agent-loop needs file + command
        allowlists and a cap.
      </p>

      <label className="plume-agent-settings-field">
        <span>Mode</span>
        <select
          className="plume-agent-settings-select"
          value={config.mode}
          disabled={busy}
          onChange={(e) => void run(() => setAgentMode(e.target.value as AgentMode))}
        >
          {AGENT_MODES.map((m) => (
            <option key={m} value={m}>
              {MODE_LABEL[m]}
            </option>
          ))}
        </select>
      </label>

      <label className="plume-agent-settings-field">
        <span>Approval</span>
        <select
          className="plume-agent-settings-select"
          value={config.approvalPolicy}
          disabled={busy}
          onChange={(e) =>
            void run(() => setApprovalPolicy(e.target.value as ApprovalPolicy))
          }
        >
          {APPROVAL_POLICIES.map((p) => (
            <option key={p} value={p}>
              {POLICY_LABEL[p]}
            </option>
          ))}
        </select>
      </label>

      <label className="plume-agent-settings-field plume-agent-settings-field-stacked">
        <span>File allowlist</span>
        <textarea
          className="plume-agent-settings-textarea"
          value={fileDraft}
          disabled={busy}
          rows={2}
          spellCheck={false}
          placeholder="src/&#10;docs/"
          onChange={(e) => setFileDraft(e.target.value)}
          aria-label="File allowlist, one project-relative path per line"
        />
      </label>

      <label className="plume-agent-settings-field plume-agent-settings-field-stacked">
        <span>Command allowlist</span>
        <textarea
          className="plume-agent-settings-textarea"
          value={commandDraft}
          disabled={busy}
          rows={2}
          spellCheck={false}
          placeholder="cargo test&#10;npm run build"
          onChange={(e) => setCommandDraft(e.target.value)}
          aria-label="Command allowlist, one command per line"
        />
      </label>

      <label className="plume-agent-settings-field">
        <span>Iteration cap</span>
        <input
          type="text"
          inputMode="numeric"
          className="plume-agent-settings-input"
          value={capDraft}
          disabled={busy}
          placeholder="none"
          spellCheck={false}
          onChange={(e) => setCapDraft(e.target.value)}
          aria-label={`Iteration cap, 1 to ${AGENT_MAX_ITERATION_CAP}, blank for none`}
        />
      </label>

      <div className="plume-agent-settings-actions">
        <button
          type="button"
          className="ink-button"
          disabled={busy || capInvalid}
          onClick={onApplyAllowlist}
        >
          Apply gates
        </button>
        {capInvalid ? (
          <span className="plume-agent-settings-inline-error">cap must be a number</span>
        ) : null}
      </div>

      {reasons.length > 0 ? (
        <ul className="plume-agent-settings-reasons" role="alert">
          {reasons.map((reason) => (
            <li key={reason}>{reason}</li>
          ))}
        </ul>
      ) : null}
    </section>
  );
}
