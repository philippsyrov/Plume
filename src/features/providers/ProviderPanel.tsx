// Provider registry + reachability surface.
//
// One row per provider: name, runtime category, current reachability
// state, latency on success. The rows are rendered exclusively from
// IPC results — no client-side caching or guessing — so an external
// agent reading the DOM sees the same truth a human does.
//
// D1 scope: list + health. No model loading, no chat, no engines.
// Adapter-specific affordances (start MLX-LM, attach to Ollama daemon,
// pick a model) land in later slices behind the same component.

import { useCallback, useEffect, useState } from 'react';

import { ipcErrorMessage, isIpcError } from '../../lib/api/errors';
import {
  categoryLabel,
  getProvidersHealth,
  listProviders,
  reachabilityLabel,
  type ProviderHealth,
  type ProviderInfo,
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
        return (
          <li key={provider.id} className="plume-providers-row" role="listitem">
            <div className="plume-providers-name">
              <strong>{provider.displayName}</strong>
              <span className="plume-providers-category">
                {categoryLabel(provider.category)}
              </span>
            </div>
            <ReachabilityBadge state={reachability} latencyMs={health?.latencyMs ?? null} />
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

function formatError(err: unknown): string {
  if (isIpcError(err)) return ipcErrorMessage(err);
  if (err instanceof Error) return err.message;
  return 'Failed to load providers.';
}
