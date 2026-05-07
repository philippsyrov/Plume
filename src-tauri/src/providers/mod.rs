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

pub mod health;
pub mod registry;

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
