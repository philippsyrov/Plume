//! Shared probe for OpenAI-style `/v1/models` endpoints.
//!
//! Both LM Studio (port 1234) and llama.cpp's `llama-server` (port
//! 8080) advertise OpenAI compatibility and serve `/v1/models` at
//! `{ "object": "list", "data": [{ "id": ..., "object": "model",
//! ... }] }`. The fields each runtime adds beyond the OpenAI core
//! differ — llama.cpp also returns `aliases`, `tags`, `status`,
//! `architecture`; LM Studio sticks closer to the OpenAI minimum —
//! but every release of both servers ships `data[].id`, which is
//! the only field D4 needs to populate `ProviderHealth.models`.
//!
//! Shape verified against:
//!   * llama.cpp `tools/server/server-models.cpp` `get_router_models`
//!     (line ~1252) — keys: `id`, `aliases`, `tags`, `object`,
//!     `owned_by`, `created`, `status`, `architecture`. Wraps in
//!     `{ "data": [...], "object": "list" }`.
//!   * LM Studio "OpenAI Compatibility" docs at
//!     <https://lmstudio.ai/docs/developer/openai-compat/models>:
//!     advertises OpenAI parity. Core `id` field is the stable
//!     surface.
//!
//! Per-model size is NOT reported by either runtime, so the probe
//! sets `size_bytes: None` for every entry. Per-model details
//! (parameter count, quantization, context length) are also absent
//! from `/v1/models`; the `providers.modelDetails` verb stays
//! Ollama-only for now.

use std::io;
use std::time::Duration;

use serde::Deserialize;

use super::http::http_request;
use super::ProviderModel;

const MODELS_PATH: &str = "/v1/models";

/// Probe an OpenAI-compatible `/v1/models` endpoint at the given
/// `host:port` and return the installed models. Caller contract
/// mirrors `ollama::probe_models`: timeout applies per-syscall,
/// errors are `io::Error` so callers can fold them into the
/// existing offline path.
pub fn probe_models(host: &str, port: u16, timeout: Duration) -> io::Result<Vec<ProviderModel>> {
    let body = http_request(host, port, "GET", MODELS_PATH, None, timeout)?;
    parse_models(&body).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))
}

/// Minimal projection of OpenAI's list-models envelope. We tolerate
/// any extra top-level keys.
#[derive(Debug, Deserialize)]
struct ModelsResponse {
    data: Vec<ModelEntry>,
}

/// One entry in `data[]`. `id` is the only field we surface; every
/// other key (`object`, `created`, `owned_by`, llama.cpp's
/// `aliases`, `tags`, `status`, `architecture`, …) is ignored on
/// purpose so an upstream schema bump cannot drift our types.
#[derive(Debug, Deserialize)]
struct ModelEntry {
    id: String,
}

fn parse_models(body: &str) -> Result<Vec<ProviderModel>, serde_json::Error> {
    let resp: ModelsResponse = serde_json::from_str(body)?;
    Ok(resp
        .data
        .into_iter()
        .map(|m| ProviderModel {
            id: m.id,
            // Neither LM Studio nor llama.cpp report a per-model byte
            // count in `/v1/models`. Keeping `None` is honest; the
            // panel renders count + names and skips the size badge.
            size_bytes: None,
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::super::http::{drain_request, read_full_request};
    use super::*;
    use std::io::Write as _;
    use std::net::TcpListener;
    use std::thread;

    /// Real `/v1/models` body shape from llama.cpp's
    /// `server-models.cpp` `get_router_models`. Includes the
    /// llama.cpp-specific fields (aliases, tags, status,
    /// architecture) that our parser must tolerate.
    const LLAMA_CPP_FIXTURE: &str = r#"{
        "object": "list",
        "data": [
            {
                "id": "Qwen2.5-Coder-7B-Instruct.gguf",
                "aliases": ["qwen-coder"],
                "tags": ["code"],
                "object": "model",
                "owned_by": "llamacpp",
                "created": 1731000000,
                "status": { "value": "running", "args": "" },
                "architecture": {
                    "input_modalities": ["text"],
                    "output_modalities": ["text"]
                }
            }
        ]
    }"#;

    /// Trimmed `/v1/models` body matching LM Studio's documented
    /// OpenAI parity. Two entries, only the OpenAI-standard fields.
    const LM_STUDIO_FIXTURE: &str = r#"{
        "object": "list",
        "data": [
            { "id": "lmstudio-community/Gemma-2B-Instruct-GGUF", "object": "model", "created": 1731000000, "owned_by": "lmstudio" },
            { "id": "TheBloke/Llama-3-8B-Instruct-GGUF",        "object": "model", "created": 1731000000, "owned_by": "lmstudio" }
        ]
    }"#;

    #[test]
    fn parses_llama_cpp_response() {
        let models = parse_models(LLAMA_CPP_FIXTURE).expect("parse");
        assert_eq!(
            models,
            vec![ProviderModel {
                id: "Qwen2.5-Coder-7B-Instruct.gguf".into(),
                // `/v1/models` never reports byte size; this is the
                // contract the UI keys off to skip the size badge.
                size_bytes: None,
            }]
        );
    }

    #[test]
    fn parses_lm_studio_response() {
        let models = parse_models(LM_STUDIO_FIXTURE).expect("parse");
        assert_eq!(models.len(), 2);
        assert_eq!(models[0].id, "lmstudio-community/Gemma-2B-Instruct-GGUF");
        assert_eq!(models[1].id, "TheBloke/Llama-3-8B-Instruct-GGUF");
        assert!(models.iter().all(|m| m.size_bytes.is_none()));
    }

    #[test]
    fn parses_empty_data_array() {
        // A daemon with no model loaded returns `{ "data": [] }`.
        // UI surfaces "no models installed", which is materially
        // different from "we did not probe".
        let body = r#"{ "object": "list", "data": [] }"#;
        let models = parse_models(body).expect("parse");
        assert!(models.is_empty());
    }

    #[test]
    fn rejects_response_missing_data() {
        // Anything that isn't `{ data: [...] }` fails fast. Falling
        // back to `models: null` is more honest than rendering an
        // empty list.
        let err = parse_models(r#"{ "object": "list" }"#).expect_err("should fail");
        assert!(err.to_string().contains("data"));
    }

    #[test]
    fn rejects_response_where_data_item_missing_id() {
        let body = r#"{ "data": [{ "object": "model" }] }"#;
        let err = parse_models(body).expect_err("should fail");
        assert!(err.to_string().to_lowercase().contains("id"));
    }

    #[test]
    fn probe_models_round_trip_against_stub() {
        // Wire the parser end-to-end. The stub captures the request
        // and asserts on the wire shape so this test would fail if
        // the client ever used the wrong method or path.
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let response_body = LM_STUDIO_FIXTURE;
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            response_body.len(),
            response_body,
        );

        let captured = std::sync::Arc::new(std::sync::Mutex::new(None::<Vec<u8>>));
        let captured_for_thread = captured.clone();
        thread::spawn(move || {
            if let Ok((mut sock, _)) = listener.accept() {
                let request = read_full_request(&mut sock);
                *captured_for_thread.lock().unwrap() = Some(request);
                let _ = sock.write_all(response.as_bytes());
            }
        });

        let models =
            probe_models("127.0.0.1", port, Duration::from_millis(500)).expect("probe_models");
        assert_eq!(models.len(), 2);

        let request_bytes = captured
            .lock()
            .unwrap()
            .take()
            .expect("stub never received a request");
        let request = std::str::from_utf8(&request_bytes).expect("utf-8 request");
        assert!(
            request.starts_with("GET /v1/models HTTP/1.1\r\n"),
            "expected GET /v1/models, got start: {:?}",
            request.lines().next()
        );
    }

    #[test]
    fn probe_models_surfaces_non_2xx_via_http_request() {
        // The status-code check lives in `http::http_request`; this
        // test just confirms an error from the underlying call
        // bubbles up as an `io::Error` instead of a misleading parse
        // failure.
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let response = "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
        thread::spawn(move || {
            if let Ok((mut sock, _)) = listener.accept() {
                drain_request(&mut sock);
                let _ = sock.write_all(response.as_bytes());
            }
        });

        let err = probe_models("127.0.0.1", port, Duration::from_millis(500)).expect_err("err");
        assert!(err.to_string().contains("404"));
    }
}
