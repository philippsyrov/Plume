// Provider registry + reachability + per-model truth surface.
//
// D2 added model count + names. D3 layers per-model truth on top:
// click a model row and we fire `providers.modelDetails` to fetch
// family / params / quantization / context / runtime path, plus a
// cautious fit verdict against the host's physical memory. The
// detail fetch is lazy — collapsed rows trigger no IPC.
//
// Rows render exclusively from IPC results, so an external agent
// reading the DOM sees the same truth a human does.

import { useCallback, useEffect, useRef, useState } from 'react';

import { ipcErrorMessage, isIpcError } from '../../lib/api/errors';
import {
  categoryLabel,
  fitLabel,
  getModelDetails,
  getProvidersHealth,
  listProviders,
  reachabilityLabel,
  type FitState,
  type ProviderHealth,
  type ProviderInfo,
  type ProviderModel,
  type ProviderModelDetails,
  type ReachabilityState,
} from '../../lib/api/providers';

type LoadState =
  | { kind: 'loading' }
  | { kind: 'ready'; providers: ProviderInfo[]; healthById: Map<string, ProviderHealth> }
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

export function ProviderPanel() {
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
      const [providers, health] = await Promise.all([
        listProviders(),
        getProvidersHealth(),
      ]);
      if (gen !== generationRef.current) return;
      const healthById = new Map(health.map((h) => [h.id, h]));
      setState({ kind: 'ready', providers, healthById });
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
        <ProviderList
          providers={state.providers}
          healthById={state.healthById}
          details={details}
          expanded={expanded}
          onToggleModel={onToggleModel}
        />
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
};

function ProviderList({
  providers,
  healthById,
  details,
  expanded,
  onToggleModel,
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
                providerId={provider.id}
                models={models}
                details={details}
                expanded={expanded}
                onToggle={onToggleModel}
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
  providerId: string;
  models: ProviderModel[];
  details: Record<string, DetailState>;
  expanded: Record<string, boolean>;
  onToggle: (providerId: string, modelId: string) => void;
};

function ModelSummary({ providerId, models, details, expanded, onToggle }: ModelSummaryProps) {
  if (models.length === 0) {
    return (
      <p className="plume-providers-models plume-providers-models-empty">
        no models installed
      </p>
    );
  }
  const count = `${models.length} model${models.length === 1 ? '' : 's'}`;
  return (
    <div className="plume-providers-models">
      <p className="plume-providers-models-count">{count}</p>
      <ul className="plume-model-list" role="list">
        {models.map((m) => {
          const key = detailKey(providerId, m.id);
          const isOpen = !!expanded[key];
          const state = details[key];
          return (
            <li key={m.id} className="plume-model-item">
              <button
                type="button"
                className={`plume-model-toggle${isOpen ? ' plume-model-toggle-open' : ''}`}
                onClick={() => onToggle(providerId, m.id)}
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
              {isOpen ? (
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

function formatError(err: unknown): string {
  if (isIpcError(err)) return ipcErrorMessage(err);
  if (err instanceof Error) return err.message;
  return 'Failed to load providers.';
}
