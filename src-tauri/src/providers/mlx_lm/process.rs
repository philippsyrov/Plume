//! D40: Plume-managed MLX-LM process supervisor skeleton.
//!
//! Five pieces:
//!
//! 1. **Port allocation.** `allocate_port` binds `127.0.0.1:0`,
//!    reads the OS-assigned ephemeral port, then drops the listener
//!    so the child can rebind. There is a small race window between
//!    the drop and the child's bind that the upstream "retry once
//!    on health-probe timeout" plan in `docs/MLX_RUNTIME.md` covers
//!    end-to-end; the allocator itself is unconditional.
//!
//! 2. **Command shape.** `build_command_args` produces the
//!    `python -m mlx_lm server --model … --host 127.0.0.1 --port …
//!    --log-level …` invocation per `docs/MLX_RUNTIME.md § CLI`.
//!    The deprecated `python -m mlx_lm.server` form is NEVER used —
//!    upstream prints a deprecation message when callers use it.
//!    `default_mlx_lm_command()` returns the production launcher;
//!    tests construct their own `MlxLmCommand` so they can exercise
//!    the supervisor without an actual `mlx-lm` install.
//!
//! 3. **Ring buffer.** `RingBuffer` is a capped `VecDeque<u8>` that
//!    background reader threads push stdout + stderr into. The cap
//!    is `RING_BUFFER_CAP = 16 KiB`; pushes beyond it drop the
//!    oldest bytes. The point is to keep enough context to surface
//!    a bring-up failure in the IPC response without unbounded
//!    memory growth.
//!
//! 4. **Health poll.** `poll_health` opens a TCP connection to
//!    `127.0.0.1:<port>` and writes a minimal `GET /health`
//!    request, reading the status line of the response. Returns
//!    `Ok(())` on `200 OK`. Backoff (50 ms → 200 ms → 500 ms,
//!    capped at the overall budget) keeps the poll cheap while the
//!    child is still loading the model.
//!
//! 5. **Owned-process registry + start/stop.** `start_server` runs
//!    the four-step lifecycle (allocate → spawn → background reader
//!    → poll) and inserts the result into a process-wide registry.
//!    `stop_server` looks up the handle id, sends SIGINT on unix
//!    (with a 3-second grace period before falling back to
//!    `Child::kill`), then drops the registration so the port is
//!    free for the next start.
//!
//! No chat routing here. No model download. No auto-install. The
//! caller is responsible for handing in a model path that already
//! exists on disk — `providers.localModels` is the source.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::Read;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

// D117: launch shape (port allocation + command builder) and the
// ring buffer live in sibling files. Re-exported at their original
// `process::` paths so `use super::process::*;` in the test sibling
// and every internal caller resolve unchanged.
#[path = "process_launch.rs"]
mod launch;
pub use launch::{allocate_port, build_command_args, default_mlx_lm_command, MlxLmCommand};

#[path = "process_ring_buffer.rs"]
mod ring_buffer;
pub use ring_buffer::{RingBuffer, RING_BUFFER_CAP};

// D119: the /health readiness probe follows the same pattern.
#[path = "process_health.rs"]
mod health;
pub use health::{poll_health, HealthError};

// D52 diagnostics follow the same pattern (moved out when the Codex
// #154 reservation rework pushed this file past the 800-line cap).
#[path = "process_diagnostics.rs"]
mod diagnostics;
pub use diagnostics::{lookup_diagnostics, ServerDiagnostics};

// Thermos I1: the stop-side (SIGINT-grace → SIGKILL escalation, the
// normal-exit sweep, the recovery listing) lives in a sibling file,
// same decomposition pattern as launch / ring buffer / health.
#[path = "process_stop.rs"]
mod stop;
pub(crate) use stop::stop_child;
pub use stop::{
    list_managed_servers, shutdown_all_managed_servers, stop_server, ManagedServerInfo,
};
// `StopOutcome`, `ShutdownSummary`, and the grace constant are
// consumed inside `stop` itself in production (callers reach the
// summary through `shutdown_all_managed_servers`' return type
// without naming it); only the lifecycle tests name them from
// outside, so the re-export is test-gated to keep the lib build
// warning-free.
#[cfg(test)]
pub(crate) use stop::{ShutdownSummary, StopOutcome, STOP_SIGINT_GRACE};

/// Hard cap on concurrently managed servers. Each child holds a
/// multi-GB model in unified memory, so any realistic machine is
/// saturated well before eight; the cap exists so the registry (and
/// the exit sweep's thread fan-out, which spawns one stopper thread
/// per entry) stays bounded rather than growing with a runaway
/// caller. Enforced atomically at slot-reservation time, under the
/// same lock as the spawn (Codex #154 lifecycle fix): a full
/// supervisor refuses before creating a process, and concurrent
/// starts cannot all slip past the check. Children that exited on
/// their own are reaped before the count so they never pin a slot.
pub const MAX_MANAGED_SERVERS: usize = 8;

/// Default overall startup budget for `start_server`. mlx-lm
/// loading a 7B weight set from a cold cache can spend 10–25 s
/// reading shards on a typical NVMe; thirty seconds keeps a hard
/// stop on the worst case while not failing healthy starts.
pub const DEFAULT_START_TIMEOUT: Duration = Duration::from_secs(30);

// --- supervisor / registry ----------------------------------------------

/// Opaque handle id issued by `start_server`. The wire shape is a
/// hex-encoded counter so the frontend can copy/paste it in
/// diagnostics without re-formatting; uniqueness is guaranteed for
/// the lifetime of the process by `NEXT_HANDLE_ID`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(transparent)]
pub struct ServerHandleId(pub String);

/// Returned by `start_server`. The frontend stores `id` and pairs
/// it with `stopServer` calls; the `port` is exposed so the chat
/// route slice (D40+1) can target the right port without
/// re-reading the registry.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerHandle {
    pub id: ServerHandleId,
    pub port: u16,
    /// PID of the child mlx-lm process. Surfaced for diagnostics
    /// (Activity Monitor, `kill`); the registry owns the actual
    /// `Child` value.
    pub pid: u32,
}

/// Input to `start_server`. Most fields default; tests override
/// `command` to substitute a fake binary.
#[derive(Debug, Clone)]
pub struct ServerStartOptions {
    /// Absolute path to the model on disk. Today this is a
    /// `mlx-folder` or `transformer-folder` from
    /// `providers.localModels`. The supervisor does not validate
    /// the path's MLX-ness; that's the caller's job.
    pub model_path: PathBuf,
    /// Launcher. `None` selects `default_mlx_lm_command()`.
    pub command: Option<MlxLmCommand>,
    /// `--log-level` value. `"INFO"` matches the upstream default.
    pub log_level: String,
    /// Overall startup deadline (port → spawn → health-probe-OK).
    /// `None` selects `DEFAULT_START_TIMEOUT`.
    pub startup_timeout: Option<Duration>,
    /// Opaque caller-side model identity (the `providers.localModels`
    /// inventory id the IPC handler resolved into `model_path`).
    /// Stored verbatim and echoed by `list_managed_servers` so a
    /// frontend that lost its handles on reload can re-key a running
    /// server without re-deriving identity from the absolute path.
    /// The supervisor itself never interprets it; empty means "the
    /// caller had no inventory id" (direct Rust callers, tests).
    pub model_id: String,
}

impl Default for ServerStartOptions {
    fn default() -> Self {
        Self {
            model_path: PathBuf::new(),
            command: None,
            log_level: "INFO".to_string(),
            startup_timeout: None,
            model_id: String::new(),
        }
    }
}

#[derive(Debug)]
pub enum StartError {
    /// Couldn't bind a port. System-level failure (no free
    /// ephemeral ports, etc.).
    PortAllocation(std::io::Error),
    /// `Command::spawn` failed. The most common cause is the
    /// configured `program` not being on PATH — `python` not
    /// installed, or `--user` install not on PATH. The string
    /// describes the OS-level error for the IPC response.
    Spawn(std::io::Error),
    /// `start_server` couldn't get a 200 from `/health` within
    /// `startup_timeout`. `stderr_tail` carries the captured
    /// output so the caller can surface a clear error.
    HealthTimeout { stderr_tail: String },
    /// `/health` answered with an unexpected status. Carries the
    /// status code and the captured tail for diagnostics.
    HealthBadStatus { status: u16, stderr_tail: String },
    /// `model_path` is empty. Defensive: the IPC handler validates
    /// the input, but a Rust caller that bypasses the handler
    /// shouldn't see a panic.
    InvalidModelPath,
    /// The supervisor already manages `MAX_MANAGED_SERVERS` live
    /// children. Surfaced before any spawn so a full registry never
    /// creates a process it would immediately kill.
    RegistryFull,
    /// The exit sweep has begun. No start may reserve a slot or
    /// register a child once shutdown is in progress — a child that
    /// registered after the sweep drained the registry would outlive
    /// the sweep unstoppably (Codex #154 lifecycle fix).
    ShuttingDown,
}

impl std::fmt::Display for StartError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StartError::PortAllocation(e) => write!(f, "port allocation failed: {e}"),
            StartError::Spawn(e) => write!(f, "spawn failed: {e}"),
            StartError::HealthTimeout { .. } => {
                write!(f, "mlx-lm server did not become ready before the deadline")
            }
            StartError::HealthBadStatus { status, .. } => {
                write!(f, "/health returned status {status}")
            }
            StartError::InvalidModelPath => write!(f, "model_path is empty"),
            StartError::RegistryFull => write!(
                f,
                "already managing {MAX_MANAGED_SERVERS} servers; stop one before starting another"
            ),
            StartError::ShuttingDown => {
                write!(f, "Plume is shutting down; not starting new servers")
            }
        }
    }
}

#[derive(Debug)]
pub enum StopError {
    /// No handle with that id is registered. The handle was never
    /// issued, already stopped, or belongs to a different Plume
    /// instance.
    UnknownHandle,
    /// Sending the signal or waiting for the child failed at the
    /// OS level. The handle is removed from the registry either
    /// way; the supervisor never holds onto a zombie.
    Io(std::io::Error),
}

impl std::fmt::Display for StopError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StopError::UnknownHandle => f.write_str("unknown server handle id"),
            StopError::Io(e) => write!(f, "stop failed: {e}"),
        }
    }
}

struct ServerProcess {
    port: u16,
    child: Child,
    /// D52 reads from this in `lookup_diagnostics` to surface the
    /// last N bytes of mlx-lm's stdout+stderr. Background reader
    /// threads keep pushing into this Arc; holding the Arc here
    /// keeps the ring alive after start_server returns.
    output: Arc<Mutex<RingBuffer>>,
    /// D45 Codex HIGH fix: the exact `--model` value the supervisor
    /// passed to `python -m mlx_lm server`. Chat routing echoes this
    /// back in the OpenAI request's `model` field so the upstream
    /// server's "model must match what was loaded" check passes; if
    /// we sent the IPC-layer `payload.modelId` (like "gemma-2b") and
    /// the server was launched with `--model /abs/path/...`, the two
    /// would disagree and a future mlx-lm with dynamic-reload could
    /// try to fetch a different model from the HF cache. Stored as
    /// `String` (already path-utf8-lossy from `build_command_args`)
    /// so the chat layer doesn't have to think about PathBuf.
    model_label: String,
    /// D52: unix-epoch milliseconds when the handle was registered
    /// (i.e. when `/health` first answered 200). The diagnostics
    /// verb subtracts this from "now" to surface uptime; future UIs
    /// can also show "started at HH:MM" if useful. Captured ONCE at
    /// registration so a slow `SystemTime` read during a log dump
    /// doesn't pollute the answer.
    started_at_ms: u64,
    /// Caller-side inventory id from `ServerStartOptions::model_id`.
    /// Round-tripped by `list_managed_servers`; empty for callers
    /// that had none.
    model_id: String,
}

/// One registry slot. `Starting` is a reservation taken UNDER THE
/// SAME LOCK as the spawn (Codex #154 lifecycle fix): it counts
/// toward `MAX_MANAGED_SERVERS` — so concurrent starts cannot
/// over-spawn past the cap — and it carries the child's pid so the
/// exit sweep can kill a mid-startup child whose `Child` value is
/// still owned by the starting thread. `try_start_once` upgrades the
/// slot to `Running` only if it still exists at commit time; the
/// sweep draining it (or `shutting_down` being set) means the start
/// must stop its own child and refuse.
enum ManagedSlot {
    Starting { pid: u32 },
    Running(ServerProcess),
}

/// Registry map + the shutdown latch, guarded by one mutex so
/// "reserve a slot" and "may starts still register?" are a single
/// atomic question.
struct RegistryState {
    slots: HashMap<String, ManagedSlot>,
    /// Set (permanently) by `shutdown_all`. Once true, no start may
    /// reserve or register — a child registered after the drain
    /// would outlive the sweep with nothing left to stop it.
    shutting_down: bool,
}

impl RegistryState {
    /// Reap children that exited on their own (crash, OOM-kill,
    /// clean exit) so they stop being listed as running, stop
    /// resolving via lookup, and stop consuming cap slots (Codex
    /// #154 lifecycle fix). `try_wait` both detects and reaps the
    /// zombie. `Starting` slots are never touched — their owning
    /// start thread manages that child's lifetime. An `Err` from
    /// `try_wait` keeps the entry: dropping ownership on an
    /// ambiguous answer could orphan a live child.
    fn reap_exited(&mut self) {
        self.slots.retain(|_, slot| match slot {
            ManagedSlot::Starting { .. } => true,
            ManagedSlot::Running(server) => !matches!(server.child.try_wait(), Ok(Some(_))),
        });
    }
}

/// The owned-process registry plus every operation that touches it.
/// Production uses one process-wide instance (`supervisor()`), which
/// is exactly the pre-Thermos-I1 behavior; the struct exists so the
/// lifecycle tests can run sweep / listing / cap assertions against
/// an isolated instance instead of racing other tests on the global
/// registry (see the D110 comment in `process_tests.rs`).
pub(crate) struct Supervisor {
    state: Mutex<RegistryState>,
}

fn supervisor() -> &'static Supervisor {
    static SUPERVISOR: OnceLock<Supervisor> = OnceLock::new();
    SUPERVISOR.get_or_init(Supervisor::new)
}

fn next_handle_id() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(1);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("srv_{n:016x}")
}

/// Start an mlx-lm server. Allocates a port, spawns the configured
/// launcher with the standard arg shape, kicks off a background
/// thread that drains stdout+stderr into a ring buffer, then polls
/// `/health` until the overall budget runs out. On success, the
/// handle is registered and returned. On failure, the child is
/// killed (if started) and any captured output is included in the
/// `StartError` for the caller's diagnostic.
///
/// **Port-race retry (Codex D40 MEDIUM fix).** `allocate_port`
/// binds `127.0.0.1:0`, reads the OS-assigned port, then drops the
/// listener so the child can rebind. A different process can win
/// that port in the gap between the drop and the child's bind.
/// When the health probe times out on the first attempt we treat
/// the port as potentially-lost and retry ONCE with a freshly
/// allocated port; the child of the first attempt is already
/// killed and reaped by `try_start_once`'s error path. A second
/// `HealthTimeout` surfaces honestly — the most likely cause is
/// the model being too big or `mlx-lm` not actually installed,
/// neither of which a third retry would fix.
///
/// Concurrency: safe for concurrent calls, but each call allocates
/// its own port and registers its own handle.
pub fn start_server(options: ServerStartOptions) -> Result<ServerHandle, StartError> {
    supervisor().start_server(options)
}

impl Supervisor {
    pub(crate) fn new() -> Self {
        Self {
            state: Mutex::new(RegistryState {
                slots: HashMap::new(),
                shutting_down: false,
            }),
        }
    }

    pub(crate) fn start_server(
        &self,
        options: ServerStartOptions,
    ) -> Result<ServerHandle, StartError> {
        if options.model_path.as_os_str().is_empty() {
            return Err(StartError::InvalidModelPath);
        }
        // Cap and shutdown gating live inside `try_start_once`: the
        // slot reservation there holds the registry lock across the
        // spawn, which is what makes the cap atomic under concurrent
        // starts (Codex #154 lifecycle fix). No pre-check here — it
        // would only re-answer a question the reservation answers
        // authoritatively.

        // Capture the inputs once so we can replay them on retry. The
        // options enum is `Clone` for exactly this; the supervisor's
        // public API is move-by-value so we don't keep callers
        // re-constructing it.
        let attempt1 = self.try_start_once(options.clone());
        match attempt1 {
            Ok(handle) => Ok(handle),
            Err(StartError::HealthTimeout { .. }) => {
                // OS port race or transient — retry with a fresh port.
                self.try_start_once(options)
            }
            Err(other) => Err(other),
        }
    }
}

/// One spawn-and-poll attempt on the process-wide supervisor.
///
/// `#[cfg(test)]` + `pub(crate)` so the test sibling can compare a
/// single attempt's elapsed time against the public `start_server`'s
/// two-attempt elapsed — the only honest way to assert the retry
/// fired without making the supervisor count attempts itself.
#[cfg(test)]
pub(crate) fn try_start_once(options: ServerStartOptions) -> Result<ServerHandle, StartError> {
    supervisor().try_start_once(options)
}

impl Supervisor {
    /// One spawn-and-poll attempt. Extracted from `start_server` so
    /// the port-race retry can call it twice without duplicating the
    /// lifecycle logic.
    ///
    /// Codex #154 lifecycle fix: the slot reservation, the cap
    /// check, the shutdown gate, and the spawn itself all happen
    /// under ONE registry lock acquisition. That closes two races at
    /// once: (a) concurrent starts cannot both pass a cap check and
    /// then both spawn — the loser sees the winner's `Starting` slot
    /// and refuses before creating a process; (b) an exit sweep can
    /// never miss a mid-startup child — either it acquires the lock
    /// first (this start then sees `shutting_down` and never
    /// spawns), or it acquires it after the reservation landed, in
    /// which case the slot carries the child's pid and the sweep
    /// kills it directly. Holding the mutex across `allocate_port` +
    /// `spawn` is a few milliseconds of syscalls on a user-driven,
    /// rare operation; the slow part (the health poll) runs outside
    /// the lock as before.
    pub(crate) fn try_start_once(
        &self,
        options: ServerStartOptions,
    ) -> Result<ServerHandle, StartError> {
        let ServerStartOptions {
            model_path,
            command: launcher,
            log_level,
            startup_timeout,
            model_id,
        } = options;
        let cmd = launcher.unwrap_or_else(default_mlx_lm_command);
        let startup_timeout = startup_timeout.unwrap_or(DEFAULT_START_TIMEOUT);
        let handle_id = next_handle_id();

        let (mut child, port) = {
            let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
            // Children that exited on their own must not consume cap
            // slots (Codex #154 lifecycle fix).
            state.reap_exited();
            if state.shutting_down {
                return Err(StartError::ShuttingDown);
            }
            if state.slots.len() >= MAX_MANAGED_SERVERS {
                return Err(StartError::RegistryFull);
            }

            let port = allocate_port().map_err(StartError::PortAllocation)?;
            let args = build_command_args(&model_path, port, &log_level);
            let mut command = Command::new(&cmd.program);
            command
                .args(&cmd.args_prefix)
                .args(&args)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
            configure_own_session(&mut command);
            let child = command.spawn().map_err(StartError::Spawn)?;
            state
                .slots
                .insert(handle_id.clone(), ManagedSlot::Starting { pid: child.id() });
            (child, port)
        };

        let pid = child.id();
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        let output = Arc::new(Mutex::new(RingBuffer::new(RING_BUFFER_CAP)));

        if let Some(mut s) = stdout {
            let buf = output.clone();
            thread::spawn(move || drain_into_ring(&mut s, &buf));
        }
        if let Some(mut s) = stderr {
            let buf = output.clone();
            thread::spawn(move || drain_into_ring(&mut s, &buf));
        }

        // Now poll /health within the overall budget.
        match poll_health(port, startup_timeout) {
            Ok(()) => {
                let model_label = model_path.to_string_lossy().into_owned();
                let handle = ServerHandle {
                    id: ServerHandleId(handle_id.clone()),
                    port,
                    pid,
                };
                let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
                if state.shutting_down || !state.slots.contains_key(&handle_id) {
                    // The exit sweep began while we were polling: it
                    // drained our `Starting` slot and SIGKILLed the
                    // pid. Registering now would produce a child the
                    // sweep can never see again, so stop our own copy
                    // (reaping whatever the sweep left) and refuse.
                    drop(state);
                    let _ = stop_child(&mut child);
                    return Err(StartError::ShuttingDown);
                }
                state.slots.insert(
                    handle_id,
                    ManagedSlot::Running(ServerProcess {
                        port,
                        child,
                        output,
                        model_label,
                        started_at_ms: now_unix_ms(),
                        model_id,
                    }),
                );
                Ok(handle)
            }
            Err(health_err) => {
                // Release the reservation on EVERY failure path so a
                // failed start never consumes a cap slot.
                self.state
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .slots
                    .remove(&handle_id);
                let tail = output
                    .lock()
                    .map(|b| b.snapshot())
                    .unwrap_or_else(|_| String::new());
                let _ = stop_child(&mut child);
                match health_err {
                    HealthError::Status(status) => Err(StartError::HealthBadStatus {
                        status,
                        stderr_tail: tail,
                    }),
                    _ => Err(StartError::HealthTimeout { stderr_tail: tail }),
                }
            }
        }
    }
}

/// Spawn `command`'s child in its own session (and therefore its
/// own process group) so a SIGINT to Plume doesn't ALSO fire on the
/// child (and vice versa). The supervisor's stop path signals the
/// child's process group explicitly, which is the right escape
/// hatch — and is also why hard-crash cleanup is impossible from
/// here: a SIGKILLed Plume never runs any sweep, and the detached
/// session means the orphan won't receive a group signal either.
/// Shared with the lifecycle tests so their controlled children get
/// the exact production signal topology.
#[cfg(unix)]
pub(crate) fn configure_own_session(command: &mut Command) {
    use std::os::unix::process::CommandExt;
    unsafe {
        command.pre_exec(|| {
            if libc_setsid() < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
}

#[cfg(not(unix))]
pub(crate) fn configure_own_session(_command: &mut Command) {}

/// Drain a reader into the ring buffer until EOF. Errors swallow —
/// a stuck reader doesn't take the supervisor with it. The reader
/// must implement `Read`; both `ChildStdout` and `ChildStderr` do.
fn drain_into_ring<R: Read>(reader: &mut R, buf: &Arc<Mutex<RingBuffer>>) {
    let mut chunk = [0u8; 1024];
    loop {
        match reader.read(&mut chunk) {
            Ok(0) => break, // EOF
            Ok(n) => {
                if let Ok(mut guard) = buf.lock() {
                    guard.push_bytes(&chunk[..n]);
                }
            }
            Err(_) => break,
        }
    }
}

// --- minimal raw-FFI bindings for the SIGINT escape hatch ---------------
//
// We deliberately avoid pulling in the `libc` or `nix` crate just
// for two function bindings. Stable `std` doesn't expose signals;
// the bindings below are the only ones we need.

#[cfg(unix)]
extern "C" {
    fn kill(pid: i32, sig: i32) -> i32;
    fn setsid() -> i32;
}

#[cfg(unix)]
unsafe fn libc_kill(pid: i32, sig: i32) -> i32 {
    kill(pid, sig)
}

#[cfg(unix)]
unsafe fn libc_setsid() -> i32 {
    setsid()
}

/// Look up the bound port + the model label for a registered handle.
/// D45 chat routing uses both to translate the frontend's `handleId`
/// into a (port, model-string) pair without exposing the supervisor
/// registry itself.
///
/// The `model_label` is the exact `--model` value the supervisor
/// passed at spawn — typically an absolute path under
/// `default_model_dir()`. Codex D45 HIGH: chat requests echo this
/// back in the OpenAI `model` field so a future mlx-lm with
/// dynamic-reload doesn't see a mismatch between the launched model
/// and the request's claimed model id.
///
/// Returns `None` when the id isn't registered — either it was
/// never issued, has been stopped, or belongs to a different
/// Plume instance. The caller surfaces this as `IpcError::NotFound`
/// so the frontend can re-fetch its handle bookkeeping.
pub fn lookup_handle_info(id: &ServerHandleId) -> Option<HandleInfo> {
    supervisor().lookup_handle_info(id)
}

impl Supervisor {
    pub(crate) fn lookup_handle_info(&self, id: &ServerHandleId) -> Option<HandleInfo> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        // A child that exited on its own must resolve to `None` so
        // dispatch rejects the handle as stale instead of routing a
        // request to a dead port (Codex #154 lifecycle fix).
        state.reap_exited();
        match state.slots.get(&id.0) {
            Some(ManagedSlot::Running(s)) => Some(HandleInfo {
                port: s.port,
                model_label: s.model_label.clone(),
            }),
            // `Starting` slots have no issued handle yet; the id
            // cannot legitimately be known to any caller.
            _ => None,
        }
    }
}

/// Resolved view of a registered handle. The chat dispatch wants
/// both the port (where to connect) and the model label (what
/// string to put on the wire's `model` field) atomically with one
/// registry lock acquisition. A tuple would do the job; a struct is
/// stable for adding `pid` / `started_at` / etc. as the supervisor
/// grows additional inspection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandleInfo {
    pub port: u16,
    pub model_label: String,
}

/// D52: monotonic-ish "now" in unix epoch milliseconds, saturating to
/// `0` on the impossible "system clock is before 1970" case. The
/// supervisor doesn't need sub-millisecond precision; uptime + the
/// "started at" label both round to seconds in the UI.
fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Test-only registry helper: insert a `ServerProcess` synthesized
/// from a port and a `Child` stub. The D45 chat-routing tests use
/// this to point a registered handle at a fake HTTP server without
/// actually spawning mlx-lm. Production code uses `start_server`
/// exclusively; this helper is `#[cfg(test)]` so the production
/// binary cannot construct a handle that bypasses health probing.
///
/// `model_label` is the same value `start_server` would record from
/// `options.model_path` — tests pass whatever string they want to
/// see echoed back from `lookup_handle_info`.
#[cfg(test)]
pub(crate) fn register_for_test(
    port: u16,
    child: Child,
    model_label: impl Into<String>,
) -> ServerHandleId {
    supervisor().register_for_test(port, child, model_label, "")
}

#[cfg(test)]
impl Supervisor {
    /// Instance-scoped twin of the free `register_for_test`, with the
    /// caller-side `model_id` exposed so the Thermos-I1 listing tests
    /// can assert the id round-trips.
    pub(crate) fn register_for_test(
        &self,
        port: u16,
        child: Child,
        model_label: impl Into<String>,
        model_id: impl Into<String>,
    ) -> ServerHandleId {
        let id = next_handle_id();
        let output = Arc::new(Mutex::new(RingBuffer::new(RING_BUFFER_CAP)));
        self.state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .slots
            .insert(
                id.clone(),
                ManagedSlot::Running(ServerProcess {
                    port,
                    child,
                    output,
                    model_label: model_label.into(),
                    started_at_ms: now_unix_ms(),
                    model_id: model_id.into(),
                }),
            );
        ServerHandleId(id)
    }

    /// Pids of slots still in their startup window. The
    /// quit-during-start regression polls this to know the moment a
    /// concurrent `try_start_once` has spawned (and reserved) before
    /// it fires the sweep.
    pub(crate) fn starting_pids(&self) -> Vec<u32> {
        let state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state
            .slots
            .values()
            .filter_map(|slot| match slot {
                ManagedSlot::Starting { pid } => Some(*pid),
                ManagedSlot::Running(_) => None,
            })
            .collect()
    }
}

/// D52 test helper: insert a process with a synthetic output buffer
/// pre-populated by the caller. Lets the diagnostics tests assert on
/// log-tail behaviour without spawning an mlx-lm child whose timing is
/// unpredictable. The injected ring buffer is identical to what the
/// production drain threads would build up — same `RING_BUFFER_CAP`,
/// same `push_bytes` semantics.
#[cfg(test)]
pub(crate) fn register_for_test_with_log(
    port: u16,
    child: Child,
    model_label: impl Into<String>,
    log_bytes: &[u8],
) -> ServerHandleId {
    let id = next_handle_id();
    let output = Arc::new(Mutex::new(RingBuffer::new(RING_BUFFER_CAP)));
    if let Ok(mut guard) = output.lock() {
        guard.push_bytes(log_bytes);
    }
    supervisor()
        .state
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .slots
        .insert(
            id.clone(),
            ManagedSlot::Running(ServerProcess {
                port,
                child,
                output,
                model_label: model_label.into(),
                started_at_ms: now_unix_ms(),
                model_id: String::new(),
            }),
        );
    ServerHandleId(id)
}
