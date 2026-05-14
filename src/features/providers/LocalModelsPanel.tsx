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

import type { LocalModel } from '../../lib/api/providers';
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
    // Don't echo the panel-wide error twice. Keep the card visible
    // so the chip's filled state still has a target; show a quiet
    // placeholder.
    return (
      <section className="plume-local-models-card ink-panel" aria-label="Local model files">
        <h3>Local models</h3>
        <p className="plume-providers-status" role="status">
          Local model scan paused — see Providers panel for details.
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
        <li key={model.id} className="plume-local-models-row">
          <span className="plume-local-models-name">{model.name}</span>
          <span className="ink-badge plume-local-models-kind">
            {localModelKindLabel(model.kind)}
          </span>
          <span className="plume-local-models-size">{formatBytes(model.sizeBytes)}</span>
        </li>
      ))}
    </ul>
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
