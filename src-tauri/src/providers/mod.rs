//! Provider registry + reachability.
//!
//! Implements the static side of `providers.list` and the dynamic
//! side of `providers.health` from `docs/IPC_CONTRACT.md` § providers.
//! No model loading, no chat — that lives in later slices behind the
//! `Provider` trait sketched in `docs/MODEL_PROVIDERS.md`.
//!
//! The two-track split (provider track vs engine track) is documented
//! in `docs/MODEL_PROVIDERS.md § Runtime categories`. Engines are *not*
//! in this module.

use serde::Serialize;

pub mod fit;
pub mod health;
pub mod http;
pub mod ollama;
pub mod openai_compat;
pub mod registry;

pub use fit::FitEstimate;

/// Static provider metadata. Mirrors `ProviderInfo` in
/// `docs/IPC_CONTRACT.md`.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProviderInfo {
    pub id: String,
    pub display_name: String,
    pub category: ProviderCategory,
    pub capabilities: ProviderCapabilities,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ProviderCategory {
    /// Plume spawns and supervises the runtime.
    PlumeManaged,
    /// Plume connects to a daemon the user has already started.
    Connected,
}

/// Mirrors `ProviderCapabilities` in `docs/MODEL_PROVIDERS.md`.
///
/// D1 fills only the obvious axes (`owned_process`, `tool_calls`).
/// `streaming` is `false` until chat lands; `max_context: 0` reads
/// "unknown" per the contract.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct ProviderCapabilities {
    pub streaming: bool,
    pub tool_calls: ToolCallSupport,
    pub vision: bool,
    pub embeddings: bool,
    pub max_context: u32,
    pub owned_process: bool,
}

/// `#[allow(dead_code)]`: `PromptOnly`, `JsonMode`, and `Native` are
/// reserved for adapters that genuinely support those modes. They are
/// part of the published wire contract today (`docs/MODEL_PROVIDERS.md`
/// — "MVP only uses `None` and `PromptOnly`; the richer variants are
/// reserved so the field shape doesn't churn"). Dropping and re-adding
/// later would be a contract regression.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq, Default)]
#[serde(rename_all = "kebab-case")]
pub enum ToolCallSupport {
    #[default]
    None,
    PromptOnly,
    JsonMode,
    Native,
}

/// Dynamic per-provider state. One snapshot per probe. Returned by
/// `providers.health`.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProviderHealth {
    pub id: String,
    pub state: ReachabilityState,
    /// TCP connect latency in milliseconds. `None` for non-probed
    /// states (`NotConfigured`, `Unknown`).
    pub latency_ms: Option<u32>,
    /// Unix epoch milliseconds when this snapshot was taken.
    pub probed_at_ms: u64,
    /// Models the runtime currently reports through its list
    /// endpoint. The semantic is adapter-specific:
    ///
    /// - Ollama (`/api/tags`): the daemon's installed-tag catalog.
    /// - LM Studio (`/v1/models`): the models LM Studio describes
    ///   as "visible to the server" — typically loaded/loadable in
    ///   the running session, not the full downloaded library.
    ///   LM Studio's richer `/api/v1/models` is roadmap.
    /// - llama.cpp (`/v1/models`): the models `llama-server` is
    ///   currently serving.
    ///
    /// `None` means "we did not probe" or "the adapter does not
    /// know how"; the empty vector means "we probed and the runtime
    /// reported zero models". The UI must render those two cases
    /// differently.
    pub models: Option<Vec<ProviderModel>>,
}

/// Per-model metadata returned by `providers.health`. Tiny on purpose
/// — the model picker (later slice) is the home for richer fields
/// like quantization, family, parameter size. Today the panel only
/// shows count + names + raw byte size.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProviderModel {
    /// Adapter-specific model id, opaque to Plume. For Ollama this is
    /// the tag string (`gemma:7b`, `qwen2.5-coder:14b-q4`, …).
    pub id: String,
    /// On-disk size in bytes if the adapter reports it. `None` when
    /// the runtime omits the field. UI formats the bytes itself.
    pub size_bytes: Option<u64>,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ReachabilityState {
    /// Probed and a TCP listener answered.
    Available,
    /// Probed and nothing answered before the timeout.
    Offline,
    /// Plume does not yet know how to start or contact this provider.
    /// Today this is the default for Plume-managed runtimes — process
    /// supervision lands later. Not an error state.
    NotConfigured,
}

pub use health::probe_all;
pub use registry::default_providers;

/// Richer per-model snapshot returned by `providers.modelDetails`.
/// Fetched lazily — the provider panel asks for one of these only
/// when the user expands a model row. Mirrors the wire shape in
/// `docs/IPC_CONTRACT.md § providers`.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProviderModelDetails {
    /// Echoes the provider id from the request so the frontend can
    /// route responses back to the right row.
    pub provider_id: String,
    /// Echoes the model id from the request.
    pub model_id: String,
    /// Concrete per-model fields the adapter could read. `None` when
    /// the probe failed; the caller should not treat that as an
    /// authoritative "no info" — try again on refresh.
    pub details: Option<ProviderModelInfo>,
    /// Estimated fit on the current host. Always present; its `state`
    /// can be `unknown` when inputs are missing.
    pub fit: FitEstimate,
    /// Hand-written runtime path label. Today: `"GGUF/Metal"` for
    /// Ollama on macOS, mirroring what `docs/MODEL_PROVIDERS.md §
    /// Ollama` says about the GGUF/Metal default on Mac. `None` for
    /// providers that have no honest label to display yet.
    pub runtime_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProviderModelInfo {
    /// File container format (`"gguf"`, `"safetensors"`, …) verbatim
    /// from the runtime.
    pub format: Option<String>,
    /// Family string from the runtime (`"llama"`, `"gemma"`, …).
    pub family: Option<String>,
    /// Human-readable parameter-size label (`"8.0B"`, `"7B"`, …).
    pub parameter_size: Option<String>,
    /// Exact parameter count when the runtime reports it. Preferred
    /// over `parameter_size` for fit math because the display string
    /// rounds.
    pub parameter_count: Option<u64>,
    /// Quantization label (`"Q4_0"`, `"Q4_K_M"`, `"F16"`, …).
    pub quantization: Option<String>,
    /// Native context window. `None` when the runtime omits it.
    pub context_length: Option<u32>,
    /// Capability flags verbatim from the runtime
    /// (`"completion"`, `"vision"`, …). Empty array means "we asked
    /// but the runtime didn't say"; `null` is not used.
    pub capabilities: Vec<String>,
}
