//! Plume-managed MLX-LM runtime.
//!
//! D40 lands the **process-supervisor skeleton**: port allocation,
//! spawn shape, /health probe, owned-process registry, SIGINT→kill
//! shutdown. No chat routing yet — the OpenAI-SSE parser from D39
//! is wired in by a follow-up slice. No model download. No
//! auto-install of `mlx-lm` (the user installs it themselves).
//!
//! Module layout follows `docs/MLX_RUNTIME.md § Module placement`:
//!
//! ```text
//! providers/mlx_lm/
//!   mod.rs         this file (registry + re-exports)
//!   process.rs     spawn / supervise / shutdown (D40)
//!   process_launch.rs       port allocation + command builder (D117 split)
//!   process_ring_buffer.rs  capped stdout+stderr capture (D117 split)
//!   process_tests.rs
//!   routes.rs      OpenAI-SSE chat routing (follow-up)
//! ```
//!
//! The supervisor uses a process-wide registry (`OnceLock<Mutex<
//! HashMap<HandleId, ServerProcess>>>`) keyed by an opaque handle id
//! the IPC verbs round-trip with the frontend. The handle owns the
//! `std::process::Child`, the captured `ProcessOutput` ring buffer,
//! and the allocated port; dropping the registry entry on stop is
//! what actually frees the port for re-allocation.

pub mod process;

#[cfg(test)]
#[path = "process_tests.rs"]
mod tests;

// Production re-exports. The test-only helpers (`allocate_port`,
// `build_command_args`, `default_mlx_lm_command`, `MlxLmCommand`,
// `RingBuffer`) stay reachable through `process::` directly so the
// public surface stays small; tests `use super::process::*`.
pub use process::{
    lookup_diagnostics, lookup_handle_info, start_server, stop_server, ServerDiagnostics,
    ServerHandle, ServerHandleId, ServerStartOptions, StartError, StopError,
};
