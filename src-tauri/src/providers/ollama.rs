//! Ollama adapter probes.
//!
//! D2 ships the first adapter-specific HTTP probe: `GET /api/tags`
//! against the localhost Ollama daemon to list installed models. The
//! result feeds `ProviderHealth.models` so the provider panel can
//! report "N models" instead of just "available".
//!
//! Constraints kept:
//!   - No new crate deps. We already hand-roll the TCP probe with
//!     `std::net`; one localhost JSON GET does not justify pulling
//!     in `reqwest` / `ureq`. If a third adapter needs HTTP we swap
//!     this for a real client at that point.
//!   - Localhost only. No TLS, no auth, no proxies, no env-var
//!     overrides yet — D2 is "is the default daemon up and what does
//!     it have", nothing more.
//!   - Strict timeouts. Same envelope as the TCP probe so a stalled
//!     daemon never holds up the panel.
//!   - No model downloads, no `ollama serve` auto-start. We only
//!     read.
//!
//! This module owns: the request, the response framing, the JSON
//! parser, and a stub-driven test harness. `health.rs` calls
//! `probe_models` after a successful TCP connect.
//!
//! Failure modes:
//!   - TCP open / write / read errors → `io::Error` (kind preserved).
//!   - Non-200 status → `io::ErrorKind::Other` with the status line
//!     in the message.
//!   - Header / body framing surprise (no `\r\n\r\n`, non-utf8) →
//!     `io::ErrorKind::InvalidData`.
//!   - JSON shape doesn't match → also `InvalidData`.
//!
//! Caller in `health.rs` treats any error as "no model list this
//! time" and leaves `ProviderHealth.models` as `None`.

use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

use serde::Deserialize;

use super::ProviderModel;

const TAGS_PATH: &str = "/api/tags";

/// Probe Ollama's `/api/tags` endpoint and return the installed
/// models. Same caller contract as `health::probe_tcp`: timeout
/// applies per-syscall, errors are `io::Error` so the caller can
/// fold them into the existing offline path.
pub fn probe_models(host: &str, port: u16, timeout: Duration) -> io::Result<Vec<ProviderModel>> {
    let body = http_get(host, port, TAGS_PATH, timeout)?;
    parse_tags(&body).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))
}

// --- HTTP/1.1 GET, hand-rolled --------------------------------------
//
// Why this exists:
//   * The TCP probe in `health.rs` already uses `std::net::TcpStream`
//     directly. Adding a real HTTP client just for one localhost GET
//     pulls in tokio/hyper/native-tls, which is a lot of weight for
//     no extra value.
//   * The Ollama daemon serves with `Content-Length` set and honors
//     `Connection: close`. That means we can read until EOF and
//     trust everything after the first `\r\n\r\n` is the body —
//     no chunked-encoding decoder, no keep-alive bookkeeping.
//
// What this is NOT:
//   * A general-purpose HTTP client. `Transfer-Encoding: chunked`
//     responses, redirects, and TLS are all out of scope. If a
//     future adapter (LM Studio's OpenAI-compatible API, llama.cpp's
//     `llama-server`) needs richer behavior, replace this whole
//     section with `ureq` or similar and route every adapter through
//     it.

fn http_get(host: &str, port: u16, path: &str, timeout: Duration) -> io::Result<String> {
    let addr: SocketAddr = format!("{host}:{port}")
        .parse()
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, format!("bad addr: {e}")))?;

    let mut stream = TcpStream::connect_timeout(&addr, timeout)?;
    stream.set_read_timeout(Some(timeout))?;
    stream.set_write_timeout(Some(timeout))?;

    let req = format!(
        "GET {path} HTTP/1.1\r\n\
         Host: {host}:{port}\r\n\
         User-Agent: plume\r\n\
         Accept: application/json\r\n\
         Connection: close\r\n\
         \r\n"
    );
    stream.write_all(req.as_bytes())?;
    stream.flush()?;

    // `Connection: close` means the server closes after the body, so
    // read_to_end returns everything in one shot. The cap is a safety
    // ceiling against a runaway response — Ollama's tag list for a
    // realistic install is tens of KB; 4 MiB is far above that.
    let mut response = Vec::with_capacity(4096);
    (&stream).take(4 * 1024 * 1024).read_to_end(&mut response)?;

    let body_start = find_subsequence(&response, b"\r\n\r\n")
        .map(|i| i + 4)
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "missing CRLF CRLF header/body separator",
            )
        })?;

    let header_text = std::str::from_utf8(&response[..body_start])
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "non-utf8 header"))?;
    let status_line = header_text.lines().next().unwrap_or("").trim();
    if !is_status_2xx(status_line) {
        return Err(io::Error::other(format!("non-2xx response: {status_line}")));
    }

    String::from_utf8(response[body_start..].to_vec())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "non-utf8 body"))
}

fn is_status_2xx(line: &str) -> bool {
    // Accepts "HTTP/1.1 200 OK", "HTTP/1.0 200 OK", "HTTP/1.1 204
    // No Content", and similar. We do not consume bodies of >=300.
    let mut parts = line.split_whitespace();
    let _version = parts.next();
    let code = parts.next().unwrap_or("");
    matches!(code.as_bytes(), [b'2', _, _])
}

fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    haystack.windows(needle.len()).position(|w| w == needle)
}

// --- /api/tags JSON shape -------------------------------------------

#[derive(Debug, Deserialize)]
struct TagsResponse {
    models: Vec<TagEntry>,
}

#[derive(Debug, Deserialize)]
struct TagEntry {
    /// Ollama's display name + tag (e.g. `gemma:7b`). The `model`
    /// field exists too in newer versions and tends to be identical;
    /// we read `name` since it is the historical, more stable key.
    name: String,
    /// On-disk size in bytes. Ollama reports this for every entry,
    /// but tolerate omission so a future server change does not break
    /// the panel.
    #[serde(default)]
    size: Option<u64>,
}

fn parse_tags(body: &str) -> Result<Vec<ProviderModel>, serde_json::Error> {
    let resp: TagsResponse = serde_json::from_str(body)?;
    Ok(resp
        .models
        .into_iter()
        .map(|t| ProviderModel {
            id: t.name,
            size_bytes: t.size,
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    // `Read` and `Write` come in through `super::*` (the parent module
    // imports them from `std::io`). Re-importing them here would
    // shadow with the same path and trigger an unused-import warning.
    use std::net::TcpListener;
    use std::thread;

    /// Drain the client's request headers before we hand back a
    /// response. Closing the socket while the receive buffer still
    /// has unread bytes causes the kernel to send RST instead of
    /// FIN, which surfaces on the client as `ConnectionReset` and
    /// hides whatever response we just wrote. Reading until the
    /// header terminator (`\r\n\r\n`) drains enough that closing
    /// is clean.
    fn drain_request(sock: &mut std::net::TcpStream) {
        let mut buf = [0u8; 1024];
        loop {
            match sock.read(&mut buf) {
                Ok(0) => return,
                Ok(_n) => {
                    // Quick-and-dirty: peek into the running buffer
                    // and stop once we have seen the header
                    // terminator. Real servers parse this; tests
                    // are happy with substring search.
                    if buf.windows(4).any(|w| w == b"\r\n\r\n") {
                        return;
                    }
                }
                Err(_) => return,
            }
        }
    }

    #[test]
    fn parses_realistic_tags_response() {
        // Trimmed down sample of a real Ollama /api/tags body.
        // Includes the size field plus a `details` block we ignore on
        // purpose — the adapter must tolerate extra keys.
        let body = r#"{
            "models": [
                {
                    "name": "gemma:7b",
                    "model": "gemma:7b",
                    "modified_at": "2024-04-19T10:00:00Z",
                    "size": 5011853764,
                    "digest": "abc123",
                    "details": { "family": "gemma", "parameter_size": "7B" }
                },
                {
                    "name": "qwen2.5-coder:14b-q4",
                    "size": 9020000000
                }
            ]
        }"#;
        let models = parse_tags(body).expect("parse");
        assert_eq!(
            models,
            vec![
                ProviderModel {
                    id: "gemma:7b".into(),
                    size_bytes: Some(5_011_853_764),
                },
                ProviderModel {
                    id: "qwen2.5-coder:14b-q4".into(),
                    size_bytes: Some(9_020_000_000),
                },
            ]
        );
    }

    #[test]
    fn parses_empty_models_list() {
        // A daemon that's been started but never had a model pulled
        // returns the field as an empty array. UI surfaces "0 models",
        // which is materially different from "we did not probe".
        let body = r#"{"models": []}"#;
        let models = parse_tags(body).expect("parse");
        assert!(models.is_empty());
    }

    #[test]
    fn rejects_wrong_shape() {
        // Anything that isn't `{ models: [...] }` fails fast. We
        // would rather show "no list" than silently render garbage.
        let err = parse_tags(r#"{"foo": []}"#).expect_err("should fail");
        assert!(err.to_string().contains("models"));
    }

    #[test]
    fn http_get_returns_body_for_canned_response() {
        // Bind an ephemeral port and pretend to be Ollama for one
        // request. The stub ensures probe_models works end-to-end
        // without needing a real daemon — useful in CI and on dev
        // machines where Ollama may not be installed.
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let canned = "{\"models\":[{\"name\":\"phi:latest\",\"size\":1000}]}";
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n{}",
            canned.len(),
            canned,
        );
        thread::spawn(move || {
            if let Ok((mut sock, _)) = listener.accept() {
                drain_request(&mut sock);
                let _ = sock.write_all(response.as_bytes());
            }
        });

        let body =
            http_get("127.0.0.1", port, TAGS_PATH, Duration::from_millis(500)).expect("http_get");
        assert_eq!(body, canned);
    }

    #[test]
    fn probe_models_round_trip_against_stub() {
        // Wire the parser end-to-end: stub server returns a real-
        // looking tags blob, probe_models normalizes it.
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let canned = r#"{"models":[{"name":"gemma:2b","size":1500000000}]}"#;
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            canned.len(),
            canned,
        );
        thread::spawn(move || {
            if let Ok((mut sock, _)) = listener.accept() {
                drain_request(&mut sock);
                let _ = sock.write_all(response.as_bytes());
            }
        });

        let models =
            probe_models("127.0.0.1", port, Duration::from_millis(500)).expect("probe_models");
        assert_eq!(
            models,
            vec![ProviderModel {
                id: "gemma:2b".into(),
                size_bytes: Some(1_500_000_000),
            }]
        );
    }

    #[test]
    fn http_get_surfaces_non_2xx_status() {
        // 404 should produce an error rather than be silently parsed
        // as JSON — Ollama can return `{"error":"..."}` on 404 and we
        // would otherwise try to deserialize that as TagsResponse.
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let response = "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
        thread::spawn(move || {
            if let Ok((mut sock, _)) = listener.accept() {
                drain_request(&mut sock);
                let _ = sock.write_all(response.as_bytes());
            }
        });

        let err =
            http_get("127.0.0.1", port, TAGS_PATH, Duration::from_millis(500)).expect_err("err");
        assert!(err.to_string().contains("404"));
    }
}
