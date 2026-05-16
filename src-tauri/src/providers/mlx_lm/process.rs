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
use std::collections::{HashMap, VecDeque};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

/// Hard cap on the per-handle stdout+stderr ring buffer. Sized to
/// hold a typical Python traceback (a few hundred bytes) plus the
/// upstream "Loading model from …" lines mlx-lm prints during
/// startup. 16 KiB is far past either while still preventing a
/// runaway child from filling memory.
pub const RING_BUFFER_CAP: usize = 16 * 1024;

/// Default per-poll TCP connect + read budget on the health probe.
/// One slow probe shouldn't stall the supervisor; the outer loop
/// keeps trying within the overall startup deadline.
const HEALTH_PROBE_PER_ATTEMPT: Duration = Duration::from_millis(500);

/// Steps in the backoff sequence between health-probe attempts.
/// The supervisor reads from the end and pops, so attempt 1 sleeps
/// 50 ms, attempt 2 sleeps 200 ms, attempt 3+ sleeps 500 ms. Tied
/// loosely to mlx-lm's typical first-load latency: weight reads
/// dominate, so the first second of attempts is cheap and frequent.
const HEALTH_BACKOFF_STEPS_MS: &[u64] = &[50, 200, 500];

/// Grace period after SIGINT before falling back to SIGKILL on
/// stop. mlx_lm's `KeyboardInterrupt` handler should call
/// `response_generator.stop_and_join()` plus `httpd.shutdown()`,
/// which completes in under a second on idle servers; three
/// seconds is conservative for an in-flight chat completion to
/// drain.
const STOP_SIGINT_GRACE: Duration = Duration::from_secs(3);

/// Default overall startup budget for `start_server`. mlx-lm
/// loading a 7B weight set from a cold cache can spend 10–25 s
/// reading shards on a typical NVMe; thirty seconds keeps a hard
/// stop on the worst case while not failing healthy starts.
pub const DEFAULT_START_TIMEOUT: Duration = Duration::from_secs(30);

/// Allocate an ephemeral TCP port on 127.0.0.1. Binds, reads the
/// OS-assigned port, then immediately drops the listener so the
/// child can rebind. Returns an `io::Error` if the bind itself
/// fails — that's a system-level failure (no free ports, etc),
/// not the race window.
///
/// The race window between drop and the child's bind is documented
/// in `docs/MLX_RUNTIME.md § Port allocation`; `start_server`
/// addresses it by retrying once on a health-probe timeout.
pub fn allocate_port() -> std::io::Result<u16> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let port = listener.local_addr()?.port();
    drop(listener);
    Ok(port)
}

/// Non-deprecated mlx-lm launcher per `docs/MLX_RUNTIME.md § CLI`.
/// Splits the program (which we want testable) from the
/// args-prefix (the subcommand shape `mlx-lm` requires). Tests
/// override `program` to a path that does not need `mlx-lm`
/// installed.
#[derive(Debug, Clone)]
pub struct MlxLmCommand {
    /// Executable to run. Production: `"python"` resolved via
    /// `PATH`. Tests: an absolute path to a binary that can read
    /// the same args (e.g. `/usr/bin/python3` for a fake HTTP
    /// server, or `/bin/sleep` for shutdown-only tests).
    pub program: String,
    /// Args inserted before the `--model` / `--host` / `--port`
    /// args `build_command_args` produces. Production:
    /// `["-m", "mlx_lm", "server"]`. Tests: whatever args their
    /// fake binary expects (often empty for a `sleep N` stub).
    pub args_prefix: Vec<String>,
}

/// The production launcher: `python -m mlx_lm server …`. See the
/// rationale on the subcommand form in `docs/MLX_RUNTIME.md`.
pub fn default_mlx_lm_command() -> MlxLmCommand {
    MlxLmCommand {
        program: "python".to_string(),
        args_prefix: vec!["-m".into(), "mlx_lm".into(), "server".into()],
    }
}

/// Compose the trailing args for an mlx-lm-style chat server:
/// `--model PATH --host 127.0.0.1 --port N --log-level LEVEL`.
/// Pulled out so tests can assert on the exact shape without
/// running the supervisor. The `model_path` is rendered with
/// `to_string_lossy` so a path containing a non-UTF-8 byte still
/// goes through (the kernel won't lose it).
pub fn build_command_args(model_path: &std::path::Path, port: u16, log_level: &str) -> Vec<String> {
    vec![
        "--model".to_string(),
        model_path.to_string_lossy().into_owned(),
        "--host".to_string(),
        "127.0.0.1".to_string(),
        "--port".to_string(),
        port.to_string(),
        "--log-level".to_string(),
        log_level.to_string(),
    ]
}

/// Bounded byte buffer for captured stdout + stderr. Push beyond
/// `capacity` drops the oldest bytes so a runaway child's output
/// can't grow memory unbounded. `snapshot` returns a lossy-UTF-8
/// view for inclusion in error messages and tracing logs.
#[derive(Debug)]
pub struct RingBuffer {
    capacity: usize,
    data: VecDeque<u8>,
}

impl RingBuffer {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            data: VecDeque::with_capacity(capacity),
        }
    }

    pub fn push_bytes(&mut self, bytes: &[u8]) {
        if self.capacity == 0 {
            return;
        }
        for &byte in bytes {
            if self.data.len() == self.capacity {
                self.data.pop_front();
            }
            self.data.push_back(byte);
        }
    }

    pub fn snapshot(&self) -> String {
        // Iterating + collecting through `from_utf8_lossy` would
        // require a contiguous slice; `make_contiguous` would
        // mutate the buffer and we want an immutable read. Build a
        // contiguous Vec and decode in one go.
        let bytes: Vec<u8> = self.data.iter().copied().collect();
        String::from_utf8_lossy(&bytes).into_owned()
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.data.len()
    }

    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }
}

/// `GET /health` against `127.0.0.1:<port>` with the given overall
/// budget. Returns `Ok(())` when the server answers `200 OK`.
/// Loops with `HEALTH_BACKOFF_STEPS_MS` between attempts until the
/// deadline expires; transient connect refusals (the child is
/// still binding) are retries, not fatal. A non-200 status line is
/// fatal — that means the server is up but returning an unexpected
/// shape and the caller should surface a clear error instead of
/// polling forever.
pub fn poll_health(port: u16, overall_timeout: Duration) -> Result<(), HealthError> {
    let deadline = Instant::now() + overall_timeout;
    let mut attempt: usize = 0;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(HealthError::Timeout);
        }
        let per_attempt = HEALTH_PROBE_PER_ATTEMPT.min(remaining);
        match try_health_probe(port, per_attempt) {
            Ok(()) => return Ok(()),
            Err(HealthError::Status(s)) => return Err(HealthError::Status(s)),
            // ConnectRefused / Timeout / Io are transient — fall
            // through to the backoff and try again.
            Err(_) => {}
        }
        let backoff_ms = HEALTH_BACKOFF_STEPS_MS[attempt.min(HEALTH_BACKOFF_STEPS_MS.len() - 1)];
        attempt = attempt.saturating_add(1);
        let backoff = Duration::from_millis(backoff_ms);
        let until = Instant::now() + backoff;
        if until > deadline {
            // No point sleeping past the deadline; bail with
            // Timeout so the caller's error message is honest.
            return Err(HealthError::Timeout);
        }
        thread::sleep(backoff);
    }
}

#[derive(Debug)]
pub enum HealthError {
    /// Connect refused (server still binding) or socket timed out
    /// before the request completed. Transient; the supervisor's
    /// loop retries.
    ConnectRefused,
    /// Server answered but with a non-200 status line. The caller
    /// should NOT retry — the runtime is up but speaking a
    /// different protocol.
    Status(u16),
    /// Per-attempt or overall deadline expired.
    Timeout,
    /// Underlying I/O error (read, write, socket address parse).
    /// `#[allow(dead_code)]` because tests don't currently
    /// construct one — the surrounding code reads them via
    /// `Debug` and `matches!` only — but the variant is part of
    /// the supervisor's error contract.
    #[allow(dead_code)]
    Io(std::io::Error),
}

fn try_health_probe(port: u16, per_attempt: Duration) -> Result<(), HealthError> {
    let addr: SocketAddr = ([127, 0, 0, 1], port).into();
    let mut stream = TcpStream::connect_timeout(&addr, per_attempt).map_err(|err| {
        if err.kind() == std::io::ErrorKind::ConnectionRefused {
            HealthError::ConnectRefused
        } else if err.kind() == std::io::ErrorKind::TimedOut {
            HealthError::Timeout
        } else {
            HealthError::Io(err)
        }
    })?;
    stream
        .set_read_timeout(Some(per_attempt))
        .map_err(HealthError::Io)?;
    stream
        .set_write_timeout(Some(per_attempt))
        .map_err(HealthError::Io)?;
    // Minimal HTTP/1.1 request. `Connection: close` so the server
    // doesn't hold the socket open after responding.
    let request =
        b"GET /health HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\nAccept: */*\r\n\r\n";
    stream.write_all(request).map_err(HealthError::Io)?;

    // Read just enough of the response to parse the status line.
    // 1 KiB is plenty: status line + a few headers fit comfortably.
    let mut buf = [0u8; 1024];
    let read = stream.read(&mut buf).map_err(HealthError::Io)?;
    if read == 0 {
        return Err(HealthError::ConnectRefused);
    }
    let head = std::str::from_utf8(&buf[..read]).unwrap_or("");
    // "HTTP/1.1 200 OK\r\n..." — parse the status code.
    let status_code = head
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse::<u16>().ok())
        .ok_or(HealthError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "could not parse HTTP status line",
        )))?;
    if status_code == 200 {
        Ok(())
    } else {
        Err(HealthError::Status(status_code))
    }
}

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
}

impl Default for ServerStartOptions {
    fn default() -> Self {
        Self {
            model_path: PathBuf::new(),
            command: None,
            log_level: "INFO".to_string(),
            startup_timeout: None,
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
    /// Held for the lifetime of the registration; future restart /
    /// inspect code reads it. `dead_code` for now.
    #[allow(dead_code)]
    port: u16,
    child: Child,
    /// Background reader threads keep pushing into this Arc; future
    /// `providers.serverLogs(handle)` verb will read it. Holding the
    /// Arc here keeps the ring alive after start_server returns.
    #[allow(dead_code)]
    output: Arc<Mutex<RingBuffer>>,
}

fn registry() -> &'static Mutex<HashMap<String, ServerProcess>> {
    static REGISTRY: OnceLock<Mutex<HashMap<String, ServerProcess>>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
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
    if options.model_path.as_os_str().is_empty() {
        return Err(StartError::InvalidModelPath);
    }

    // Capture the inputs once so we can replay them on retry. The
    // options enum is `Clone` for exactly this; the supervisor's
    // public API is move-by-value so we don't keep callers
    // re-constructing it.
    let attempt1 = try_start_once(options.clone());
    match attempt1 {
        Ok(handle) => Ok(handle),
        Err(StartError::HealthTimeout { .. }) => {
            // OS port race or transient — retry with a fresh port.
            try_start_once(options)
        }
        Err(other) => Err(other),
    }
}

/// One spawn-and-poll attempt. Extracted from `start_server` so
/// the port-race retry can call it twice without duplicating the
/// lifecycle logic.
///
/// `pub(crate)` so the test sibling can compare a single attempt's
/// elapsed time against the public `start_server`'s
/// two-attempt elapsed — the only honest way to assert the retry
/// fired without making the supervisor count attempts itself.
pub(crate) fn try_start_once(options: ServerStartOptions) -> Result<ServerHandle, StartError> {
    let cmd = options.command.unwrap_or_else(default_mlx_lm_command);
    let log_level = options.log_level;
    let startup_timeout = options.startup_timeout.unwrap_or(DEFAULT_START_TIMEOUT);

    let port = allocate_port().map_err(StartError::PortAllocation)?;
    let args = build_command_args(&options.model_path, port, &log_level);

    let mut command = Command::new(&cmd.program);
    command
        .args(&cmd.args_prefix)
        .args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    {
        // Spawn the child in its own process group so a SIGINT to
        // Plume doesn't ALSO fire on the child (and vice versa).
        // The supervisor's stop() sends SIGINT to the child's PID
        // explicitly, which is the right escape hatch.
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
    let mut child = command.spawn().map_err(StartError::Spawn)?;
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
            let handle_id = next_handle_id();
            let handle = ServerHandle {
                id: ServerHandleId(handle_id.clone()),
                port,
                pid,
            };
            registry().lock().unwrap_or_else(|e| e.into_inner()).insert(
                handle_id,
                ServerProcess {
                    port,
                    child,
                    output,
                },
            );
            Ok(handle)
        }
        Err(HealthError::Status(status)) => {
            let tail = output
                .lock()
                .map(|b| b.snapshot())
                .unwrap_or_else(|_| String::new());
            let _ = stop_child(&mut child);
            Err(StartError::HealthBadStatus {
                status,
                stderr_tail: tail,
            })
        }
        Err(_) => {
            let tail = output
                .lock()
                .map(|b| b.snapshot())
                .unwrap_or_else(|_| String::new());
            let _ = stop_child(&mut child);
            Err(StartError::HealthTimeout { stderr_tail: tail })
        }
    }
}

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

/// Stop a server by handle id. Removes the registration first so
/// the port is free for the next start even if the kill itself
/// hits an error. On unix, sends SIGINT to the child's process
/// group and waits up to `STOP_SIGINT_GRACE`; if the child hasn't
/// exited, escalates to SIGKILL across the WHOLE process group
/// (Codex D40 fix) — `Child::kill` alone would only signal the
/// direct child, leaving any grandchildren mlx-lm spawned alive.
/// `Child::kill` + `wait` still runs after the pgroup SIGKILL so
/// std reaps the zombie. On Windows, immediate `Child::kill`.
pub fn stop_server(id: &ServerHandleId) -> Result<(), StopError> {
    let mut reg = registry().lock().unwrap_or_else(|e| e.into_inner());
    let mut server = reg.remove(&id.0).ok_or(StopError::UnknownHandle)?;
    drop(reg); // free the registry mutex while we wait on the child
    stop_child(&mut server.child).map_err(StopError::Io)?;
    Ok(())
}

fn stop_child(child: &mut Child) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        let pid = child.id();
        // Send SIGINT to the child's own process group (negative
        // pid) so any grandchildren mlx-lm spawned also see it.
        // We set up its own session in pre_exec, so the negative
        // pgid is the child's pgid.
        unsafe {
            // Best-effort; ignore EPERM/ESRCH (already exited).
            let _ = libc_kill(-(pid as i32), 2); // 2 == SIGINT
        }
        let deadline = Instant::now() + STOP_SIGINT_GRACE;
        loop {
            match child.try_wait()? {
                Some(_status) => return Ok(()),
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
            let _ = libc_kill(-(pid as i32), 9); // 9 == SIGKILL
        }
        // Fall through to `Child::kill` and `wait` regardless so
        // the std side reaps the zombie. `kill(9)` against an
        // already-exited child is a harmless ESRCH.
        let _ = child.kill();
        let _ = child.wait()?;
        Ok(())
    }
    #[cfg(not(unix))]
    {
        let _ = child.kill();
        let _ = child.wait()?;
        Ok(())
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

/// Test-only registry inspector: returns the number of currently
/// tracked servers. Lets the tests assert that the registry empties
/// after every successful stop.
#[cfg(test)]
pub(crate) fn registry_len() -> usize {
    registry().lock().unwrap_or_else(|e| e.into_inner()).len()
}
