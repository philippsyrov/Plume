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

import {
  getLocalModelDetails,
  getServerDiagnostics,
  type LocalModelDetails,
  type ServerDiagnostics,
} from '../../lib/api/providers';
import type { LocalModel, LocalModelSource } from '../../lib/api/providers';
import { ipcErrorMessage, isIpcError } from '../../lib/api/errors';
import { formatBytes } from '../../lib/format';
import { detectMlxLogHint } from './mlxLogPatterns';
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
          onStop={() => void servers.stop(model.id).catch(() => {})}
          onUse={() =>
            onSelect({
              providerId: MLX_LM_PROVIDER_ID,
              providerDisplayName: 'MLX (Plume-managed)',
              modelId: model.id,
            })
          }
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
  onUse,
  noProject,
}: {
  model: LocalModel;
  status: MlxServerStatus;
  isSelected: boolean;
  onStart: () => void;
  onStop: () => void;
  onUse: () => void;
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
      {/* D87: the actionable line is just caret + name + the Start/Stop
          (or running) controls, kept on one nowrap row so a selected +
          running model never wraps its badges over the buttons. The
          descriptive badges (kind / source / size) drop to a quiet meta
          line below, where they can wrap freely without disturbing the
          controls. */}
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
        </button>
        {supervisable ? (
          <MlxServerControls
            status={status}
            isSelected={isSelected}
            onStart={onStart}
            onStop={onStop}
            onUse={onUse}
            noProject={noProject}
          />
        ) : null}
      </div>
      <div className="plume-local-models-meta">
        <span className="ink-badge plume-local-models-kind">
          {localModelKindLabel(model.kind)}
        </span>
        {/* D51: source badge. Names where Plume found the model on disk —
            secondary to the kind classifier. Always rendered so the panel
            never hides where a model came from. */}
        <span
          className="ink-badge plume-local-models-source"
          title={localModelSourceTitle(model.source)}
        >
          {localModelSourceLabel(model.source)}
        </span>
        <span className="plume-local-models-size">{formatBytes(model.sizeBytes)}</span>
      </div>
      {expanded ? <LocalModelDetailsBody state={detailState} model={model} /> : null}
      {expanded && status.kind === 'running' ? (
        <DiagnosticsDisclosure handleId={status.handle.id} />
      ) : null}
      {status.kind === 'error' ? (
        <p className="plume-local-models-error" role="alert">
          {status.message}
        </p>
      ) : null}
    </li>
  );
}

/**
 * D52: small "Logs" disclosure on running rows. Fires
 * `providers.serverDiagnostics(handleId)` lazily on first expand and
 * caches the snapshot until the user clicks Refresh. Read-only — the
 * verb never mutates the registry.
 *
 * The disclosure is intentionally simple: no auto-polling. The
 * dominant question this answers is "is mlx-lm OK?" and "what
 * happened during loading?" — both are answered by an on-demand
 * snapshot. A live tail would complicate the supervisor's lock
 * pattern (each poll re-acquires the registry + ring buffer mutex)
 * for a usability win the panel doesn't need.
 */
type DiagnosticsDisclosureState =
  | { kind: 'idle' }
  | { kind: 'loading' }
  | { kind: 'ready'; snapshot: ServerDiagnostics }
  | { kind: 'error'; message: string };

function DiagnosticsDisclosure({ handleId }: { handleId: string }) {
  const [expanded, setExpanded] = useState(false);
  const [state, setState] = useState<DiagnosticsDisclosureState>({ kind: 'idle' });

  const fetchSnapshot = useCallback(async () => {
    setState({ kind: 'loading' });
    try {
      const snapshot = await getServerDiagnostics(handleId);
      setState({ kind: 'ready', snapshot });
    } catch (err: unknown) {
      const message = isIpcError(err)
        ? ipcErrorMessage(err)
        : err instanceof Error
          ? err.message
          : "Couldn't read server diagnostics.";
      setState({ kind: 'error', message });
    }
  }, [handleId]);

  const onToggle = useCallback(() => {
    const next = !expanded;
    setExpanded(next);
    if (next && state.kind === 'idle') {
      void fetchSnapshot();
    }
  }, [expanded, state.kind, fetchSnapshot]);

  return (
    <div className="plume-local-models-diagnostics">
      <button
        type="button"
        className="plume-local-models-diagnostics-toggle"
        onClick={onToggle}
        aria-expanded={expanded}
      >
        <span className="plume-local-models-caret" aria-hidden="true">
          {expanded ? '▾' : '▸'}
        </span>
        Logs &amp; diagnostics
      </button>
      {expanded ? <DiagnosticsBody state={state} onRefresh={fetchSnapshot} /> : null}
    </div>
  );
}

function DiagnosticsBody({
  state,
  onRefresh,
}: {
  state: DiagnosticsDisclosureState;
  onRefresh: () => void;
}) {
  if (state.kind === 'idle' || state.kind === 'loading') {
    return (
      <div className="plume-local-models-diagnostics-body" role="status">
        Reading server diagnostics…
      </div>
    );
  }
  if (state.kind === 'error') {
    return (
      <div className="plume-local-models-diagnostics-body" role="alert">
        <p className="plume-local-models-error">{state.message}</p>
        <button
          type="button"
          className="plume-local-models-diagnostics-refresh"
          onClick={onRefresh}
        >
          Retry
        </button>
      </div>
    );
  }
  const { snapshot } = state;
  const truncated = snapshot.logBytes >= snapshot.logCapacity;
  // D57: heuristic-classify the log tail. The detector returns null
  // when no pattern fires, in which case we render nothing — the raw
  // log is the source of truth and the hint is purely additive.
  const hint = detectMlxLogHint(snapshot.logTail);
  return (
    <div className="plume-local-models-diagnostics-body">
      <dl className="plume-local-models-diagnostics-meta">
        <div className="plume-local-models-detail-row">
          <dt>Port</dt>
          <dd>{snapshot.port}</dd>
        </div>
        <div className="plume-local-models-detail-row">
          <dt>PID</dt>
          <dd>{snapshot.pid}</dd>
        </div>
        <div className="plume-local-models-detail-row">
          <dt>Uptime</dt>
          <dd>{formatUptime(snapshot.uptimeMs)}</dd>
        </div>
        <div className="plume-local-models-detail-row">
          <dt>Model</dt>
          <dd>{snapshot.modelLabel}</dd>
        </div>
        <div className="plume-local-models-detail-row">
          <dt>Log buffer</dt>
          <dd>
            {snapshot.logBytes} / {snapshot.logCapacity} bytes
            {truncated ? ' (older output dropped)' : ''}
          </dd>
        </div>
      </dl>
      {hint ? (
        <div
          className="plume-local-models-diagnostics-hint"
          role="status"
          data-hint-kind={hint.kind}
        >
          <p className="plume-local-models-diagnostics-hint-label">{hint.label}</p>
          <p className="plume-local-models-diagnostics-hint-suggestion">
            {hint.suggestion}
          </p>
        </div>
      ) : null}
      <pre className="plume-local-models-diagnostics-log" aria-label="Recent server output">
        {snapshot.logTail || '(no output captured yet)'}
      </pre>
      <button
        type="button"
        className="plume-local-models-diagnostics-refresh"
        onClick={onRefresh}
      >
        Refresh
      </button>
    </div>
  );
}

/**
 * D52: format `uptimeMs` as `Hh Mm Ss` (omitting zero leading parts).
 * Keeps the diagnostics row compact when the server has only been
 * running for seconds.
 */
function formatUptime(uptimeMs: number): string {
  const total = Math.max(0, Math.floor(uptimeMs / 1000));
  const hours = Math.floor(total / 3600);
  const minutes = Math.floor((total % 3600) / 60);
  const seconds = total % 60;
  if (hours > 0) return `${hours}h ${minutes}m ${seconds}s`;
  if (minutes > 0) return `${minutes}m ${seconds}s`;
  return `${seconds}s`;
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
  onUse,
  noProject,
}: {
  status: MlxServerStatus;
  isSelected: boolean;
  onStart: () => void;
  onStop: () => void;
  onUse: () => void;
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
          {noProject && !isSelected ? (
            <button
              type="button"
              className="ink-button plume-local-models-use"
              onClick={onUse}
            >
              Use in chat
            </button>
          ) : null}
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

function LocalModelDetailsBody({ state, model }: { state: DetailState; model: LocalModel }) {
  // D51: the source label + path are surfaced regardless of the
  // details-fetch state. They come from the inventory row itself,
  // so they are honest even while `providers.localModelDetails` is
  // still loading or has just errored. This matters most for
  // external sources — the user opens the disclosure because they
  // want to confirm where Plume found this folder before clicking
  // Start.
  const sourceRow: [string, string] = [
    'Source',
    `${localModelSourceLabel(model.source)} · ${displayPath(model.path)}`,
  ];
  if (state.kind === 'loading' || state.kind === 'idle') {
    return (
      <dl className="plume-local-models-details" aria-busy="true">
        <SourceDetailRow row={sourceRow} />
        <div className="plume-local-models-detail-row" role="status">
          <dt>Details</dt>
          <dd>Reading on-disk details…</dd>
        </div>
      </dl>
    );
  }
  if (state.kind === 'error') {
    return (
      <dl className="plume-local-models-details">
        <SourceDetailRow row={sourceRow} />
        <div
          className="plume-local-models-detail-row plume-local-models-details-error"
          role="alert"
        >
          <dt>Details</dt>
          <dd>{state.message}</dd>
        </div>
      </dl>
    );
  }
  const { details } = state;
  const rows: Array<[string, string]> = [sourceRow];
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

function SourceDetailRow({ row: [label, value] }: { row: [string, string] }) {
  return (
    <div className="plume-local-models-detail-row">
      <dt>{label}</dt>
      <dd>{value}</dd>
    </div>
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

/**
 * D51: compact, friendly label for a `LocalModelSource`. Rendered in
 * the row's source badge. Kept short so the row header doesn't wrap
 * in narrow columns.
 */
function localModelSourceLabel(source: LocalModelSource): string {
  switch (source) {
    case 'plume-model-dir':
      return 'Plume';
    case 'locally-ai-cache':
      return 'Locally AI';
    case 'lm-studio-cache':
      return 'LM Studio';
  }
}

/**
 * D51: hover-title for the source badge, naming the on-disk root the
 * source represents. The user often wants to know which folder a row
 * actually came from before starting; the badge's hint answers that
 * without making them expand the row.
 */
function localModelSourceTitle(source: LocalModelSource): string {
  switch (source) {
    case 'plume-model-dir':
      return "Plume's own model directory ($PLUME_MODEL_DIR or <project>/plume-models)";
    case 'locally-ai-cache':
      return "Locally AI's HuggingFace cache (read-only)";
    case 'lm-studio-cache':
      return "LM Studio's models tree (read-only)";
  }
}

/**
 * D51: shorten an absolute path for display in the details panel.
 * Substitutes `~/` for the user's home dir when the path lives under
 * it (typical for Locally AI and LM Studio caches). Plume can't know
 * `$HOME` from the renderer without an IPC call, so we use a small
 * heuristic on the absolute prefix; the title attribute still carries
 * the full path so a curious user can read it.
 */
function displayPath(absolute: string): string {
  // navigator.userAgent doesn't carry $HOME; sniff the absolute path
  // for `/Users/<name>/` (macOS) or `/home/<name>/` (Linux) and fold
  // that prefix into `~/`. Best-effort UI cosmetic — the full path
  // is preserved via the title attribute.
  const macHome = /^\/Users\/[^/]+\//.exec(absolute);
  const linuxHome = /^\/home\/[^/]+\//.exec(absolute);
  const match = macHome ?? linuxHome;
  if (match) {
    return `~/${absolute.slice(match[0].length)}`;
  }
  return absolute;
}
