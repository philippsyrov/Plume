//! D52 diagnostics for the MLX supervisor: the read-only snapshot
//! behind `providers.serverDiagnostics(handleId)`.
//!
//! Split out of `process.rs` in the same sibling-file pattern as
//! `process_launch.rs` / `process_ring_buffer.rs` /
//! `process_health.rs` / `process_stop.rs` (the Codex #154
//! reservation rework pushed the supervisor past the 800-line
//! decomposition cap). This is a CHILD module of `process`, so it
//! shares the registry internals; every public item re-exports
//! through `process::` and callers never name this module directly.

use serde::Serialize;

use super::{now_unix_ms, supervisor, ManagedSlot, ServerHandleId, Supervisor, RING_BUFFER_CAP};

/// D52 diagnostics snapshot for a registered handle. Surfaced via
/// `providers.serverDiagnostics(handleId)` so the panel can render
/// uptime + a log tail next to a running row without the user having
/// to drop to a terminal. Read-only — the verb never mutates the
/// process registry or restarts a server.
///
/// `log_bytes` is the current ring buffer occupancy; `log_capacity` is
/// the cap (`RING_BUFFER_CAP = 16 KiB`). When `log_bytes ==
/// log_capacity` the supervisor is at the cap and oldest bytes are
/// being evicted as new output arrives. The UI can render a "log
/// truncated" hint in that case.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ServerDiagnostics {
    /// Opaque handle id (round-trips with `providers.stopServer`).
    pub handle_id: String,
    /// Bound port on 127.0.0.1.
    pub port: u16,
    /// Child process PID. Surfaced for Activity Monitor / `kill`.
    pub pid: u32,
    /// The `--model` value the supervisor passed at spawn — typically
    /// an absolute path under `default_model_dir()` or a known source.
    pub model_label: String,
    /// Unix epoch milliseconds when the handle was registered (i.e.
    /// the moment `/health` first answered 200).
    pub started_at_ms: u64,
    /// `now_unix_ms() - started_at_ms`. Saturating; never negative.
    pub uptime_ms: u64,
    /// Last N bytes of mlx-lm's stdout+stderr, decoded lossily as
    /// UTF-8. The ring is 16 KiB; this string is at most that long.
    pub log_tail: String,
    /// Currently-resident bytes in the ring buffer.
    pub log_bytes: u32,
    /// Hard cap on the ring buffer (currently `RING_BUFFER_CAP`).
    /// The UI can derive "log_truncated" as `log_bytes ==
    /// log_capacity`.
    pub log_capacity: u32,
}

/// D52: read a diagnostics snapshot for a registered handle. Returns
/// `None` when the id isn't registered (never issued, already
/// stopped, belongs to a different Plume instance) — the IPC layer
/// maps that to `NotFound` so the panel can drop the disclosure.
/// Snapshot is taken atomically under the registry mutex so a
/// concurrent `stop_server` can't observe the handle in a half-
/// destroyed state; the log tail also locks the ring buffer briefly,
/// which is the same lock the reader threads hold while pushing
/// stdout / stderr.
pub fn lookup_diagnostics(id: &ServerHandleId) -> Option<ServerDiagnostics> {
    supervisor().lookup_diagnostics(id)
}

impl Supervisor {
    pub(crate) fn lookup_diagnostics(&self, id: &ServerHandleId) -> Option<ServerDiagnostics> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        // A child that exited on its own must not surface stale
        // diagnostics (Codex #154 lifecycle fix); `Starting` slots
        // have no issued handle yet.
        state.reap_exited();
        let server = match state.slots.get(&id.0) {
            Some(ManagedSlot::Running(server)) => server,
            _ => return None,
        };
        let log_tail = server
            .output
            .lock()
            .map(|guard| guard.snapshot())
            .unwrap_or_default();
        let log_bytes = server
            .output
            .lock()
            .map(|guard| guard.len() as u32)
            .unwrap_or(0);
        let now = now_unix_ms();
        let uptime_ms = now.saturating_sub(server.started_at_ms);
        Some(ServerDiagnostics {
            handle_id: id.0.clone(),
            port: server.port,
            pid: server.child.id(),
            model_label: server.model_label.clone(),
            started_at_ms: server.started_at_ms,
            uptime_ms,
            log_tail,
            log_bytes,
            log_capacity: RING_BUFFER_CAP as u32,
        })
    }
}
