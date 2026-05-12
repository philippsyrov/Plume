//! Provider reachability probes.
//!
//! D1 uses a TCP connect against the well-known localhost port for
//! each connected runtime. That is an honest "is something listening"
//! signal without committing to a runtime-specific HTTP shape.
//!
//! D2 / D4 layer adapter-specific HTTP probes on top: when the TCP
//! handshake succeeds and the provider id is one we know how to ask,
//! we fetch its model list and attach it to the health snapshot.
//! Today that is Ollama (`/api/tags`), LM Studio (`/v1/models`), and
//! llama.cpp (`/v1/models`). Each adapter owns its own probe; this
//! file only dispatches.
//!
//! MLX-LM stays `NotConfigured` today — process supervision and
//! lockfiles haven't landed. Not an error state; the picker UI shows
//! it as "not configured" rather than "offline" so the user knows
//! the difference between "we asked and got silence" and "we don't
//! know how to ask".
//!
//! llama.cpp was `NotConfigured` through D3. D4 starts TCP-probing
//! the documented `llama-server` default port (8080) in case the
//! user is already running it. Its registry category stays
//! `PlumeManaged` because Plume's eventual plan is still to
//! supervise the process itself.

use std::io;
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use super::{ollama, openai_compat, ProviderHealth, ProviderModel, ReachabilityState};

/// How long a single TCP connect waits before we call the daemon
/// offline. Kept tight so a freshly-opened project doesn't sit on
/// the spinner — a healthy localhost daemon answers in single-digit
/// milliseconds.
const PROBE_TIMEOUT: Duration = Duration::from_millis(250);

/// HTTP probes get a slightly looser budget than TCP. A localhost
/// daemon serving JSON should respond well under this; we mostly
/// want the extra room to absorb the request/response round-trip
/// cost on a slow machine without flapping `models: None`.
const HTTP_PROBE_TIMEOUT: Duration = Duration::from_millis(750);

/// Runtimes we know how to TCP-probe. Keep this in sync with the
/// registry — entries here must match an `id` returned by
/// `super::registry::default_providers`. Categorization is separate:
/// `llama-cpp` is `PlumeManaged` in the registry but we still probe
/// `8080` so a user running `llama-server` themselves shows up.
///
/// Default ports come from each runtime's docs:
///   * Ollama       — `OLLAMA_HOST` defaults to `127.0.0.1:11434`.
///   * LM Studio    — local server defaults to `127.0.0.1:1234`.
///   * llama-server — `--host 127.0.0.1 --port 8080` defaults.
const TCP_PROBE_TARGETS: &[(&str, &str, u16)] = &[
    ("ollama", "127.0.0.1", 11434),
    ("lm-studio", "127.0.0.1", 1234),
    ("llama-cpp", "127.0.0.1", 8080),
];

/// Plume-managed providers we report as NotConfigured for now.
/// These will report real reachability once supervision lands.
const NOT_CONFIGURED_IDS: &[&str] = &["mlx-lm"];

/// Run every probe in parallel and merge with the not-configured
/// list. Each probe runs on the Tauri blocking pool so the async
/// runtime is not held by `TcpStream::connect_timeout`.
pub async fn probe_all() -> Vec<ProviderHealth> {
    let now = unix_ms();

    let mut handles = Vec::with_capacity(TCP_PROBE_TARGETS.len());
    for (id, host, port) in TCP_PROBE_TARGETS {
        let id = (*id).to_string();
        let host = (*host).to_string();
        let port = *port;
        handles.push(tauri::async_runtime::spawn_blocking(move || {
            probe_one(&id, &host, port, now)
        }));
    }

    let mut out = Vec::with_capacity(handles.len() + NOT_CONFIGURED_IDS.len());
    for handle in handles {
        match handle.await {
            Ok(snapshot) => out.push(snapshot),
            Err(join_err) => {
                // A panic in the blocking pool must not poison the
                // whole probe — log and move on. The frontend treats
                // the absence of an entry the same way it treats an
                // explicit offline state.
                tracing::warn!(error = %join_err, "provider probe panicked");
            }
        }
    }

    for id in NOT_CONFIGURED_IDS {
        out.push(ProviderHealth {
            id: (*id).to_string(),
            state: ReachabilityState::NotConfigured,
            latency_ms: None,
            probed_at_ms: now,
            models: None,
        });
    }

    out
}

/// Pure helper: connect once with a tight timeout, return latency on
/// success. Used by `probe_all` and exercised directly in tests.
pub fn probe_tcp(host: &str, port: u16, timeout: Duration) -> io::Result<u32> {
    let addr: SocketAddr = (host, port)
        .to_socket_addrs()?
        .next()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "no addresses resolved"))?;
    let started = Instant::now();
    TcpStream::connect_timeout(&addr, timeout)?;
    Ok(started.elapsed().as_millis().min(u128::from(u32::MAX)) as u32)
}

fn probe_one(id: &str, host: &str, port: u16, now_ms: u64) -> ProviderHealth {
    match probe_tcp(host, port, PROBE_TIMEOUT) {
        Ok(latency_ms) => {
            // TCP says the daemon is up. Now ask the adapter for its
            // model list. Failure here is not an "offline" signal —
            // the daemon answered, we just couldn't read the list —
            // so we keep `state: Available` and leave `models: None`.
            let models = adapter_models(id, host, port);
            ProviderHealth {
                id: id.to_string(),
                state: ReachabilityState::Available,
                latency_ms: Some(latency_ms),
                probed_at_ms: now_ms,
                models,
            }
        }
        Err(_) => ProviderHealth {
            id: id.to_string(),
            state: ReachabilityState::Offline,
            latency_ms: None,
            probed_at_ms: now_ms,
            models: None,
        },
    }
}

/// Dispatch to the adapter that owns this provider id. Returning
/// `None` means "we did not produce a list" and is the default for
/// every adapter we have not wired up. The empty vector means "we
/// asked and the daemon reports zero models" — UI must distinguish.
///
/// All three connected runtimes share the same failure semantics:
/// HTTP probe error keeps `state: Available` (the daemon answered
/// TCP) but leaves `models: None` (we couldn't read the list).
fn adapter_models(id: &str, host: &str, port: u16) -> Option<Vec<ProviderModel>> {
    let probe_result = match id {
        "ollama" => ollama::probe_models(host, port, HTTP_PROBE_TIMEOUT),
        // LM Studio and llama.cpp both serve OpenAI-style /v1/models
        // and share the same parser. They differ in port (1234 vs
        // 8080), which the TCP_PROBE_TARGETS table already encodes.
        "lm-studio" | "llama-cpp" => openai_compat::probe_models(host, port, HTTP_PROBE_TIMEOUT),
        _ => return None,
    };
    match probe_result {
        Ok(models) => Some(models),
        Err(err) => {
            tracing::debug!(provider = %id, error = %err, "model-list probe failed");
            None
        }
    }
}

fn unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    #[test]
    fn probe_tcp_succeeds_for_listening_port() {
        // Bind to an ephemeral port and immediately probe it.
        // OS-assigned ports avoid races with whatever else is
        // running on this machine — important because Ollama
        // (11434) and LM Studio (1234) may actually be live in
        // dev. Listener stays in scope so the probe finds it.
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral");
        let port = listener.local_addr().expect("local_addr").port();

        let latency = probe_tcp("127.0.0.1", port, Duration::from_millis(500))
            .expect("listening port should respond");
        // u32 is unsigned; assertion is presence + finite. Latency
        // can be 0 ms on a localhost connect.
        assert!(
            latency < 500,
            "latency {} ms exceeded probe budget",
            latency
        );
    }

    #[test]
    fn probe_tcp_fails_for_unbound_port() {
        // Bind, capture the port, then drop the listener so the port
        // is free again. There's a brief window where the OS may
        // hold the port in TIME_WAIT, but TcpStream::connect_timeout
        // returns ConnectionRefused (or similar) for a port with
        // nothing listening.
        let port = {
            let listener = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral");
            listener.local_addr().expect("local_addr").port()
        };

        let result = probe_tcp("127.0.0.1", port, Duration::from_millis(250));
        assert!(
            result.is_err(),
            "probe_tcp succeeded against an unbound port {port}: {:?}",
            result,
        );
    }

    #[test]
    fn adapter_models_returns_none_for_unknown_id() {
        // Dispatcher safety: an id we have no adapter for must yield
        // `None`, never panic. MLX-LM and any unknown id hit this
        // path. We deliberately do NOT exercise lm-studio / llama-cpp
        // here because both have adapters wired up now and a real
        // daemon on this machine would race the assertion.
        assert!(adapter_models("mlx-lm", "127.0.0.1", 0).is_none());
        assert!(adapter_models("nonsense", "127.0.0.1", 0).is_none());
    }
}
