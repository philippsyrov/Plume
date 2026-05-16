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

import { useCallback, useState } from 'react';

import { getLocalModelDetails, type LocalModelDetails } from '../../lib/api/providers';
import type { LocalModel } from '../../lib/api/providers';
import { ipcErrorMessage, isIpcError } from '../../lib/api/errors';
import type { ProviderInventory } from './useProviderInventory';

export type LocalModelsPanelProps = {
  inventory: ProviderInventory;
};

export function LocalModelsPanel({ inventory }: LocalModelsPanelProps) {
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
      <LocalModelsBody models={state.localModels} error={state.localModelError} />
    </section>
  );
}

function LocalModelsBody({
  models,
  error,
}: {
  models: LocalModel[];
  error: string | null;
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
        <LocalModelRow key={model.id} model={model} />
      ))}
    </ul>
  );
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

function LocalModelRow({ model }: { model: LocalModel }) {
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

  return (
    <li className="plume-local-models-row">
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
      {expanded ? <LocalModelDetailsBody state={detailState} /> : null}
    </li>
  );
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
