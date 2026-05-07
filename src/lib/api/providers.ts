// Typed wrapper for provider IPC. Mirrors `docs/IPC_CONTRACT.md` §
// providers and `docs/MODEL_PROVIDERS.md § Runtime categories`.
//
// `providers.list` is the static registry; `providers.health` is the
// dynamic reachability snapshot. Both are global — they don't gate
// on the open project.

import { invokeIpc } from './ipc';

export type ProviderId = string;

export type ProviderCategory = 'plume-managed' | 'connected';

export type ToolCallSupport = 'none' | 'prompt-only' | 'json-mode' | 'native';

export type ProviderCapabilities = {
  streaming: boolean;
  toolCalls: ToolCallSupport;
  vision: boolean;
  embeddings: boolean;
  /** 0 means "unknown" per the contract, not "no context window". */
  maxContext: number;
  ownedProcess: boolean;
};

export type ProviderInfo = {
  id: ProviderId;
  displayName: string;
  category: ProviderCategory;
  capabilities: ProviderCapabilities;
};

/**
 * - `available`: a TCP listener answered the probe.
 * - `offline`: probed and got silence (timeout / refused).
 * - `not-configured`: Plume does not yet know how to start or
 *   contact this provider. Today this is the default for
 *   Plume-managed runtimes; it is *not* an error.
 */
export type ReachabilityState = 'available' | 'offline' | 'not-configured';

export type ProviderHealth = {
  id: ProviderId;
  state: ReachabilityState;
  /** TCP latency in ms; null for non-probed states. */
  latencyMs: number | null;
  /** Unix epoch ms when the snapshot was taken. */
  probedAtMs: number;
};

type EmptyPayload = Record<string, never>;

export function listProviders(): Promise<ProviderInfo[]> {
  return invokeIpc<EmptyPayload, ProviderInfo[]>('providers_list', {});
}

export function getProvidersHealth(): Promise<ProviderHealth[]> {
  return invokeIpc<EmptyPayload, ProviderHealth[]>('providers_health', {});
}

/** Render-friendly text for a reachability state. */
export function reachabilityLabel(state: ReachabilityState): string {
  switch (state) {
    case 'available':
      return 'available';
    case 'offline':
      return 'offline';
    case 'not-configured':
      return 'not configured';
  }
}

/** Render-friendly text for a category. */
export function categoryLabel(category: ProviderCategory): string {
  switch (category) {
    case 'plume-managed':
      return 'Plume-managed';
    case 'connected':
      return 'connected';
  }
}
