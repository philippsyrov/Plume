//! Stop-side of the MLX supervisor: the SIGINT-grace → SIGKILL
//! escalation, the Thermos-I1 normal-exit sweep, and the
//! managed-server recovery listing.
//!
//! Split out of `process.rs` in the same sibling-file pattern as
//! `process_launch.rs` / `process_ring_buffer.rs` /
//! `process_health.rs` (D117/D119) so the supervisor stays under the
//! 800-line decomposition cap. This is a CHILD module of `process`,
//! so it shares the registry internals; every public item re-exports
//! through `process::` and callers never name this module directly.

use std::process::Child;
use std::thread;
use std::time::{Duration, Instant};

use serde::Serialize;

use super::{now_unix_ms, supervisor, ManagedSlot, ServerHandleId, StopError, Supervisor};

/// Grace period after SIGINT before falling back to SIGKILL on
/// stop. mlx_lm's `KeyboardInterrupt` handler should call
/// `response_generator.stop_and_join()` plus `httpd.shutdown()`,
/// which completes in under a second on idle servers; three
/// seconds is conservative for an in-flight chat completion to
/// drain.
pub(crate) const STOP_SIGINT_GRACE: Duration = Duration::from_secs(3);

/// How `stop_child` brought the child down. Observable so the
/// escalation branch is testable and so the exit sweep can log how
/// many children needed force.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopOutcome {
    /// The child exited within `STOP_SIGINT_GRACE` of the group
    /// SIGINT — the graceful path.
    GracefulExit,
    /// The grace period elapsed and the child (plus its process
    /// group) was SIGKILLed. On non-unix targets every stop reports
    /// this variant: there is no graceful signal, only `Child::kill`.
    EscalatedSigkill,
}

/// Stop a server by handle id on the process-wide supervisor.
/// Removes the registration first so the port is free for the next
/// start even if the kill itself hits an error. On unix, sends
/// SIGINT to the child's process group and waits up to
/// `STOP_SIGINT_GRACE`; if the child hasn't exited, escalates to
/// SIGKILL across the WHOLE process group (Codex D40 fix) —
/// `Child::kill` alone would only signal the direct child, leaving
/// any grandchildren mlx-lm spawned alive. `Child::kill` + `wait`
/// still runs after the pgroup SIGKILL so std reaps the zombie. On
/// Windows, immediate `Child::kill`.
pub fn stop_server(id: &ServerHandleId) -> Result<(), StopError> {
    supervisor().stop_server(id)
}

/// One row of `list_managed_servers` / `providers.listServers`: the
/// recovery view of a live Plume-managed child. Contains only what
/// this process itself started — the registry never learns about
/// servers from other Plume instances or the wider machine, so the
/// verb cannot claim ownership it doesn't have.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ManagedServerInfo {
    /// Opaque handle id (round-trips with `providers.stopServer` and
    /// `providers.serverDiagnostics`).
    pub handle_id: String,
    /// Bound port on 127.0.0.1.
    pub port: u16,
    /// Child process PID, for Activity Monitor / diagnostics parity
    /// with `ServerHandle`.
    pub pid: u32,
    /// The caller-side inventory id recorded at start (empty when
    /// the caller had none). A reloaded frontend re-keys its
    /// per-model bookkeeping on this without a path scan.
    pub model_id: String,
    /// The exact `--model` value the supervisor passed at spawn.
    pub model_label: String,
    /// Unix epoch milliseconds when `/health` first answered 200.
    pub started_at_ms: u64,
    /// `now - started_at_ms`, saturating.
    pub uptime_ms: u64,
}

/// What the exit sweep did, for the shutdown log line. All counters
/// are bounded by `MAX_MANAGED_SERVERS`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ShutdownSummary {
    /// Children stopped (gracefully or by escalation).
    pub stopped: usize,
    /// Subset of `stopped` that needed the SIGKILL escalation.
    pub escalated: usize,
    /// Children whose stop returned an OS error (or whose stopper
    /// thread panicked). The registry entry is gone either way.
    pub errors: usize,
    /// Children still in their startup window (spawned, `/health`
    /// not yet 200) that the sweep SIGKILLed by pid (Codex #154
    /// lifecycle fix). No grace period: a loading child has no
    /// in-flight requests to drain, and the owning start thread will
    /// reap it and refuse to register.
    pub killed_starting: usize,
}

/// Snapshot of every HEALTHY server this Plume process currently
/// manages. Children that exited on their own are reaped first and
/// never listed; children still in their startup window are excluded
/// until `/health` passes (their handle doesn't exist yet). Ordered
/// oldest-start first (handle id as the tiebreaker) so the wire
/// shape is deterministic.
pub fn list_managed_servers() -> Vec<ManagedServerInfo> {
    supervisor().list_servers()
}

/// Stop every managed child — running ones via the SIGINT-grace →
/// SIGKILL escalation, mid-startup ones via a direct pid SIGKILL —
/// and latch the registry shut so no in-flight start can register
/// afterwards. Called from the Tauri `RunEvent::Exit` hook on NORMAL
/// application exit. This is explicitly not crash recovery: a
/// SIGKILLed or crashed Plume never runs this sweep, and the
/// children's own sessions (see `configure_own_session`) mean
/// nothing else will signal them either — that limitation is
/// documented in `docs/MLX_RUNTIME.md § Shutdown`.
pub fn shutdown_all_managed_servers() -> ShutdownSummary {
    supervisor().shutdown_all()
}

impl Supervisor {
    pub(crate) fn stop_server(&self, id: &ServerHandleId) -> Result<(), StopError> {
        let mut server = {
            let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
            match state.slots.remove(&id.0) {
                Some(ManagedSlot::Running(server)) => server,
                // A `Starting` slot's handle id was never issued to
                // any caller — treat a hit as unknown and put the
                // reservation back so the owning start thread's
                // commit still finds it.
                Some(reservation @ ManagedSlot::Starting { .. }) => {
                    state.slots.insert(id.0.clone(), reservation);
                    return Err(StopError::UnknownHandle);
                }
                None => return Err(StopError::UnknownHandle),
            }
        }; // registry mutex freed while we wait on the child
        stop_child(&mut server.child).map_err(StopError::Io)?;
        Ok(())
    }

    pub(crate) fn list_servers(&self) -> Vec<ManagedServerInfo> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        // Children that exited on their own (crash after readiness,
        // clean self-exit) must not be listed — a reloaded frontend
        // would re-adopt a dead pid as `running` (Codex #154
        // lifecycle fix). `Starting` slots are excluded below: their
        // handle isn't issued until `/health` passes.
        state.reap_exited();
        let now = now_unix_ms();
        let mut servers: Vec<ManagedServerInfo> = state
            .slots
            .iter()
            .filter_map(|(handle_id, slot)| match slot {
                ManagedSlot::Starting { .. } => None,
                ManagedSlot::Running(server) => Some(ManagedServerInfo {
                    handle_id: handle_id.clone(),
                    port: server.port,
                    pid: server.child.id(),
                    model_id: server.model_id.clone(),
                    model_label: server.model_label.clone(),
                    started_at_ms: server.started_at_ms,
                    uptime_ms: now.saturating_sub(server.started_at_ms),
                }),
            })
            .collect();
        servers.sort_by(|a, b| {
            a.started_at_ms
                .cmp(&b.started_at_ms)
                .then_with(|| a.handle_id.cmp(&b.handle_id))
        });
        servers
    }

    /// Latch `shutting_down`, drain the registry, and stop every
    /// child. Running children go through the same `stop_child`
    /// escalation `stop_server` uses, one thread per child so the
    /// whole sweep is bounded by a single grace period
    /// (~`STOP_SIGINT_GRACE` + reap) instead of N of them stacked;
    /// the fan-out is itself bounded because the registry never
    /// exceeds `MAX_MANAGED_SERVERS` entries. Children still in
    /// their startup window (`Starting` slots) are SIGKILLed by pid
    /// (Codex #154 lifecycle fix): their `Child` value lives on the
    /// start thread, so the pid recorded at reservation is the only
    /// name the sweep has for them — and once `shutting_down` is
    /// set, that thread can never register them either.
    pub(crate) fn shutdown_all(&self) -> ShutdownSummary {
        let drained: Vec<ManagedSlot> = {
            let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
            state.shutting_down = true;
            state.slots.drain().map(|(_, slot)| slot).collect()
        };
        let mut summary = ShutdownSummary::default();
        let mut stoppers = Vec::new();
        for slot in drained {
            match slot {
                ManagedSlot::Running(mut server) => {
                    stoppers.push(thread::spawn(move || stop_child(&mut server.child)));
                }
                ManagedSlot::Starting { pid } => {
                    #[cfg(unix)]
                    unsafe {
                        // Negative pid → the child's whole process
                        // group (its own session per
                        // `configure_own_session`). Best-effort:
                        // ESRCH just means it already died.
                        let _ = super::libc_kill(-(pid as i32), 9); // 9 == SIGKILL
                    }
                    #[cfg(not(unix))]
                    let _ = pid; // no pid-addressed kill without a Child handle
                    summary.killed_starting += 1;
                }
            }
        }
        for stopper in stoppers {
            match stopper.join() {
                Ok(Ok(outcome)) => {
                    summary.stopped += 1;
                    if outcome == StopOutcome::EscalatedSigkill {
                        summary.escalated += 1;
                    }
                }
                Ok(Err(_)) | Err(_) => summary.errors += 1,
            }
        }
        summary
    }
}

pub(crate) fn stop_child(child: &mut Child) -> std::io::Result<StopOutcome> {
    #[cfg(unix)]
    {
        let pid = child.id();
        // Send SIGINT to the child's own process group (negative
        // pid) so any grandchildren mlx-lm spawned also see it.
        // We set up its own session in pre_exec, so the negative
        // pgid is the child's pgid.
        unsafe {
            // Best-effort; ignore EPERM/ESRCH (already exited).
            let _ = super::libc_kill(-(pid as i32), 2); // 2 == SIGINT
        }
        let deadline = Instant::now() + STOP_SIGINT_GRACE;
        loop {
            match child.try_wait()? {
                Some(_status) => return Ok(StopOutcome::GracefulExit),
                None if Instant::now() >= deadline => break,
                None => thread::sleep(Duration::from_millis(50)),
            }
        }
        // Grace exceeded — escalate to SIGKILL across the WHOLE
        // process group (Codex D40 LOW/MEDIUM fix). `Child::kill`
        // would only target the direct child; any grandchildren
        // mlx-lm spawned (uvicorn worker subprocesses, Python
        // multiprocessing pool, etc.) would survive and keep the
        // port bound. Negative pid → `pgid` per `kill(2)`.
        unsafe {
            let _ = super::libc_kill(-(pid as i32), 9); // 9 == SIGKILL
        }
        // Fall through to `Child::kill` and `wait` regardless so
        // the std side reaps the zombie. `kill(9)` against an
        // already-exited child is a harmless ESRCH.
        let _ = child.kill();
        let _ = child.wait()?;
        Ok(StopOutcome::EscalatedSigkill)
    }
    #[cfg(not(unix))]
    {
        let _ = child.kill();
        let _ = child.wait()?;
        Ok(StopOutcome::EscalatedSigkill)
    }
}
