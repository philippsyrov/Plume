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

export type ProviderModel = {
  /** Adapter-specific opaque id. For Ollama this is the tag string. */
  id: string;
  /** Raw on-disk size; null if the runtime does not report it. */
  sizeBytes: number | null;
};

export type ProviderHealth = {
  id: ProviderId;
  state: ReachabilityState;
  /** TCP latency in ms; null for non-probed states. */
  latencyMs: number | null;
  /** Unix epoch ms when the snapshot was taken. */
  probedAtMs: number;
  /**
   * Models the runtime currently has installed and can serve.
   *
   * - `null` ⇒ the adapter did not produce a list (no HTTP probe yet,
   *   or the probe failed). UI must NOT render this as "0 models".
   * - `[]` ⇒ probed and the daemon reports zero installed models.
   * - `[…]` ⇒ ordered list returned by the runtime.
   *
   * D2 fills this for Ollama only; other adapters carry `null`.
   */
  models: ProviderModel[] | null;
};

type EmptyPayload = Record<string, never>;

export function listProviders(): Promise<ProviderInfo[]> {
  return invokeIpc<EmptyPayload, ProviderInfo[]>('providers_list', {});
}

export function getProvidersHealth(): Promise<ProviderHealth[]> {
  return invokeIpc<EmptyPayload, ProviderHealth[]>('providers_health', {});
}

export type FitState = 'comfortable' | 'tight' | 'too-large' | 'unknown';

export type FitEstimate = {
  state: FitState;
  /** Plume's estimated peak working set in bytes (weights + KV + host overhead). */
  estimatedRamBytes: number | null;
  /** Host physical memory bytes when the platform reports it. */
  machineRamBytes: number | null;
  /** Auditable one-sentence rationale that drove the verdict. */
  rationale: string;
};

export type ProviderModelInfo = {
  format: string | null;
  family: string | null;
  parameterSize: string | null;
  parameterCount: number | null;
  quantization: string | null;
  contextLength: number | null;
  /** Capability flags from the runtime (`"completion"`, `"vision"`, …). */
  capabilities: string[];
};

export type ProviderModelDetails = {
  providerId: ProviderId;
  modelId: string;
  /** `null` when the per-model HTTP probe failed; the verb itself still succeeds. */
  details: ProviderModelInfo | null;
  fit: FitEstimate;
  /** Hand-written runtime-path label, e.g. `"GGUF / Metal (Ollama)"` on macOS. */
  runtimePath: string | null;
};

type ModelDetailsPayload = {
  providerId: ProviderId;
  modelId: string;
};

/**
 * Fetch the model-truth details for a single model. Fired lazily —
 * the panel only calls this when the user expands a model row.
 */
export function getModelDetails(
  providerId: ProviderId,
  modelId: string,
): Promise<ProviderModelDetails> {
  return invokeIpc<ModelDetailsPayload, ProviderModelDetails>('providers_model_details', {
    providerId,
    modelId,
  });
}

/** Render-friendly text for a fit verdict. */
export function fitLabel(state: FitState): string {
  switch (state) {
    case 'comfortable':
      return 'comfortable';
    case 'tight':
      return 'tight';
    case 'too-large':
      return 'likely too large';
    case 'unknown':
      return 'unknown';
  }
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
