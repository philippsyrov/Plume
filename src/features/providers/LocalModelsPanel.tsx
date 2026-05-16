// D32: Local-models panel.
//
// Was a subsection of the legacy `ProviderPanel` (pre-D32). D32
// promoted it to its own ink-panel so it can be hidden via the
// column's inner-panel chip strip independently of the Providers
// panel. The data (model list + scan error) comes from the same
// `useProviderInventory` call the Providers panel reads, so
// splitting did not double the IPC load.
//
// D29's fail-soft contract is preserved: a local-model scan
// rejection surfaces in this panel as an inline error message;
// the Providers panel stays authoritative for the registry +
// reachability snapshot regardless.
//
// D41 expands each row with on-disk details (architecture, max
// context, quantization, tokenizer presence, weight counts) read
// lazily from `providers.localModelDetails` when the user clicks
// the disclosure caret.
//
// D46 adds per-row Start / Stop buttons for `mlx-folder` and
// `transformer-folder` rows — the two kinds the D40 supervisor
// accepts. Clicking Start fires `providers.startServer`; on
// success the row's model becomes the currently-selected one so
// the chat panel routes through it via the D45 handleId path.
// Single-file kinds (`gguf`, `safetensors`) render the legacy
// row layout — `mlx_lm.server` doesn't consume them, so showing
// a button that always rejects would be a lie.

import { useCallback, useState } from 'react';

import { getLocalModelDetails, type LocalModelDetails } from '../../lib/api/providers';
import type { LocalModel } from '../../lib/api/providers';
import { ipcErrorMessage, isIpcError } from '../../lib/api/errors';
import type { ProviderInventory } from './useProviderInventory';
import type { SelectedModel } from '../model-picker/useSelectedModel';
import { sameSelection } from '../model-picker/useSelectedModel';
import {
  MLX_LM_PROVIDER_ID,
  type MlxServersApi,
  type MlxServerStatus,
} from './useMlxServers';

export type LocalModelsPanelProps = {
  inventory: ProviderInventory;
  /** D46: per-modelId server lifecycle bus. */
  servers: MlxServersApi;
  /** D46: currently-selected model (read-only here). */
  selected: SelectedModel | null;
  /** D46: hand a started MLX model up to the global selection
   * state. Called on a successful `providers.startServer` so the
   * chat panel immediately routes through the new handle. */
  onSelect: (next: SelectedModel) => void;
  /** D49 (optional, defaults to `false`): the panel is mounted
   *  in the no-project chat shell. The D40 supervisor's trust
   *  gate requires a trusted open project for
   *  `providers.startServer`, and there is no such project here,
   *  so the Start button stays disabled with an honest hint
   *  instead of letting the user click into a `NeedsApproval`.
   *  Stop on already-running handles (started before the user
   *  opened no-project chat) is still allowed — that's a
   *  cleanup verb the backend doesn't gate. */
  noProject?: boolean;
};

export function LocalModelsPanel({
  inventory,
  servers,
  selected,
  onSelect,
  noProject = false,
}: LocalModelsPanelProps) {
  const { state } = inventory;

  // The panel is part of the provider-inventory load: it shares
  // the loading and panel-wide error states with `ProvidersPanel`.
  // When the inventory is loading we render a quiet status line;
  // when the inventory failed wholesale, the Providers panel
  // already shows the panel-wide error and we render nothing
  // useful here (a duplicated red message would feel like two
  // failures, not one).
  if (state.kind === 'loading') {
    return (
      <section className="plume-local-models-card ink-panel" aria-label="Local model files">
        <h3>Local models</h3>
        <p className="plume-providers-status" role="status">
          Probing local providers…
        </p>
      </section>
    );
  }
  if (state.kind === 'error') {
    // The Providers panel renders this same message — but D32 lets
    // the user hide that panel, so the error has to be readable
    // here too. Echoing it is the right trade: if both panels are
    // visible the user sees the same message twice (clear signal),
    // and if only Local models is visible the user still learns
    // why nothing loaded. Pre-D32 the two panels were one
    // ink-panel, so the message only appeared once — that's no
    // longer the right contract.
    return (
      <section className="plume-local-models-card ink-panel" aria-label="Local model files">
        <h3>Local models</h3>
        <p className="plume-providers-status plume-providers-error" role="alert">
          {state.message}
        </p>
      </section>
    );
  }

  return (
    <section className="plume-local-models-card ink-panel" aria-label="Local model files">
      <h3>Local models</h3>
      <LocalModelsBody
        models={state.localModels}
        error={state.localModelError}
        servers={servers}
        selected={selected}
        onSelect={onSelect}
        noProject={noProject}
      />
    </section>
  );
}

function LocalModelsBody({
  models,
  error,
  servers,
  selected,
  onSelect,
  noProject,
}: {
  models: LocalModel[];
  error: string | null;
  servers: MlxServersApi;
  selected: SelectedModel | null;
  onSelect: (next: SelectedModel) => void;
  noProject: boolean;
}) {
  if (error) {
    // D29 fail-soft: the scan rejected but the rest of the
    // inventory still rendered. Show the failure inline.
    return (
      <p className="plume-local-models-error" role="alert">
        Local model scan failed: {error}
      </p>
    );
  }
  if (models.length === 0) {
    return <p className="plume-local-models-empty">No local model files yet.</p>;
  }
  return (
    <ul className="plume-local-models-list" role="list">
      {models.map((model) => (
        <LocalModelRow
          key={model.id}
          model={model}
          status={servers.statusOf(model.id)}
          isSelected={sameSelection(selected, {
            providerId: MLX_LM_PROVIDER_ID,
            modelId: model.id,
          })}
          onStart={() => void handleStart(model, servers, onSelect)}
          onStop={() => void servers.stop(model.id)}
          noProject={noProject}
        />
      ))}
    </ul>
  );
}

/**
 * D46: start a server and, on success, set the global selection
 * so the chat panel routes through the new handle. The selection
 * is intentionally side-effecty (not gated on a separate Select
 * click) — the workflow we're optimizing for is "click Start →
 * open chat → type a prompt." A user who started one MLX model
 * and wants to chat against a different (still-running) one can
 * click Select on its row in `ProvidersPanel` like for any other
 * runtime.
 */
async function handleStart(
  model: LocalModel,
  servers: MlxServersApi,
  onSelect: (next: SelectedModel) => void,
): Promise<void> {
  const handle = await servers.start(model.id);
  if (!handle) return;
  onSelect({
    providerId: MLX_LM_PROVIDER_ID,
    providerDisplayName: 'MLX (Plume-managed)',
    modelId: model.id,
  });
}

/** Which local-model kinds the D40 supervisor will accept on Start. */
function isSupervisable(kind: LocalModel['kind']): boolean {
  return kind === 'mlx-folder' || kind === 'transformer-folder';
}

// D41: per-row state machine. The disclosure caret toggles
// `expanded`; on first expand we fire `providers.localModelDetails`
// and cache the result so collapsing + re-expanding doesn't re-hit
// the IPC. A fetch failure is sticky in the same way — the user can
// collapse + retry by clicking again, which re-fires.
type DetailState =
  | { kind: 'idle' }
  | { kind: 'loading' }
  | { kind: 'ready'; details: LocalModelDetails }
  | { kind: 'error'; message: string };

function LocalModelRow({
  model,
  status,
  isSelected,
  onStart,
  onStop,
  noProject,
}: {
  model: LocalModel;
  status: MlxServerStatus;
  isSelected: boolean;
  onStart: () => void;
  onStop: () => void;
  noProject: boolean;
}) {
  const [expanded, setExpanded] = useState(false);
  const [detailState, setDetailState] = useState<DetailState>({ kind: 'idle' });

  const onToggle = useCallback(async () => {
    const next = !expanded;
    setExpanded(next);
    if (!next) return;
    // First expand (or retry after an error) fires the lazy fetch.
    if (detailState.kind === 'ready' || detailState.kind === 'loading') return;
    setDetailState({ kind: 'loading' });
    try {
      const details = await getLocalModelDetails(model.id);
      setDetailState({ kind: 'ready', details });
    } catch (err: unknown) {
      const message = isIpcError(err)
        ? ipcErrorMessage(err)
        : err instanceof Error
          ? err.message
          : 'Couldn’t read local model details.';
      setDetailState({ kind: 'error', message });
    }
  }, [expanded, detailState.kind, model.id]);

  const supervisable = isSupervisable(model.kind);

  return (
    <li className="plume-local-models-row">
      <div className="plume-local-models-row-header">
        <button
          type="button"
          className="plume-local-models-row-toggle"
          onClick={onToggle}
          aria-expanded={expanded}
          aria-label={`${expanded ? 'Collapse' : 'Expand'} details for ${model.name}`}
        >
          <span className="plume-local-models-caret" aria-hidden="true">
            {expanded ? '▾' : '▸'}
          </span>
          <span className="plume-local-models-name">{model.name}</span>
          <span className="ink-badge plume-local-models-kind">
            {localModelKindLabel(model.kind)}
          </span>
          <span className="plume-local-models-size">{formatBytes(model.sizeBytes)}</span>
        </button>
        {supervisable ? (
          <MlxServerControls
            status={status}
            isSelected={isSelected}
            onStart={onStart}
            onStop={onStop}
            noProject={noProject}
          />
        ) : null}
      </div>
      {expanded ? <LocalModelDetailsBody state={detailState} /> : null}
      {status.kind === 'error' ? (
        <p className="plume-local-models-error" role="alert">
          {status.message}
        </p>
      ) : null}
    </li>
  );
}

/**
 * D46: per-row Start / Stop / running indicator. Buttons are gated
 * by the kind classifier — only `mlx-folder` / `transformer-folder`
 * reach this component (`isSupervisable` filters the rest out at
 * the row layer). Within those kinds:
 *
 *   * `idle` / `error` → Start button enabled.
 *   * `starting` / `stopping` → both disabled, status label is the
 *     live hint.
 *   * `running` → Stop enabled; "port N" badge surfaces the bound
 *     port for diagnostics ("Activity Monitor says it's listening
 *     where I expect"). Selected models also get a "selected" badge
 *     so the user knows chat is wired to this one.
 *
 * D49: when `noProject` is true, the `idle` / `error` Start button
 * renders disabled with a "open and trust a project" tooltip.
 * The D40 supervisor requires a trusted open project to spawn
 * `python -m mlx_lm server …` — surfacing that as an inline
 * disabled state is the smallest safe path while still letting
 * the user see what's installed and stop servers they started
 * elsewhere. The `running` state still offers Stop (cleanup verb,
 * not gated) and the in-flight states keep their hint labels.
 */
function MlxServerControls({
  status,
  isSelected,
  onStart,
  onStop,
  noProject,
}: {
  status: MlxServerStatus;
  isSelected: boolean;
  onStart: () => void;
  onStop: () => void;
  noProject: boolean;
}) {
  switch (status.kind) {
    case 'running':
      return (
        <div className="plume-local-models-controls">
          {isSelected ? (
            <span className="ink-badge plume-local-models-selected">selected</span>
          ) : null}
          <span
            className="plume-local-models-port"
            title={`mlx-lm bound to 127.0.0.1:${status.handle.port} (pid ${status.handle.pid})`}
          >
            port {status.handle.port}
          </span>
          <button
            type="button"
            className="ink-button plume-local-models-stop"
            onClick={onStop}
          >
            Stop
          </button>
        </div>
      );
    case 'starting':
      return (
        <div className="plume-local-models-controls">
          <span className="plume-local-models-status" role="status">
            starting…
          </span>
        </div>
      );
    case 'stopping':
      return (
        <div className="plume-local-models-controls">
          <span className="plume-local-models-status" role="status">
            stopping…
          </span>
        </div>
      );
    case 'idle':
    case 'error':
      return (
        <div className="plume-local-models-controls">
          <button
            type="button"
            className="ink-button plume-local-models-start"
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

function LocalModelDetailsBody({ state }: { state: DetailState }) {
  if (state.kind === 'loading' || state.kind === 'idle') {
    return (
      <div className="plume-local-models-details" role="status">
        Reading on-disk details…
      </div>
    );
  }
  if (state.kind === 'error') {
    return (
      <div className="plume-local-models-details plume-local-models-details-error" role="alert">
        {state.message}
      </div>
    );
  }
  const { details } = state;
  const rows: Array<[string, string]> = [];
  if (details.architecture) rows.push(['Architecture', details.architecture]);
  if (details.modelType) rows.push(['Model type', details.modelType]);
  if (details.maxContext !== null) {
    rows.push(['Max context', `${details.maxContext.toLocaleString()} tokens`]);
  }
  if (details.quantizationBits !== null && details.quantizationGroupSize !== null) {
    rows.push([
      'Quantization',
      `${details.quantizationBits}-bit · group ${details.quantizationGroupSize}`,
    ]);
  }
  rows.push(['Tokenizer', details.tokenizerPresent ? 'present' : 'missing']);
  rows.push([
    'Weights',
    `${details.weightFileCount} ${details.weightFileCount === 1 ? 'file' : 'files'} · ${formatBytes(details.weightBytesTotal)}`,
  ]);
  return (
    <dl className="plume-local-models-details">
      {rows.map(([label, value]) => (
        <div key={label} className="plume-local-models-detail-row">
          <dt>{label}</dt>
          <dd>{value}</dd>
        </div>
      ))}
    </dl>
  );
}

function localModelKindLabel(kind: LocalModel['kind']): string {
  switch (kind) {
    case 'gguf':
      return 'GGUF';
    case 'safetensors':
      return 'safetensors';
    case 'transformer-folder':
      return 'transformer folder';
    case 'mlx-folder':
      return 'MLX folder';
  }
}

function formatBytes(bytes: number): string {
  const KIB = 1024;
  const MIB = KIB * 1024;
  const GIB = MIB * 1024;
  if (bytes >= GIB) return `${(bytes / GIB).toFixed(1)} GB`;
  if (bytes >= MIB) return `${Math.round(bytes / MIB)} MB`;
  if (bytes >= KIB) return `${Math.round(bytes / KIB)} KB`;
  return `${bytes} B`;
}
