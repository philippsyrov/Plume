//! `/health` readiness probe for the D40 supervisor. Extracted from
//! `process.rs` (D119); `try_start_once` polls this after spawn and
//! before registering the handle. No registry state lives here.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::thread;
use std::time::{Duration, Instant};

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
