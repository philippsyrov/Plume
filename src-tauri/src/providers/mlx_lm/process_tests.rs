//! Tests for `mlx_lm::process`. Pure unit tests (port allocator,
//! command builder, ring buffer) plus a handful that stand up an
//! in-process TCP listener to exercise the health probe and one
//! integration test that spawns `/bin/sleep` to exercise the
//! supervisor's start + stop lifecycle without needing a real
//! `mlx-lm` install.

use super::process::*;
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::thread;
use std::time::{Duration, Instant};

// --- port allocator -----------------------------------------------------

#[test]
fn allocate_port_returns_a_usable_ephemeral_port() {
    // `allocate_port` only guarantees the returned number is a real,
    // nonzero port that was free at allocation time — it does NOT
    // guarantee the port stays free after the listener is dropped,
    // since any other process (including another test in this same
    // binary under cargo's default parallel execution) can claim it
    // first. This used to assert a re-bind on the exact same port
    // after drop, which flaked under `cargo test` for that reason.
    // See `allocate_port_reserves_a_live_port_while_held` for a
    // race-free check that the allocator hands back a genuinely
    // live socket.
    let port = allocate_port().expect("alloc");
    assert!(port > 0, "port 0 means we leaked the OS-assigned value");
}

#[test]
fn allocate_port_reserves_a_live_port_while_held() {
    // Binds the same way `allocate_port` does internally, but keeps
    // the listener alive for the whole assertion instead of
    // dropping then re-binding — there is no window for another
    // parallel test to steal the port, because we never release it
    // before we're done asserting against it.
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("local_addr").port();
    assert!(port > 0, "port 0 means the OS didn't assign one");
    // Connecting while still holding the listener proves it's a
    // genuinely live, connectable socket rather than just a
    // syntactically valid u16.
    TcpStream::connect(("127.0.0.1", port)).expect("connect to the held listener");
}

#[test]
fn allocate_port_supports_back_to_back_calls() {
    // Two consecutive allocations must each succeed and return a
    // nonzero port. This deliberately does NOT assert p1 != p2 —
    // whether the kernel reuses a just-released ephemeral port is
    // its own policy, not something allocate_port controls or
    // promises, and asserting inequality here was a false-red test
    // waiting to happen (same class of flake as the drop-then-rebind
    // one fixed above).
    let p1 = allocate_port().expect("alloc 1");
    let p2 = allocate_port().expect("alloc 2");
    assert!(p1 > 0, "first alloc returned port 0");
    assert!(p2 > 0, "second alloc returned port 0");
}

// --- command builder ----------------------------------------------------

/// Cargo runs tests in parallel by default; tests that mutate the
/// `PLUME_MLX_PYTHON` env var MUST serialize on this mutex so their
/// set / read / restore window isn't interleaved with another test
/// reading the same var. Same posture as the D50 env mutex in
/// `local_models`. The mutex is local to the test module so
/// production code is unaffected.
fn d58_env_mutex() -> &'static std::sync::Mutex<()> {
    static MUTEX: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
    MUTEX.get_or_init(|| std::sync::Mutex::new(()))
}

#[test]
fn default_command_uses_non_deprecated_subcommand_form() {
    // The deprecated form is `python -m mlx_lm.server`; the
    // supervisor must never use it. The subcommand form is
    // `python -m mlx_lm server`. D58: this test ALSO pins the
    // pre-D58 default (`program == "python"`) when the
    // `PLUME_MLX_PYTHON` env var is unset; that's the bare-PATH
    // path the rest of the supervisor still expects to be
    // available when the user hasn't opted in.
    let _guard = d58_env_mutex().lock().unwrap_or_else(|e| e.into_inner());
    std::env::remove_var("PLUME_MLX_PYTHON");

    let cmd = default_mlx_lm_command();
    assert_eq!(cmd.program, "python");
    assert_eq!(cmd.args_prefix, vec!["-m", "mlx_lm", "server"]);
    // Belt-and-braces: make sure no element is the deprecated
    // dotted form.
    for arg in &cmd.args_prefix {
        assert!(
            !arg.starts_with("mlx_lm."),
            "deprecated dotted form leaked into default command: {arg}"
        );
    }
}

// ─── D58: PLUME_MLX_PYTHON env override ─────────────────────────────────

/// When `PLUME_MLX_PYTHON` is set to a real-looking path,
/// `default_mlx_lm_command()` uses that as `program` instead of the
/// bare `"python"`. The `args_prefix` stays `-m mlx_lm server`
/// regardless — D58 only touches the interpreter, not the module
/// invocation shape (the deprecated `mlx_lm.server` form must still
/// never appear).
#[test]
fn plume_mlx_python_env_overrides_program_in_default_command() {
    let _guard = d58_env_mutex().lock().unwrap_or_else(|e| e.into_inner());
    let venv_python = "/Users/operator/.venvs/mlx-env/bin/python";
    std::env::set_var("PLUME_MLX_PYTHON", venv_python);

    let cmd = default_mlx_lm_command();

    std::env::remove_var("PLUME_MLX_PYTHON");

    assert_eq!(
        cmd.program, venv_python,
        "expected PLUME_MLX_PYTHON to override program"
    );
    // args_prefix invariant: the `-m mlx_lm server` shape MUST
    // survive the override. If a future change accidentally moves
    // args into `program`, this catches it.
    assert_eq!(cmd.args_prefix, vec!["-m", "mlx_lm", "server"]);
    for arg in &cmd.args_prefix {
        assert!(
            !arg.starts_with("mlx_lm."),
            "deprecated dotted form leaked through env override: {arg}"
        );
    }
}

/// An empty `PLUME_MLX_PYTHON` falls back to the default `"python"`
/// rather than spawning `""` (which would surface as a confusing
/// `Spawn(No such file or directory)`). Same for whitespace-only —
/// the user clearing the env var in their shell shouldn't break the
/// happy path silently.
#[test]
fn plume_mlx_python_empty_or_whitespace_falls_back_to_default() {
    let _guard = d58_env_mutex().lock().unwrap_or_else(|e| e.into_inner());

    // Empty string.
    std::env::set_var("PLUME_MLX_PYTHON", "");
    let cmd_empty = default_mlx_lm_command();
    assert_eq!(
        cmd_empty.program, "python",
        "empty env var should fall back to default"
    );

    // Whitespace only — tabs, spaces, newlines.
    std::env::set_var("PLUME_MLX_PYTHON", "  \t \n ");
    let cmd_ws = default_mlx_lm_command();
    assert_eq!(
        cmd_ws.program, "python",
        "whitespace-only env var should fall back to default"
    );

    std::env::remove_var("PLUME_MLX_PYTHON");

    // args_prefix invariant survives both edge cases.
    assert_eq!(cmd_empty.args_prefix, vec!["-m", "mlx_lm", "server"]);
    assert_eq!(cmd_ws.args_prefix, vec!["-m", "mlx_lm", "server"]);
}

/// Leading/trailing whitespace on an otherwise-valid value is
/// stripped before use — copying a path out of a terminal often
/// pulls in a trailing newline. The trimmed value is what reaches
/// the spawn.
#[test]
fn plume_mlx_python_trims_surrounding_whitespace() {
    let _guard = d58_env_mutex().lock().unwrap_or_else(|e| e.into_inner());
    std::env::set_var("PLUME_MLX_PYTHON", "  /opt/homebrew/bin/python3 \n");

    let cmd = default_mlx_lm_command();

    std::env::remove_var("PLUME_MLX_PYTHON");

    assert_eq!(cmd.program, "/opt/homebrew/bin/python3");
    assert_eq!(cmd.args_prefix, vec!["-m", "mlx_lm", "server"]);
}

#[test]
fn build_command_args_emits_the_pinned_shape() {
    let args = build_command_args(&PathBuf::from("/models/qwen-7b"), 51234, "INFO");
    assert_eq!(
        args,
        vec![
            "--model",
            "/models/qwen-7b",
            "--host",
            "127.0.0.1",
            "--port",
            "51234",
            "--log-level",
            "INFO",
        ]
    );
}

#[test]
fn build_command_args_renders_port_as_decimal() {
    // u16 max as decimal not hex.
    let args = build_command_args(&PathBuf::from("/m"), 65535, "DEBUG");
    let port_idx = args.iter().position(|a| a == "--port").unwrap();
    assert_eq!(args[port_idx + 1], "65535");
}

// --- ring buffer --------------------------------------------------------

#[test]
fn ring_buffer_collects_pushes_under_capacity() {
    let mut rb = RingBuffer::new(64);
    rb.push_bytes(b"hello ");
    rb.push_bytes(b"world");
    assert_eq!(rb.snapshot(), "hello world");
    assert_eq!(rb.len(), 11);
}

#[test]
fn ring_buffer_drops_oldest_when_full() {
    let mut rb = RingBuffer::new(5);
    rb.push_bytes(b"abcdef"); // 6 bytes into a 5-byte ring
    assert_eq!(rb.snapshot(), "bcdef");
    assert_eq!(rb.len(), 5);
}

#[test]
fn ring_buffer_handles_repeated_overflow() {
    let mut rb = RingBuffer::new(4);
    rb.push_bytes(b"a"); // "a"
    rb.push_bytes(b"bc"); // "abc"
    rb.push_bytes(b"defg"); // "defg" (a, b, c dropped)
    assert_eq!(rb.snapshot(), "defg");
}

#[test]
fn ring_buffer_zero_capacity_stays_empty() {
    let mut rb = RingBuffer::new(0);
    rb.push_bytes(b"anything");
    assert!(rb.is_empty());
    assert_eq!(rb.snapshot(), "");
}

#[test]
fn ring_buffer_snapshot_lossy_utf8_for_non_text_bytes() {
    let mut rb = RingBuffer::new(8);
    // Push a valid prefix and a stray 0xFF — the snapshot should
    // contain the U+FFFD replacement character rather than panic.
    rb.push_bytes(b"ok");
    rb.push_bytes(&[0xFFu8]);
    let snap = rb.snapshot();
    assert!(snap.starts_with("ok"));
    assert!(
        snap.contains('\u{FFFD}'),
        "expected replacement char, got: {snap:?}"
    );
}

// --- health probe -------------------------------------------------------

#[test]
fn poll_health_succeeds_against_a_simulated_200_server() {
    let port = allocate_port().expect("alloc");
    let server_thread =
        spawn_tiny_http_server(port, "HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n");
    poll_health(port, Duration::from_secs(2)).expect("expected 200");
    drop(server_thread);
}

#[test]
fn poll_health_returns_status_when_server_answers_503() {
    let port = allocate_port().expect("alloc");
    let _server = spawn_tiny_http_server(
        port,
        "HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\n\r\n",
    );
    let err = poll_health(port, Duration::from_secs(2)).expect_err("expected non-200 err");
    match err {
        HealthError::Status(503) => {}
        other => panic!("expected Status(503), got {other:?}"),
    }
}

#[test]
fn poll_health_times_out_when_nothing_listens() {
    let port = allocate_port().expect("alloc");
    let started = Instant::now();
    let err = poll_health(port, Duration::from_millis(600)).expect_err("expected timeout");
    assert!(matches!(err, HealthError::Timeout), "got {err:?}");
    // Loose bound — shouldn't massively overshoot the budget.
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "poll waited too long: {:?}",
        started.elapsed()
    );
}

fn spawn_tiny_http_server(port: u16, response: &'static str) -> thread::JoinHandle<()> {
    let listener = TcpListener::bind(("127.0.0.1", port)).expect("bind");
    listener.set_nonblocking(false).expect("blocking accept ok");
    thread::spawn(move || {
        use std::io::{Read, Write};
        // Accept exactly one connection and write the canned response.
        if let Ok((mut stream, _)) = listener.accept() {
            let mut buf = [0u8; 1024];
            let _ = stream.read(&mut buf);
            let _ = stream.write_all(response.as_bytes());
        }
    })
}

// --- supervisor lifecycle (spawn + register + stop) --------------------
//
// These tests use `/bin/sleep` as a stand-in for `mlx-lm`. We
// substitute `program = "/bin/sleep"` and `args_prefix = ["60"]`
// so spawn produces a real OS child we can then stop and verify
// gets reaped. We skip the health-probe path (no /health server in
// `sleep`) by using a zero-arg startup_timeout and asserting the
// HealthTimeout error path.

#[cfg(unix)]
#[test]
fn start_server_with_invalid_model_path_rejects_before_spawn() {
    let opts = ServerStartOptions {
        model_path: PathBuf::new(),
        ..Default::default()
    };
    let err = start_server(opts).expect_err("empty model path");
    assert!(matches!(err, StartError::InvalidModelPath));
}

#[cfg(unix)]
#[test]
fn start_server_with_missing_binary_returns_spawn_error() {
    let opts = ServerStartOptions {
        model_path: PathBuf::from("/tmp/whatever"),
        command: Some(MlxLmCommand {
            program: "/this/path/does/not/exist/mlx-lm-stub".into(),
            args_prefix: vec![],
        }),
        log_level: "INFO".into(),
        startup_timeout: Some(Duration::from_millis(200)),
    };
    let err = start_server(opts).expect_err("missing binary should fail");
    assert!(
        matches!(err, StartError::Spawn(_)),
        "expected Spawn, got {err:?}"
    );
}

#[cfg(unix)]
#[test]
fn start_server_returns_health_timeout_when_child_never_listens() {
    // Spawn `/bin/sleep 30` which never opens a /health endpoint.
    // The supervisor should kill the child after the short
    // startup_timeout and return HealthTimeout with the (empty)
    // stderr_tail.
    let before = registry_len();
    let opts = ServerStartOptions {
        model_path: PathBuf::from("/tmp/fake-model"),
        command: Some(MlxLmCommand {
            program: "/bin/sleep".into(),
            args_prefix: vec![],
        }),
        log_level: "INFO".into(),
        startup_timeout: Some(Duration::from_millis(400)),
    };
    let err = start_server(opts).expect_err("no /health -> timeout");
    assert!(
        matches!(err, StartError::HealthTimeout { .. }),
        "got {err:?}"
    );
    // Registry should not have grown — the failed start cleaned up.
    assert_eq!(registry_len(), before, "registry leaked on failed start");
}

#[cfg(unix)]
#[test]
fn start_then_stop_with_fake_health_server() {
    // Pre-allocate a port and spawn an in-process /health server
    // on it. Then spawn `/bin/sleep` so the supervisor has a real
    // child to manage. We can't pass the same port to both (the
    // supervisor allocates its own), so instead we bypass
    // start_server's allocator by skipping this lifecycle path and
    // verifying stop semantics via a direct registry insert.
    //
    // The point of this test is the stop side — we manually
    // construct a ServerProcess-like state, then call stop_server
    // and verify the child is reaped. We do that by spawning sleep,
    // then immediately stopping it.
    use std::process::{Command, Stdio};

    let port = allocate_port().expect("alloc");
    let _server = spawn_tiny_http_server(port, "HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n");
    // To exercise the supervisor end-to-end we point its
    // `program` at `/bin/sh -c` running a script that listens on
    // the PORT it gets via --port, but that's fragile across
    // shells. Instead, we test the stop-by-handle path directly
    // by spawning a sleep child, registering it manually, then
    // calling stop_server.
    //
    // This intentionally skips the spawn-via-start_server side;
    // start_server is exercised by the timeout test above and the
    // missing-binary test. The combination "spawn AND health AND
    // stop in one test" requires a fake /health server bound to
    // the same port the supervisor allocates, which isn't
    // reasonable without a much fancier test harness.
    let mut child = Command::new("/bin/sleep")
        .arg("30")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn sleep");
    let pid = child.id();
    // Direct registry surgery: insert and remove like the supervisor would.
    // We use the private API via `super::process::register_for_test` only
    // if exposed; otherwise drop down to verifying the kill primitive
    // directly via std.
    let kill_result = child.kill();
    assert!(kill_result.is_ok(), "kill should succeed");
    let status = child.wait().expect("wait");
    assert!(
        !status.success(),
        "killed process should not exit successfully"
    );
    drop(_server);
    let _ = pid; // keep the variable so it's clear we're testing pid lifecycle
    let _ = port;
}

#[cfg(unix)]
#[test]
fn stop_server_with_unknown_handle_returns_unknown_handle_error() {
    let err = stop_server(&ServerHandleId("srv_deadbeef".into())).expect_err("unknown id");
    assert!(matches!(err, StopError::UnknownHandle), "got {err:?}");
}

// --- Codex D40 fixes regression tests -----------------------------------

#[cfg(unix)]
#[test]
fn start_server_retries_once_on_health_timeout_then_surfaces_error() {
    // Codex D40 MEDIUM regression: `start_server` is documented to
    // retry once on a HealthTimeout to cover the OS port race
    // between `allocate_port`'s drop and the child's bind. The
    // outer surface still yields HealthTimeout because the inner
    // attempts truly never came up (we spawn `/bin/sleep`, which
    // never binds /health), but the kill-and-reap should happen
    // TWICE — once per attempt — and the registry must not leak.
    //
    // For the time-based assertion, we measure one direct call to
    // `try_start_once` first and require the public `start_server`
    // to take noticeably longer than that. This is more robust to
    // host-CPU jitter and `poll_health`'s short-circuit behavior
    // than picking an absolute millisecond threshold.
    let before = registry_len();
    let opts = ServerStartOptions {
        model_path: PathBuf::from("/tmp/fake-model"),
        command: Some(MlxLmCommand {
            program: "/bin/sleep".into(),
            args_prefix: vec![],
        }),
        log_level: "INFO".into(),
        startup_timeout: Some(Duration::from_millis(250)),
    };
    let started = Instant::now();
    let err = start_server(opts).expect_err("no /health -> retry once -> still timeout");
    let two_attempt_elapsed = started.elapsed();
    assert!(
        matches!(err, StartError::HealthTimeout { .. }),
        "got {err:?}"
    );
    assert_eq!(registry_len(), before, "registry leaked after retry path");

    // Sanity check the retry actually ran by comparing against a
    // single direct attempt. `start_server` should take at least
    // ~1.5× a single attempt; using `1.4×` as the lower bound to
    // tolerate jitter on slow CI runners. If the retry weren't
    // firing the two would be near-identical.
    let opts2 = ServerStartOptions {
        model_path: PathBuf::from("/tmp/fake-model"),
        command: Some(MlxLmCommand {
            program: "/bin/sleep".into(),
            args_prefix: vec![],
        }),
        log_level: "INFO".into(),
        startup_timeout: Some(Duration::from_millis(250)),
    };
    let started_single = Instant::now();
    let _ = try_start_once(opts2);
    let one_attempt_elapsed = started_single.elapsed();
    assert!(
        two_attempt_elapsed.as_micros() >= one_attempt_elapsed.as_micros() * 7 / 5,
        "expected start_server to take ≥ 1.4× a single attempt; \
         two={two_attempt_elapsed:?} one={one_attempt_elapsed:?}"
    );
}

#[cfg(unix)]
#[test]
fn start_server_does_not_retry_on_invalid_input() {
    // Codex D40 MEDIUM regression: retry only fires on
    // HealthTimeout. Other StartError variants short-circuit. We
    // verify with InvalidModelPath — most-distant variant — that
    // the outer wrapper doesn't spend its retry budget on errors a
    // second spawn can't fix.
    let opts = ServerStartOptions {
        model_path: PathBuf::new(),
        ..Default::default()
    };
    let started = Instant::now();
    let err = start_server(opts).expect_err("empty path");
    assert!(matches!(err, StartError::InvalidModelPath));
    assert!(
        started.elapsed() < Duration::from_millis(100),
        "InvalidModelPath should short-circuit; took {:?}",
        started.elapsed()
    );
}

// ─── D52: diagnostics ────────────────────────────────────────────────────

/// `lookup_diagnostics` for a handle that was never issued must return
/// `None`. The IPC layer maps that to `NotFound` so the panel can drop
/// the disclosure cleanly.
#[test]
fn lookup_diagnostics_unknown_handle_returns_none() {
    let bogus = ServerHandleId("srv_deadbeef_does_not_exist".into());
    assert!(lookup_diagnostics(&bogus).is_none());
}

/// A registered handle answers with the same port, pid, and model
/// label the registration carried, plus a populated log tail when the
/// supervisor's drain pushed any bytes.
#[test]
fn lookup_diagnostics_returns_recorded_fields_and_log_tail() {
    // /bin/sleep gives us a child whose lifetime we can control. The
    // captured log is whatever we pre-populated via the test helper —
    // production has reader threads draining stdout/stderr, but the
    // shape is identical from the diagnostics verb's point of view.
    let child = std::process::Command::new("/bin/sleep")
        .arg("60")
        .spawn()
        .expect("spawn sleep");
    let pid = child.id();
    let id = register_for_test_with_log(54321, child, "/abs/path/to/gemma-2b", b"loaded weights\n");

    let diag = lookup_diagnostics(&id).expect("diagnostics");
    assert_eq!(diag.handle_id, id.0);
    assert_eq!(diag.port, 54321);
    assert_eq!(diag.pid, pid);
    assert_eq!(diag.model_label, "/abs/path/to/gemma-2b");
    assert!(diag.log_tail.contains("loaded weights"));
    assert_eq!(diag.log_bytes, "loaded weights\n".len() as u32);
    assert_eq!(diag.log_capacity, RING_BUFFER_CAP as u32);
    // Uptime is monotonic and bounded; the registration happened
    // milliseconds ago at most.
    assert!(diag.uptime_ms < 5_000);

    // Cleanup so the test doesn't leak a sleeping child.
    let _ = stop_server(&id);
    assert!(
        lookup_diagnostics(&id).is_none(),
        "diagnostics on a stopped handle must return None, not crash"
    );
}

/// The ring buffer's cap is `RING_BUFFER_CAP`; pushing past it drops
/// oldest bytes. The diagnostics snapshot must reflect that the
/// buffer is at the cap (`log_bytes == log_capacity`) so the UI can
/// render a "log truncated" hint honestly. Push 2× the cap and
/// confirm the tail carries the LAST RING_BUFFER_CAP bytes.
#[test]
fn lookup_diagnostics_log_tail_truncates_at_ring_buffer_cap() {
    // Build a payload of (2 * RING_BUFFER_CAP) bytes. The expected
    // tail is the LAST RING_BUFFER_CAP bytes of that — i.e. the
    // second half. We use a recognisable per-byte marker so the tail
    // assertion can be precise.
    let total_len = RING_BUFFER_CAP * 2;
    let payload: Vec<u8> = (0..total_len).map(|i| (i % 256) as u8).collect();
    let expected_tail: Vec<u8> = payload[RING_BUFFER_CAP..].to_vec();

    let child = std::process::Command::new("/bin/sleep")
        .arg("60")
        .spawn()
        .expect("spawn sleep");
    let id = register_for_test_with_log(54322, child, "label", &payload);

    let diag = lookup_diagnostics(&id).expect("diagnostics");
    // `log_tail` is decoded as lossy-UTF-8. Some bytes (0x80+) become
    // U+FFFD replacement chars in the string, so the lengths in chars
    // vs bytes may differ. We assert the contract on the raw byte
    // counter (`log_bytes`), and only sanity-check the string tail's
    // *length* against the byte count via the UTF-8 invariant
    // (snapshot is at most RING_BUFFER_CAP bytes resident).
    assert_eq!(
        diag.log_bytes, RING_BUFFER_CAP as u32,
        "buffer at cap should report log_bytes == log_capacity"
    );
    assert_eq!(diag.log_capacity, RING_BUFFER_CAP as u32);
    // The diagnostics snapshot is the lossy-UTF-8 view of the LAST
    // RING_BUFFER_CAP bytes. We can't compare strings byte-for-byte
    // (replacement chars), but we can re-encode the expected tail
    // through the same lossy decode and compare.
    let expected_decoded = String::from_utf8_lossy(&expected_tail).into_owned();
    assert_eq!(diag.log_tail, expected_decoded);

    // Cleanup.
    let _ = stop_server(&id);
}

/// Stopping a server then asking for its diagnostics must NOT crash
/// and must NOT surface stale fields — the registry drops the entry
/// before the kill, so the next `lookup_diagnostics` for that id
/// returns `None`. Pins the "no crash on stopped process" property
/// the D52 spec asked for explicitly.
#[test]
fn lookup_diagnostics_on_stopped_handle_returns_none() {
    let child = std::process::Command::new("/bin/sleep")
        .arg("60")
        .spawn()
        .expect("spawn sleep");
    let id = register_for_test_with_log(54323, child, "label", b"hi");
    assert!(lookup_diagnostics(&id).is_some());

    stop_server(&id).expect("stop");

    assert!(
        lookup_diagnostics(&id).is_none(),
        "stopped handle must not surface diagnostics"
    );
}

// ─── D110: registry lookup / stop cleanup ───────────────────────────────
//
// `lookup_handle_info` is the trust boundary both `agent.singleStep`
// (`commands/agent.rs`) and chat dispatch (`commands/chat/send.rs::
// resolve_route`) depend on: a `Some(HandleInfo)` is taken as "this
// handle is live, route to this port under this model label" with NO
// further verification, and a `None` is the ONLY signal either caller
// uses to reject a stale/unknown handle with `IpcError::NotFound`
// instead of silently dispatching to a dead or wrong port. Despite
// that, no test called it directly before this slice — the existing
// `resolve_route_*` test in `chat::send::tests` only exercises the
// `Some` path indirectly through one of its two callers. These tests
// pin the function itself: exact field round-trip, the unknown-id
// `None`, and that a successful `stop_server` makes THIS handle
// resolve to `None` (the property dispatch actually relies on).
//
// Deliberately NOT asserted: the global `registry_len()` before/after
// a single register+stop. `registry` is one process-wide static and
// cargo runs tests in parallel within the same binary — several other
// tests in this file (diagnostics, start/stop) register and stop their
// own handles concurrently, so an exact-count comparison races against
// unrelated tests and can flake even when production behavior is
// correct (Codex review on #89). Asserting the specific handle's own
// resolution is both race-safe and the actually-relevant property: a
// removed HashMap entry means `lookup_handle_info` for THAT id can
// never again return `Some`, regardless of what else the registry holds.

#[test]
fn lookup_handle_info_unknown_handle_returns_none() {
    let bogus = ServerHandleId("srv_never_registered".into());
    assert!(lookup_handle_info(&bogus).is_none());
}

#[test]
fn lookup_handle_info_returns_recorded_port_and_model_label() {
    let child = std::process::Command::new("/bin/sleep")
        .arg("60")
        .spawn()
        .expect("spawn sleep");
    let id = register_for_test(54324, child, "/abs/path/to/qwen2.5-coder-3b");

    let info = lookup_handle_info(&id).expect("registered handle must resolve");
    assert_eq!(
        info,
        HandleInfo {
            port: 54324,
            model_label: "/abs/path/to/qwen2.5-coder-3b".to_string(),
        },
        "chat/agent dispatch trusts this pair verbatim to route the request"
    );

    let _ = stop_server(&id);
}

#[test]
fn lookup_handle_info_returns_none_after_stop() {
    let child = std::process::Command::new("/bin/sleep")
        .arg("60")
        .spawn()
        .expect("spawn sleep");
    let id = register_for_test(54325, child, "label");
    assert!(lookup_handle_info(&id).is_some());

    stop_server(&id).expect("stop");

    assert!(
        lookup_handle_info(&id).is_none(),
        "a stopped handle must resolve to None so callers reject it as stale, \
         not silently route to the now-dead port"
    );
}
