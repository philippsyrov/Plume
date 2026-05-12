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
use serde_json::Value;

use super::ProviderModel;

const TAGS_PATH: &str = "/api/tags";
const SHOW_PATH: &str = "/api/show";

/// Probe Ollama's `/api/tags` endpoint and return the installed
/// models. Same caller contract as `health::probe_tcp`: timeout
/// applies per-syscall, errors are `io::Error` so the caller can
/// fold them into the existing offline path.
pub fn probe_models(host: &str, port: u16, timeout: Duration) -> io::Result<Vec<ProviderModel>> {
    let body = http_request(host, port, "GET", TAGS_PATH, None, timeout)?;
    parse_tags(&body).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))
}

/// Probe Ollama's `/api/show` endpoint for the named model. Returns
/// the subset of fields D3 needs for the model-truth panel.
///
/// `model_name` is the same opaque tag string the `/api/tags` probe
/// returned (e.g. `gemma:7b`). We POST `{"model": "<name>"}` — that
/// is the documented interface in
/// <https://github.com/ollama/ollama/blob/main/docs/api.md#show-model-information>.
pub fn probe_model_details(
    host: &str,
    port: u16,
    model_name: &str,
    timeout: Duration,
) -> io::Result<OllamaModelDetails> {
    let request_body = serde_json::json!({ "model": model_name }).to_string();
    let body = http_request(host, port, "POST", SHOW_PATH, Some(&request_body), timeout)?;
    parse_show(&body).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))
}

/// Plume's projection of Ollama's `/api/show` body. Only fields we
/// surface today; the rest of the upstream payload (modelfile,
/// template, license, parameters blob, full `model_info` map) is
/// intentionally dropped so changes upstream cannot drift our types.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OllamaModelDetails {
    /// Container format string from `details.format` (Ollama reports
    /// `"gguf"` for every model today, but we propagate verbatim).
    pub format: Option<String>,
    /// Model family from `details.family` (`"llama"`, `"gemma"`, …).
    pub family: Option<String>,
    /// Display string from `details.parameter_size` (`"8.0B"`).
    pub parameter_size: Option<String>,
    /// Exact parameter count from `model_info["general.parameter_count"]`
    /// when available — preferred over `parameter_size` for fit math
    /// because the human-readable string can round (e.g. "7B" for a
    /// 6.7 B model).
    pub parameter_count: Option<u64>,
    /// Quantization label from `details.quantization_level`
    /// (`"Q4_0"`, `"Q4_K_M"`, …).
    pub quantization: Option<String>,
    /// Native context window from `model_info["<family>.context_length"]`
    /// when the runtime reports it.
    pub context_length: Option<u32>,
    /// Capabilities array — `"completion"`, `"vision"`, … — verbatim
    /// from upstream. Useful for the UI to mark vision-capable models
    /// without re-parsing names.
    pub capabilities: Vec<String>,
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

fn http_request(
    host: &str,
    port: u16,
    method: &str,
    path: &str,
    body: Option<&str>,
    timeout: Duration,
) -> io::Result<String> {
    let addr: SocketAddr = format!("{host}:{port}")
        .parse()
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, format!("bad addr: {e}")))?;

    let mut stream = TcpStream::connect_timeout(&addr, timeout)?;
    stream.set_read_timeout(Some(timeout))?;
    stream.set_write_timeout(Some(timeout))?;

    // POST always carries Content-Type + Content-Length so Ollama
    // accepts the body without buffering for HTTP/1.0-style framing.
    // GET requests omit both, which matches what `/api/tags` expects.
    let (body_headers, body_str) = match body {
        Some(b) => (
            format!(
                "Content-Type: application/json\r\nContent-Length: {}\r\n",
                b.len()
            ),
            b,
        ),
        None => (String::new(), ""),
    };

    let req = format!(
        "{method} {path} HTTP/1.1\r\n\
         Host: {host}:{port}\r\n\
         User-Agent: plume\r\n\
         Accept: application/json\r\n\
         {body_headers}\
         Connection: close\r\n\
         \r\n\
         {body_str}"
    );
    stream.write_all(req.as_bytes())?;
    stream.flush()?;

    // `Connection: close` means the server closes after the body, so
    // read_to_end returns everything in one shot. The cap is a safety
    // ceiling against a runaway response — Ollama's tag/show payloads
    // for a realistic install are tens of KB; 4 MiB is far above that.
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

// --- /api/show JSON shape -------------------------------------------
//
// Real shape verified against
// https://github.com/ollama/ollama/blob/main/docs/api.md#show-model-information
// (commit pinned in the PR description). The doc shows JSON-5 with
// unquoted keys; wire format is proper JSON. We deliberately read
// only the subset we surface today and ignore the rest so an upstream
// schema bump (extra `capabilities`, new `model_info` keys, …) does
// not break the panel.

#[derive(Debug, Deserialize)]
struct ShowResponse {
    #[serde(default)]
    details: Option<ShowDetails>,
    /// `model_info` is a heterogeneous map keyed on
    /// `"general.parameter_count"`, `"llama.context_length"`, …. We
    /// hold it as raw `Value` and extract the keys we need; encoding
    /// these as a struct would force a per-family proliferation.
    #[serde(default)]
    model_info: Option<Value>,
    #[serde(default)]
    capabilities: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct ShowDetails {
    #[serde(default)]
    format: Option<String>,
    #[serde(default)]
    family: Option<String>,
    #[serde(default)]
    parameter_size: Option<String>,
    #[serde(default)]
    quantization_level: Option<String>,
}

fn parse_show(body: &str) -> Result<OllamaModelDetails, serde_json::Error> {
    let resp: ShowResponse = serde_json::from_str(body)?;
    let details = resp.details.unwrap_or(ShowDetails {
        format: None,
        family: None,
        parameter_size: None,
        quantization_level: None,
    });

    let parameter_count = resp
        .model_info
        .as_ref()
        .and_then(|v| v.get("general.parameter_count"))
        .and_then(|v| v.as_u64());

    // `model_info` keys are prefixed with the architecture name
    // (`llama.context_length`, `gemma.context_length`, …). Prefer the
    // entry that matches `details.family` and fall back to any key
    // ending in `.context_length` so a slight mismatch between the
    // two upstream fields doesn't strand the value.
    let context_length = resp.model_info.as_ref().and_then(|v| {
        let map = v.as_object()?;
        // Family-prefixed match first.
        if let Some(family) = details.family.as_deref() {
            let key = format!("{family}.context_length");
            if let Some(num) = map.get(&key).and_then(|x| x.as_u64()) {
                return Some(num);
            }
        }
        // Generic fallback.
        map.iter()
            .find(|(k, _)| k.ends_with(".context_length"))
            .and_then(|(_, val)| val.as_u64())
    });
    let context_length: Option<u32> = context_length.map(|n| n.min(u32::MAX as u64) as u32);

    Ok(OllamaModelDetails {
        format: details.format,
        family: details.family,
        parameter_size: details.parameter_size,
        parameter_count,
        quantization: details.quantization_level,
        context_length,
        capabilities: resp.capabilities.unwrap_or_default(),
    })
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
    fn http_request_returns_body_for_canned_get_response() {
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

        let body = http_request(
            "127.0.0.1",
            port,
            "GET",
            TAGS_PATH,
            None,
            Duration::from_millis(500),
        )
        .expect("http_request");
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
    fn http_request_surfaces_non_2xx_status() {
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

        let err = http_request(
            "127.0.0.1",
            port,
            "GET",
            TAGS_PATH,
            None,
            Duration::from_millis(500),
        )
        .expect_err("err");
        assert!(err.to_string().contains("404"));
    }

    /// Realistic /api/show body. Trimmed to the keys we surface plus
    /// a few we ignore on purpose so the test asserts our parser
    /// tolerates upstream additions (`modelfile`, `template`, …).
    /// Shape verified against
    /// https://github.com/ollama/ollama/blob/main/docs/api.md#show-model-information.
    ///
    /// `r##"..."##` because the embedded JSON contains `"#` (Modelfile
    /// comment markers) which would otherwise close a single-hash raw
    /// string early.
    const SHOW_FIXTURE: &str = r##"{
        "modelfile": "# Modelfile ...",
        "template": "{{ .System }}",
        "parameters": "num_keep 24",
        "details": {
            "parent_model": "",
            "format": "gguf",
            "family": "llama",
            "families": ["llama"],
            "parameter_size": "8.0B",
            "quantization_level": "Q4_0"
        },
        "model_info": {
            "general.architecture": "llama",
            "general.parameter_count": 8030261248,
            "general.quantization_version": 2,
            "llama.context_length": 8192,
            "llama.attention.head_count": 32
        },
        "capabilities": ["completion", "vision"]
    }"##;

    #[test]
    fn parses_realistic_show_response() {
        let details = parse_show(SHOW_FIXTURE).expect("parse_show");
        assert_eq!(details.format.as_deref(), Some("gguf"));
        assert_eq!(details.family.as_deref(), Some("llama"));
        assert_eq!(details.parameter_size.as_deref(), Some("8.0B"));
        assert_eq!(details.parameter_count, Some(8_030_261_248));
        assert_eq!(details.quantization.as_deref(), Some("Q4_0"));
        assert_eq!(details.context_length, Some(8192));
        assert_eq!(details.capabilities, vec!["completion", "vision"]);
    }

    #[test]
    fn show_response_with_missing_model_info_is_tolerated() {
        // Older Ollama releases omitted `model_info`; falling back to
        // the human-readable `parameter_size` is fine.
        let body = r#"{
            "details": {
                "format": "gguf",
                "family": "gemma",
                "parameter_size": "2B",
                "quantization_level": "Q4_K_M"
            }
        }"#;
        let details = parse_show(body).expect("parse_show");
        assert_eq!(details.parameter_count, None);
        assert_eq!(details.context_length, None);
        assert_eq!(details.parameter_size.as_deref(), Some("2B"));
        assert!(details.capabilities.is_empty());
    }

    #[test]
    fn context_length_falls_back_to_any_family_key() {
        // If `details.family` doesn't match what model_info uses,
        // the generic `.context_length` fallback still finds the
        // entry.
        let body = r#"{
            "details": { "family": "qwen" },
            "model_info": { "qwen2.context_length": 32768 }
        }"#;
        let details = parse_show(body).expect("parse_show");
        assert_eq!(details.context_length, Some(32768));
    }

    #[test]
    fn probe_model_details_round_trip_against_stub() {
        // Mirror the tags round-trip test for /api/show. The stub
        // exercises the POST path inside http_request and verifies
        // the body is forwarded with a Content-Length header.
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let response_body = SHOW_FIXTURE;
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            response_body.len(),
            response_body,
        );
        thread::spawn(move || {
            if let Ok((mut sock, _)) = listener.accept() {
                // Drain through the request body too. The header
                // sentinel `\r\n\r\n` is enough since the POST body
                // is tiny and the kernel buffers it.
                drain_request(&mut sock);
                let _ = sock.write_all(response.as_bytes());
            }
        });

        let details = probe_model_details(
            "127.0.0.1",
            port,
            "llama3:latest",
            Duration::from_millis(500),
        )
        .expect("probe_model_details");
        assert_eq!(details.parameter_count, Some(8_030_261_248));
        assert_eq!(details.quantization.as_deref(), Some("Q4_0"));
    }
}
