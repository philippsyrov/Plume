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
   * Models the runtime currently reports through its list endpoint.
   * The semantic varies a little by adapter — read each runtime's
   * own docs before treating this as a download catalog:
   *
   * - **Ollama** (D2, `/api/tags`): the daemon's installed-tag
   *   catalog. Fills `sizeBytes` with the on-disk byte count.
   * - **LM Studio** (D4, `/v1/models`): the models LM Studio
   *   describes as "visible to the server" — typically loaded /
   *   loadable through the running session. Not the full
   *   downloaded catalog; LM Studio's richer `/api/v1/models`
   *   endpoint is roadmap. `sizeBytes` is `null` here because
   *   `/v1/models` does not report byte size.
   * - **llama.cpp** (D4, `/v1/models`): the models `llama-server`
   *   is currently serving. `sizeBytes` is `null` for the same
   *   reason.
   *
   * Three field states the UI must distinguish:
   * - `null` ⇒ the adapter did not produce a list (no HTTP probe
   *   yet, or the probe failed). Must NOT render as "0 models".
   * - `[]` ⇒ probed and the runtime reports zero models.
   * - `[…]` ⇒ ordered list returned by the runtime.
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

/**
 * - `gguf`: a single `.gguf` weight file at the model-dir root.
 * - `safetensors`: a single `.safetensors` weight file.
 * - `transformer-folder`: a folder shaped like a HuggingFace
 *   transformer checkpoint (`config.json`, a `tokenizer*` file, and a
 *   weight file). The inventory does NOT claim the weights are
 *   MLX-format. A vanilla `huggingface-cli download` of a PyTorch or
 *   safetensors checkpoint lands in this category.
 * - `mlx-folder`: same shape as `transformer-folder` PLUS verified
 *   MLX evidence — either a `weights.npz` shard (legacy MLX format)
 *   or a `config.json` carrying the MLX-LM quantization shape
 *   (`{"quantization": {"group_size": _, "bits": _}}`). Added in
 *   D36; every `mlx-folder` is also transformer-folder-shaped on
 *   disk. The product rule is "Plume must not label a model as MLX
 *   unless it has checked enough on disk to justify that claim" —
 *   see `docs/LOCAL_AGENT_NORTH_STAR.md § MLX-first`.
 */
export type LocalModelKind = 'gguf' | 'safetensors' | 'transformer-folder' | 'mlx-folder';

/**
 * D50: which on-disk source an inventory entry came from.
 *
 * - `plume-model-dir` (primary): `$PLUME_MODEL_DIR`, default
 *   `<cwd>/plume-models`. The only source Plume writes to.
 * - `locally-ai-cache`: Locally AI's sandboxed HuggingFace cache at
 *   `~/Library/Containers/app.locallyai.Locally/Data/Library/
 *   app.locallyai.Locally/huggingface/models`. Read-only.
 * - `lm-studio-cache`: LM Studio's models tree at
 *   `~/.lmstudio/models`. Read-only.
 *
 * Ollama's blob store is deliberately NOT a source — the on-disk
 * layout is content-addressed and the human-readable model id lives
 * only in Ollama's SQLite manifest. Ollama remains a provider via
 * `/api/tags` (chat works), but the underlying files stay opaque.
 */
export type LocalModelSource = 'plume-model-dir' | 'locally-ai-cache' | 'lm-studio-cache';

export type LocalModel = {
  /**
   * Source-prefixed id of the form `<source-tag>:<relative-path>`.
   * Pre-D50 this was a bare relative path; post-D50 every id carries
   * the source tag so two roots with an identically named subfolder
   * don't collide on the wire. Frontend code should treat the id as
   * opaque and round-trip it through the IPC verbatim.
   */
  id: string;
  /** File or folder name for compact display. */
  name: string;
  /** Absolute path returned by the backend for read-only inventory. */
  path: string;
  kind: LocalModelKind;
  sizeBytes: number;
  source: LocalModelSource;
};

export function getLocalModels(): Promise<LocalModel[]> {
  return invokeIpc<EmptyPayload, LocalModel[]>('providers_local_models', {});
}

/**
 * D41: honest on-disk details for a single local-model entry. Every
 * field is optional because real-world checkpoints vary — a quantized
 * MLX folder fills everything, a vanilla HF safetensors folder drops
 * the `quantization*` pair, a single `.gguf` file drops everything
 * except `weight*`.
 */
export type LocalModelDetails = {
  architecture: string | null;
  modelType: string | null;
  maxContext: number | null;
  /** MLX-LM quantization bits. HF's `quantization_config` does NOT
   * populate this — see `docs/LOCAL_AGENT_NORTH_STAR.md § MLX-first`. */
  quantizationBits: number | null;
  quantizationGroupSize: number | null;
  tokenizerPresent: boolean;
  weightFileCount: number;
  weightBytesTotal: number;
};

type LocalModelDetailsPayload = {
  id: string;
};

/**
 * D41: fetch on-disk details for a local-model row. Fired lazily —
 * the panel only calls this when the user expands a row. The verb
 * does no network IO and no model load; the cost is one `read_dir`
 * + one bounded JSON parse per call.
 */
export function getLocalModelDetails(id: string): Promise<LocalModelDetails> {
  return invokeIpc<LocalModelDetailsPayload, LocalModelDetails>(
    'providers_local_model_details',
    { id },
  );
}

/**
 * D52: live diagnostics for a running Plume-managed MLX server. Read
 * via `providers.serverDiagnostics({handleId})` — the verb returns a
 * snapshot every time it's called; the panel polls on a slow cadence
 * (every few seconds is plenty). Read-only — the verb never restarts
 * or stops the process.
 */
export type ServerDiagnostics = {
  /** Opaque handle id round-tripped from `providers.startServer`. */
  handleId: string;
  /** Bound port on 127.0.0.1. */
  port: number;
  /** Child process PID — surfaced for Activity Monitor / manual kill. */
  pid: number;
  /** The exact `--model` value the supervisor passed at spawn. */
  modelLabel: string;
  /** Unix epoch ms when `/health` first answered 200. */
  startedAtMs: number;
  /** `now - startedAtMs`, saturating. */
  uptimeMs: number;
  /** Last ~16 KiB of mlx-lm stdout+stderr, lossy-UTF-8. */
  logTail: string;
  /** Currently-resident bytes in the supervisor's ring buffer. */
  logBytes: number;
  /** Hard cap on the ring buffer. `logBytes == logCapacity` implies
   *  earlier output was evicted to make room. */
  logCapacity: number;
};

type ServerDiagnosticsPayload = {
  handleId: string;
};

/**
 * D52: fetch a diagnostics snapshot for a running supervisor handle.
 * Rejects with `NotFound` when the handle id is unknown (never issued,
 * already stopped, belongs to a different Plume instance) so the panel
 * can drop the disclosure without surfacing a confusing error.
 */
export function getServerDiagnostics(handleId: string): Promise<ServerDiagnostics> {
  return invokeIpc<ServerDiagnosticsPayload, ServerDiagnostics>(
    'providers_server_diagnostics',
    { handleId },
  );
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

/**
 * Provider ids that today have a backing `providers.modelDetails`
 * probe. Used by the panel to gate the expand-in-place caret on
 * model rows — clicking a model with no detail probe would just
 * return `BadArgument` and surface as an error, so we hide the
 * affordance entirely instead. Add an entry here when a new
 * adapter's per-model probe lands.
 */
export const PROVIDERS_WITH_DETAILS: readonly ProviderId[] = ['ollama'];

/**
 * D40: handle returned by `providers.startServer`. The frontend
 * stores `id` and pairs it with `stopServer` calls; the `port` is
 * exposed so chat-routing code can address the running server
 * without re-reading the registry. `pid` is for diagnostics
 * (Activity Monitor, manual `kill`).
 */
export type ServerHandle = {
  id: string;
  port: number;
  pid: number;
};

export type StartServerPayload = {
  providerId: ProviderId;
  modelId: string;
};

/**
 * D40: spawn a Plume-managed local server for the given local
 * model. Today only `providerId: 'mlx-lm'` is accepted; other ids
 * reject with `BadArgument` until their adapter lands. The
 * supervisor allocates an ephemeral port, spawns
 * `python -m mlx_lm server --model … --host 127.0.0.1 --port …`,
 * polls `/health` until ready, then returns the handle.
 */
export function startServer(payload: StartServerPayload): Promise<ServerHandle> {
  return invokeIpc<StartServerPayload, ServerHandle>('providers_start_server', payload);
}

export type StopServerPayload = {
  handleId: string;
};

export type StopServerResponse = {
  ok: boolean;
};

/**
 * D40: stop a previously-started server by handle id. On unix the
 * supervisor sends SIGINT first (3-second grace), then escalates
 * to SIGKILL. Idempotent in spirit: stopping an already-stopped
 * handle returns `NotFound`.
 */
export function stopServer(payload: StopServerPayload): Promise<StopServerResponse> {
  return invokeIpc<StopServerPayload, StopServerResponse>('providers_stop_server', payload);
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
