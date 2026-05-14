// Provider registry + reachability + per-model truth surface + D6
// model selection.
//
// D2 added model count + names. D3 layered per-model truth on top:
// click a model row and we fire `providers.modelDetails` to fetch
// family / params / quantization / context / runtime path, plus a
// cautious fit verdict against the host's physical memory. The
// detail fetch is lazy — collapsed rows trigger no IPC.
//
// D6 adds a Select button per model row that hands a small
// `SelectedModel` snapshot up to the workspace shell. Selection is
// gated on the provider being reachable today — `available` only;
// `offline` and `not-configured` rows render the row but disable
// Select. When an Ollama model has already been expanded its fit
// verdict is in the local `details` cache and rides along with the
// selection (the workspace banner shows it). For LM Studio /
// llama.cpp models the snapshot has no fit because we have no
// per-model probe for them yet.
//
// Rows render exclusively from IPC results, so an external agent
// reading the DOM sees the same truth a human does.

import { useCallback, useEffect, useRef, useState } from 'react';

import { ipcErrorMessage, isIpcError } from '../../lib/api/errors';
import {
  categoryLabel,
  fitLabel,
  getLocalModels,
  getModelDetails,
  getProvidersHealth,
  listProviders,
  PROVIDERS_WITH_DETAILS,
  reachabilityLabel,
  type FitState,
  type LocalModel,
  type ProviderHealth,
  type ProviderInfo,
  type ProviderModel,
  type ProviderModelDetails,
  type ReachabilityState,
} from '../../lib/api/providers';
import type { SelectedModel } from '../model-picker/useSelectedModel';
import { sameSelection } from '../model-picker/useSelectedModel';

type LoadState =
  | { kind: 'loading' }
  | {
      kind: 'ready';
      providers: ProviderInfo[];
      healthById: Map<string, ProviderHealth>;
      localModels: LocalModel[];
    }
  | { kind: 'error'; message: string };

/// Per-model fetch state. The key is `${providerId}::${modelId}` so
/// two providers serving a model with the same id don't collide.
type DetailState =
  | { kind: 'idle' }
  | { kind: 'loading' }
  | { kind: 'ready'; details: ProviderModelDetails }
  | { kind: 'error'; message: string };

function detailKey(providerId: string, modelId: string): string {
  return `${providerId}::${modelId}`;
}

export type ProviderPanelProps = {
  selected: SelectedModel | null;
  onSelect: (next: SelectedModel) => void;
};

export function ProviderPanel({ selected, onSelect }: ProviderPanelProps) {
  const [state, setState] = useState<LoadState>({ kind: 'loading' });
  const [refreshing, setRefreshing] = useState(false);
  const [details, setDetails] = useState<Record<string, DetailState>>({});
  const [expanded, setExpanded] = useState<Record<string, boolean>>({});
  // Generation counter bumped on every `load()`. In-flight `runFetch`
  // calls capture the value at the moment they start and silently
  // drop their result if the generation has moved on. This avoids a
  // race where the user refreshes mid-fetch, the older detail
  // resolves with stale data, writes it back, and then a re-expand
  // sees `kind === 'ready'` and skips a fresh probe.
  const generationRef = useRef(0);

  const load = useCallback(async () => {
    const gen = ++generationRef.current;
    setRefreshing(true);
    try {
      const [providers, health, localModels] = await Promise.all([
        listProviders(),
        getProvidersHealth(),
        getLocalModels(),
      ]);
      if (gen !== generationRef.current) return;
      const healthById = new Map(health.map((h) => [h.id, h]));
      setState({ kind: 'ready', providers, healthById, localModels });
      // Clear cached detail state on refresh — a fresh probe could
      // have replaced models, and the user expects the details panel
      // to reflect that.
      setDetails({});
      setExpanded({});
    } catch (err) {
      if (gen !== generationRef.current) return;
      setState({ kind: 'error', message: formatError(err) });
    } finally {
      if (gen === generationRef.current) {
        setRefreshing(false);
      }
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  const onToggleModel = useCallback(
    (providerId: string, modelId: string) => {
      const key = detailKey(providerId, modelId);
      setExpanded((prev) => {
        const next = { ...prev, [key]: !prev[key] };
        if (next[key]) {
          // Lazy fetch only on first expand. Capture the generation
          // before kicking the IPC off so a later refresh can void
          // this fetch's result.
          const gen = generationRef.current;
          setDetails((d) => {
            if (d[key]?.kind === 'ready' || d[key]?.kind === 'loading') return d;
            void runFetch(providerId, modelId, gen, generationRef, setDetails);
            return { ...d, [key]: { kind: 'loading' } };
          });
        }
        return next;
      });
    },
    [],
  );

  return (
    <section className="plume-providers ink-panel" aria-label="Local model providers">
      <header className="plume-providers-header">
        <h3>Providers</h3>
        <button
          type="button"
          className="ink-button"
          onClick={() => void load()}
          disabled={refreshing}
          aria-label="Refresh provider reachability"
        >
          {refreshing ? 'Probing…' : 'Refresh'}
        </button>
      </header>

      {state.kind === 'loading' ? (
        <p className="plume-providers-status" role="status">
          Probing local providers…
        </p>
      ) : state.kind === 'error' ? (
        <p className="plume-providers-status plume-providers-error" role="alert">
          {state.message}
        </p>
      ) : (
        <>
          <ProviderList
            providers={state.providers}
            healthById={state.healthById}
            details={details}
            expanded={expanded}
            onToggleModel={onToggleModel}
            selected={selected}
            onSelect={onSelect}
          />
          <LocalModels models={state.localModels} />
        </>
      )}
    </section>
  );
}

async function runFetch(
  providerId: string,
  modelId: string,
  gen: number,
  generationRef: React.RefObject<number>,
  setDetails: React.Dispatch<React.SetStateAction<Record<string, DetailState>>>,
) {
  const key = detailKey(providerId, modelId);
  try {
    const result = await getModelDetails(providerId, modelId);
    if (gen !== generationRef.current) return;
    setDetails((d) => ({ ...d, [key]: { kind: 'ready', details: result } }));
  } catch (err) {
    if (gen !== generationRef.current) return;
    setDetails((d) => ({ ...d, [key]: { kind: 'error', message: formatError(err) } }));
  }
}

type ProviderListProps = {
  providers: ProviderInfo[];
  healthById: Map<string, ProviderHealth>;
  details: Record<string, DetailState>;
  expanded: Record<string, boolean>;
  onToggleModel: (providerId: string, modelId: string) => void;
  selected: SelectedModel | null;
  onSelect: (next: SelectedModel) => void;
};

function ProviderList({
  providers,
  healthById,
  details,
  expanded,
  onToggleModel,
  selected,
  onSelect,
}: ProviderListProps) {
  return (
    <ul className="plume-providers-list" role="list">
      {providers.map((provider) => {
        const health = healthById.get(provider.id);
        const reachability: ReachabilityState = health?.state ?? 'not-configured';
        const models = health?.models ?? null;
        return (
          <li key={provider.id} className="plume-providers-row" role="listitem">
            <div className="plume-providers-row-top">
              <div className="plume-providers-name">
                <strong>{provider.displayName}</strong>
                <span className="plume-providers-category">
                  {categoryLabel(provider.category)}
                </span>
              </div>
              <ReachabilityBadge state={reachability} latencyMs={health?.latencyMs ?? null} />
            </div>
            {models !== null ? (
              <ModelSummary
                provider={provider}
                reachability={reachability}
                models={models}
                details={details}
                expanded={expanded}
                onToggle={onToggleModel}
                selected={selected}
                onSelect={onSelect}
              />
            ) : null}
          </li>
        );
      })}
    </ul>
  );
}

type ReachabilityBadgeProps = {
  state: ReachabilityState;
  latencyMs: number | null;
};

function ReachabilityBadge({ state, latencyMs }: ReachabilityBadgeProps) {
  const label = reachabilityLabel(state);
  const className = `ink-badge plume-reachability plume-reachability-${state}`;
  if (state === 'available' && latencyMs !== null) {
    return (
      <span className={className} aria-label={`available, ${latencyMs} ms`}>
        {label} <span className="plume-providers-latency">{latencyMs} ms</span>
      </span>
    );
  }
  return <span className={className}>{label}</span>;
}

type ModelSummaryProps = {
  provider: ProviderInfo;
  reachability: ReachabilityState;
  models: ProviderModel[];
  details: Record<string, DetailState>;
  expanded: Record<string, boolean>;
  onToggle: (providerId: string, modelId: string) => void;
  selected: SelectedModel | null;
  onSelect: (next: SelectedModel) => void;
};

function ModelSummary({
  provider,
  reachability,
  models,
  details,
  expanded,
  onToggle,
  selected,
  onSelect,
}: ModelSummaryProps) {
  if (models.length === 0) {
    return (
      <p className="plume-providers-models plume-providers-models-empty">
        runtime reports no models
      </p>
    );
  }
  // Some providers (Ollama) have a backing `providers.modelDetails`
  // probe; others (LM Studio, llama.cpp) just expose `/v1/models`
  // with no per-model endpoint. Hide the expand caret for the
  // latter so users don't click into a guaranteed `BadArgument`.
  const hasDetailProbe = PROVIDERS_WITH_DETAILS.includes(provider.id);
  // D6: selection is only legal when the runtime actually answered
  // the probe (`available`). Offline/not-configured rows render the
  // list (because we kept the last-known list visible) but Select
  // stays disabled — picking a model from an offline runtime would
  // be a fake selection.
  const canSelect = reachability === 'available';
  const count = `${models.length} model${models.length === 1 ? '' : 's'}`;
  return (
    <div className="plume-providers-models">
      <p className="plume-providers-models-count">{count}</p>
      <ul className="plume-model-list" role="list">
        {models.map((m) => {
          const key = detailKey(provider.id, m.id);
          const isOpen = !!expanded[key];
          const state = details[key];
          const isSelected = sameSelection(selected, { providerId: provider.id, modelId: m.id });
          // Capture the fit verdict if it's already in the local
          // detail cache — D6 doesn't fire a fresh probe just to
          // decorate the selection.
          const cachedFit =
            state?.kind === 'ready' ? state.details.fit.state : undefined;
          return (
            <li
              key={m.id}
              className={`plume-model-item${isSelected ? ' plume-model-item-selected' : ''}`}
            >
              <div className="plume-model-row">
                {hasDetailProbe ? (
                  <button
                    type="button"
                    className={`plume-model-toggle${isOpen ? ' plume-model-toggle-open' : ''}`}
                    onClick={() => onToggle(provider.id, m.id)}
                    aria-expanded={isOpen}
                    aria-controls={`${key}-detail`}
                  >
                    <span className="plume-model-caret" aria-hidden>
                      {isOpen ? '▾' : '▸'}
                    </span>
                    <span className="plume-model-name">{m.id}</span>
                    {m.sizeBytes !== null ? (
                      <span className="plume-model-size">{formatBytes(m.sizeBytes)}</span>
                    ) : null}
                  </button>
                ) : (
                  <div className="plume-model-toggle plume-model-toggle-static" role="text">
                    <span className="plume-model-caret" aria-hidden>
                      ·
                    </span>
                    <span className="plume-model-name">{m.id}</span>
                    {m.sizeBytes !== null ? (
                      <span className="plume-model-size">{formatBytes(m.sizeBytes)}</span>
                    ) : null}
                  </div>
                )}
                <SelectModelButton
                  provider={provider}
                  modelId={m.id}
                  canSelect={canSelect}
                  reachability={reachability}
                  isSelected={isSelected}
                  cachedFit={cachedFit}
                  onSelect={onSelect}
                />
              </div>
              {hasDetailProbe && isOpen ? (
                <div
                  id={`${key}-detail`}
                  className="plume-model-detail"
                  role="region"
                  aria-label={`Details for ${m.id}`}
                >
                  <ModelDetailBody state={state} />
                </div>
              ) : null}
            </li>
          );
        })}
      </ul>
    </div>
  );
}

type SelectModelButtonProps = {
  provider: ProviderInfo;
  modelId: string;
  canSelect: boolean;
  reachability: ReachabilityState;
  isSelected: boolean;
  cachedFit: FitState | undefined;
  onSelect: (next: SelectedModel) => void;
};

function SelectModelButton({
  provider,
  modelId,
  canSelect,
  reachability,
  isSelected,
  cachedFit,
  onSelect,
}: SelectModelButtonProps) {
  if (isSelected) {
    return (
      <span
        className="ink-badge plume-model-selected-badge"
        aria-label={`Selected: ${provider.displayName} ${modelId}`}
      >
        ✓ selected
      </span>
    );
  }
  // Disabled rows still render the button so the row layout stays
  // stable; an offline reachability surfaces via the `title` and the
  // disabled attribute, which screen readers announce.
  const title = canSelect
    ? `Select ${provider.displayName} ${modelId}`
    : `Cannot select — provider is ${reachabilityLabel(reachability)}`;
  return (
    <button
      type="button"
      className="ink-button plume-model-select-button"
      disabled={!canSelect}
      title={title}
      aria-label={title}
      onClick={() => {
        // `exactOptionalPropertyTypes` rejects an explicit
        // `fit: undefined` against `fit?: FitState`; build the
        // snapshot conditionally so the optional field is either
        // present with a real value or absent entirely.
        const snapshot: SelectedModel =
          cachedFit !== undefined
            ? {
                providerId: provider.id,
                providerDisplayName: provider.displayName,
                modelId,
                fit: cachedFit,
              }
            : {
                providerId: provider.id,
                providerDisplayName: provider.displayName,
                modelId,
              };
        onSelect(snapshot);
      }}
    >
      Select
    </button>
  );
}

function ModelDetailBody({ state }: { state: DetailState | undefined }) {
  if (!state || state.kind === 'idle' || state.kind === 'loading') {
    return (
      <p className="plume-model-detail-status" role="status">
        Reading model info…
      </p>
    );
  }
  if (state.kind === 'error') {
    return (
      <p className="plume-model-detail-status plume-model-detail-error" role="alert">
        {state.message}
      </p>
    );
  }
  const { details, fit, runtimePath } = state.details;
  return (
    <dl className="plume-model-detail-grid">
      {details ? (
        <>
          {details.family ? (
            <>
              <dt>family</dt>
              <dd>{details.family}</dd>
            </>
          ) : null}
          {details.parameterSize ? (
            <>
              <dt>params</dt>
              <dd>{details.parameterSize}</dd>
            </>
          ) : null}
          {details.quantization ? (
            <>
              <dt>quant</dt>
              <dd>{details.quantization}</dd>
            </>
          ) : null}
          {details.contextLength ? (
            <>
              <dt>context</dt>
              <dd>{details.contextLength.toLocaleString()} tok</dd>
            </>
          ) : null}
        </>
      ) : (
        <>
          <dt>info</dt>
          <dd>not available</dd>
        </>
      )}
      {runtimePath ? (
        <>
          <dt>runtime</dt>
          <dd>{runtimePath}</dd>
        </>
      ) : null}
      <dt>fit</dt>
      <dd>
        <FitBadge state={fit.state} />
        <p className="plume-model-fit-rationale">{fit.rationale}</p>
      </dd>
    </dl>
  );
}

function FitBadge({ state }: { state: FitState }) {
  return <span className={`ink-badge plume-fit plume-fit-${state}`}>{fitLabel(state)}</span>;
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

function LocalModels({ models }: { models: LocalModel[] }) {
  return (
    <section className="plume-local-models" aria-label="Local model files">
      <h4>Local models</h4>
      {models.length === 0 ? (
        <p className="plume-local-models-empty">No local model files yet.</p>
      ) : (
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
      )}
    </section>
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

function formatError(err: unknown): string {
  if (isIpcError(err)) return ipcErrorMessage(err);
  if (err instanceof Error) return err.message;
  return 'Failed to load providers.';
}
