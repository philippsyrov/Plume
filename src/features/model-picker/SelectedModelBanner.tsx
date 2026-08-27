// D6 + D89: selected-model summary for the current chat.
//
// D6 shipped this as a read-only banner back when chat wasn't wired. D89
// rescues it: chat is real now, so the copy drops the "no chat, no
// loading happens yet" hedging, and an MLX selection gains inline
// Start / Stop / running controls (driven by the same `useMlxServers`
// bus the Local models panel uses). Selecting a model still happens in
// the Providers / Local models panels; this reports what the user is
// chatting with and whether it is running.
//
// States:
//   * empty   — nothing selected. Point at the left panels.
//   * ready   — provider · model id + fit chip + Clear, and for a
//               Plume-managed MLX model the Start/Stop affordance.
//
// What it deliberately still does NOT do: cross-check the latest health
// snapshot. If a provider goes offline after selection the banner keeps
// the picked text; the Providers panel stays the source of truth for
// reachability.

import { fitLabel, type FitState } from '../../lib/api/providers';
import type { SelectedModel } from './useSelectedModel';
import {
  MLX_LM_PROVIDER_ID,
  type MlxServersApi,
  type MlxServerStatus,
} from '../providers/useMlxServers';

export type SelectedModelBannerProps = {
  selected: SelectedModel | null;
  onClear: () => void;
  /** D89: lifecycle bus for Plume-managed MLX servers. Lets the banner
   *  Start/Stop the selected MLX model in place. Optional so older test
   *  scaffolds that mount the banner without a bus still render. */
  mlxServers?: MlxServersApi;
  /** D89 review fix: `true` in the no-project chat shell. The D40
   *  supervisor gates `providers.startServer` on a trusted open project,
   *  so — matching the Local models panel's D49 rule — Start renders
   *  disabled with an "open a project" hint here. Stop / running stay
   *  live (Stop is an ungated cleanup verb for a server the user started
   *  in a trusted session before switching to chat-only). */
  noProject?: boolean;
};

export function SelectedModelBanner({
  selected,
  onClear,
  mlxServers,
  noProject = false,
}: SelectedModelBannerProps) {
  const isMlx = selected?.providerId === MLX_LM_PROVIDER_ID;
  return (
    <section
      className="plume-agent-selection ink-panel"
      aria-label="Selected model"
      aria-live="polite"
    >
      <div className="plume-agent-selection-head">
        <span className="plume-agent-selection-label">Selected model</span>
        {selected !== null ? (
          <button
            type="button"
            className="ink-button plume-agent-selection-clear"
            onClick={onClear}
            aria-label={`Clear selected model ${selected.providerDisplayName} ${selected.modelId}`}
          >
            Clear
          </button>
        ) : null}
      </div>
      {selected === null ? (
        <p className="plume-agent-selection-empty">
          No model selected. Pick one from the Providers or Local models panel
          on the left.
        </p>
      ) : (
        <div className="plume-agent-selection-body">
          <p className="plume-agent-selection-id">
            <span className="plume-agent-selection-provider">
              {selected.providerDisplayName}
            </span>
            <span className="plume-agent-selection-sep" aria-hidden>
              ·
            </span>
            <span className="plume-agent-selection-model">{selected.modelId}</span>
            {selected.fit ? <FitChip fit={selected.fit} /> : null}
          </p>
          {isMlx && mlxServers ? (
            <MlxRunControls
              status={mlxServers.statusOf(selected.modelId)}
              onStart={() => void mlxServers.start(selected.modelId)}
              onStop={() => void mlxServers.stop(selected.modelId).catch(() => {})}
              noProject={noProject}
            />
          ) : null}
        </div>
      )}
    </section>
  );
}

/**
 * D89: compact Start / Stop / running indicator for the selected MLX
 * model, shown inline in the banner. Mirrors the per-row controls in
 * `LocalModelsPanel` but without the row's selection/no-project nuance —
 * here the model is, by definition, the selected one. Reusing the same
 * `MlxServerStatus` keeps the two surfaces in lockstep.
 */
function MlxRunControls({
  status,
  onStart,
  onStop,
  noProject,
}: {
  status: MlxServerStatus;
  onStart: () => void;
  onStop: () => void;
  noProject: boolean;
}) {
  switch (status.kind) {
    case 'running':
      return (
        <div className="plume-agent-selection-run">
          <span
            className="plume-agent-selection-port"
            title={`mlx-lm bound to 127.0.0.1:${status.handle.port} (pid ${status.handle.pid})`}
          >
            running · port {status.handle.port}
          </span>
          <button
            type="button"
            className="ink-button plume-agent-selection-stop"
            onClick={onStop}
          >
            Stop
          </button>
        </div>
      );
    case 'starting':
      return (
        <div className="plume-agent-selection-run">
          <span className="plume-agent-selection-status" role="status">
            starting…
          </span>
        </div>
      );
    case 'stopping':
      return (
        <div className="plume-agent-selection-run">
          <span className="plume-agent-selection-status" role="status">
            stopping…
          </span>
        </div>
      );
    case 'idle':
    case 'error':
      return (
        <div className="plume-agent-selection-run">
          {status.kind === 'error' ? (
            <span
              className="plume-agent-selection-status plume-agent-selection-status-error"
              role="alert"
            >
              {status.message}
            </span>
          ) : null}
          <button
            type="button"
            className="ink-button plume-agent-selection-start"
            onClick={onStart}
            disabled={noProject}
            title={
              noProject
                ? 'Open and trust a project to start Plume-managed runtimes.'
                : undefined
            }
            aria-disabled={noProject || undefined}
          >
            Start
          </button>
        </div>
      );
  }
}

function FitChip({ fit }: { fit: FitState }) {
  return (
    <span
      className={`ink-badge plume-fit plume-fit-${fit}`}
      aria-label={`Fit verdict: ${fitLabel(fit)}`}
    >
      {fitLabel(fit)}
    </span>
  );
}
