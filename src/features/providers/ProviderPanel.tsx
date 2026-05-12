// Provider registry + reachability surface.
//
// One row per provider: name, runtime category, current reachability
// state, latency on success, and — when the adapter contributes a
// model list — a count + names line below the reachability badge.
// The rows are rendered exclusively from IPC results — no client-side
// caching or guessing — so an external agent reading the DOM sees the
// same truth a human does.
//
// D2 scope: list + health + Ollama models. No model loading, no chat,
// no engines. Other adapters' model lists land as their HTTP probes
// land (LM Studio next).

import { useCallback, useEffect, useState } from 'react';

import { ipcErrorMessage, isIpcError } from '../../lib/api/errors';
import {
  categoryLabel,
  getProvidersHealth,
  listProviders,
  reachabilityLabel,
  type ProviderHealth,
  type ProviderInfo,
  type ProviderModel,
  type ReachabilityState,
} from '../../lib/api/providers';

type LoadState =
  | { kind: 'loading' }
  | { kind: 'ready'; providers: ProviderInfo[]; healthById: Map<string, ProviderHealth> }
  | { kind: 'error'; message: string };

export function ProviderPanel() {
  const [state, setState] = useState<LoadState>({ kind: 'loading' });
  const [refreshing, setRefreshing] = useState(false);

  const load = useCallback(async () => {
    setRefreshing(true);
    try {
      // Fire both calls in parallel. The static registry never
      // changes mid-session, but re-fetching it on refresh keeps
      // the code simple and survives a future where the registry
      // becomes config-driven.
      const [providers, health] = await Promise.all([
        listProviders(),
        getProvidersHealth(),
      ]);
      const healthById = new Map(health.map((h) => [h.id, h]));
      setState({ kind: 'ready', providers, healthById });
    } catch (err) {
      setState({ kind: 'error', message: formatError(err) });
    } finally {
      setRefreshing(false);
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

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
        <ProviderList providers={state.providers} healthById={state.healthById} />
      )}
    </section>
  );
}

type ProviderListProps = {
  providers: ProviderInfo[];
  healthById: Map<string, ProviderHealth>;
};

function ProviderList({ providers, healthById }: ProviderListProps) {
  return (
    <ul className="plume-providers-list" role="list">
      {providers.map((provider) => {
        const health = healthById.get(provider.id);
        // No health entry means the backend didn't return one for
        // this provider — render conservatively as "not configured"
        // so we never silently assert availability.
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
            {models !== null ? <ModelSummary models={models} /> : null}
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

function ModelSummary({ models }: { models: ProviderModel[] }) {
  // Distinct from `models === null` (no probe). Empty array is the
  // honest "daemon is up, has no models" signal.
  if (models.length === 0) {
    return (
      <p className="plume-providers-models plume-providers-models-empty">
        no models installed
      </p>
    );
  }
  // Show the first few names inline, then "+N more" if the list runs
  // long. The full list goes on the title attribute so a hover reveals
  // every model. Sized for the 260 px navigator column.
  const PREVIEW = 2;
  const preview = models.slice(0, PREVIEW).map((m) => m.id);
  const remaining = models.length - preview.length;
  const previewText = preview.join(', ') + (remaining > 0 ? `, +${remaining} more` : '');
  const fullList = models.map((m) => m.id).join('\n');
  const count = `${models.length} model${models.length === 1 ? '' : 's'}`;
  return (
    <p
      className="plume-providers-models"
      title={fullList}
      aria-label={`${count}: ${models.map((m) => m.id).join(', ')}`}
    >
      <span className="plume-providers-models-count">{count}</span>{' '}
      <span className="plume-providers-models-names">{previewText}</span>
    </p>
  );
}

function formatError(err: unknown): string {
  if (isIpcError(err)) return ipcErrorMessage(err);
  if (err instanceof Error) return err.message;
  return 'Failed to load providers.';
}
