//! Non-streaming Ollama `/api/chat` adapter. Retained
//! `#[cfg(test)]`-only since D7.1 as a reference implementation of
//! the single-shot protocol; the shipping IPC path goes through
//! `stream_chat`. All blocking-only support code (request builder,
//! response struct, error parser) lives here.

#![cfg(test)]

use std::time::Duration;

use serde::Deserialize;

use super::super::{ChatFinish, ChatMessage, ChatResponse, ChatRole};
use super::http::{extract_error_message, role_str, CHAT_PATH};
use super::ChatError;
use crate::providers::http::http_request_with_status;

/// Send a non-streaming chat completion to a localhost Ollama daemon
/// and parse the assistant message out of the response.
///
/// `model` is the Ollama tag string (`llama3:latest`,
/// `qwen2.5-coder:14b-q4`, …). `messages` is the full transcript;
/// Ollama is stateless across `/api/chat` calls, so the caller
/// concatenates history themselves.
///
/// `#[cfg(test)]`-gated since D7.1: the shipping IPC path goes
/// through `stream_chat`. This function and its support code are
/// kept as a reference implementation of the non-streaming protocol
/// and as the basis for several parser / error-mapping tests in
/// this module.
pub fn send_chat(
    host: &str,
    port: u16,
    model: &str,
    messages: &[ChatMessage],
    timeout: Duration,
) -> Result<ChatResponse, ChatError> {
    let request_body = build_request_body(model, messages);

    let (status, body) =
        http_request_with_status(host, port, "POST", CHAT_PATH, Some(&request_body), timeout)
            .map_err(|source| ChatError::Transport {
                host: host.to_string(),
                port,
                source,
            })?;

    if status == 404 {
        let message = extract_error_message(&body).unwrap_or_else(|| body.clone());
        return Err(ChatError::ModelNotFound {
            model: model.to_string(),
            message,
        });
    }
    if !(200..300).contains(&status) {
        let message = extract_error_message(&body).unwrap_or_else(|| body.clone());
        return Err(ChatError::BadStatus { status, message });
    }

    let parsed: OllamaChatResponse =
        serde_json::from_str(&body).map_err(|e| ChatError::Parse(e.to_string()))?;

    let finish = if parsed.done {
        ChatFinish::Stop
    } else {
        ChatFinish::Length
    };

    Ok(ChatResponse {
        message: ChatMessage {
            role: ChatRole::Assistant,
            content: parsed.message.content,
        },
        provider_id: "ollama".to_string(),
        model_id: parsed.model,
        // Caller fills duration_ms; the adapter doesn't time itself.
        duration_ms: 0,
        finish,
    })
}

/// Build the JSON request body. Kept out of `send_chat` so a test can
/// assert on the exact bytes without spinning up a TCP listener.
/// Test-only since D7.1 — the shipping path uses
/// `build_request_body_streaming`.
fn build_request_body(model: &str, messages: &[ChatMessage]) -> String {
    // Hand-build the JSON instead of using a derived `Serialize`
    // struct — that way the wire shape is auditable next to the
    // upstream docs in this file's header comment without a layer
    // of indirection.
    let messages_json = serde_json::Value::Array(
        messages
            .iter()
            .map(|m| {
                serde_json::json!({
                    "role": role_str(m.role),
                    "content": m.content,
                })
            })
            .collect(),
    );
    serde_json::json!({
        "model": model,
        "messages": messages_json,
        "stream": false,
    })
    .to_string()
}

// --- non-streaming response parsing (test-only since D7.1) ----------

#[derive(Debug, Deserialize)]
struct OllamaChatResponse {
    /// Echoed model id; can differ from the request (`llama3` →
    /// `llama3:latest`) so we propagate it verbatim.
    model: String,
    message: OllamaMessage,
    done: bool,
    // The rest of the wire payload (`created_at`, `total_duration`,
    // `eval_count`, etc.) is intentionally dropped. Adding fields
    // here later is additive; today's surface keeps no telemetry.
}

#[derive(Debug, Deserialize)]
struct OllamaMessage {
    // We trust Ollama to return role:"assistant" here. Plume always
    // overrides with ChatRole::Assistant when building the public
    // `ChatResponse`, so even if a future daemon returns "tool" or
    // "system" the surface remains honest.
    #[allow(dead_code)]
    role: String,
    content: String,
}

#[cfg(test)]
mod tests {
    use super::super::super::{ChatFinish, ChatMessage, ChatRole};
    use super::super::http::{extract_error_message, role_str};
    use super::super::ChatError;
    use super::{build_request_body, send_chat};
    use crate::providers::http::read_full_request;
    use std::io::Write;
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};
    use std::thread;
    use std::time::Duration;

    /// Realistic /api/chat body trimmed to the surface Plume reads,
    /// plus extra keys (`created_at`, `total_duration`, …) we ignore
    /// on purpose to prove the parser tolerates upstream additions.
    const CHAT_FIXTURE: &str = r#"{
        "model": "llama3:latest",
        "created_at": "2024-04-19T10:00:00.123Z",
        "message": {
            "role": "assistant",
            "content": "Hello! How can I help you today?"
        },
        "done": true,
        "done_reason": "stop",
        "total_duration": 5191566416,
        "load_duration": 2154458,
        "prompt_eval_count": 26,
        "prompt_eval_duration": 383809000,
        "eval_count": 298,
        "eval_duration": 4799921000
    }"#;

    #[test]
    fn builds_request_body_with_stream_false() {
        // The wire shape is load-bearing: if `stream` flips back to
        // its default (true) the adapter would have to parse NDJSON
        // and D7 explicitly does not.
        let msgs = vec![
            ChatMessage {
                role: ChatRole::User,
                content: "hi".into(),
            },
            ChatMessage {
                role: ChatRole::Assistant,
                content: "hi back".into(),
            },
            ChatMessage {
                role: ChatRole::User,
                content: "more".into(),
            },
        ];
        let body = build_request_body("llama3:latest", &msgs);
        let parsed: serde_json::Value = serde_json::from_str(&body).expect("valid json");
        assert_eq!(parsed["model"], "llama3:latest");
        assert_eq!(parsed["stream"], false);
        let arr = parsed["messages"].as_array().expect("messages array");
        assert_eq!(arr.len(), 3);
        assert_eq!(arr[0]["role"], "user");
        assert_eq!(arr[1]["role"], "assistant");
        assert_eq!(arr[2]["content"], "more");
    }

    #[test]
    fn role_serializes_lowercase() {
        // Spot-check every variant — Ollama only accepts the lowercase
        // form, so a future enum rename here would silently break
        // chat with a "role must be one of ..." 400.
        assert_eq!(role_str(ChatRole::System), "system");
        assert_eq!(role_str(ChatRole::User), "user");
        assert_eq!(role_str(ChatRole::Assistant), "assistant");
        assert_eq!(role_str(ChatRole::Tool), "tool");
    }

    #[test]
    fn extracts_error_message_from_ollama_404_body() {
        let body = r#"{"error":"model 'foo' not found, try pulling it first"}"#;
        assert_eq!(
            extract_error_message(body).as_deref(),
            Some("model 'foo' not found, try pulling it first")
        );
    }

    #[test]
    fn extract_error_message_returns_none_for_non_json() {
        assert!(extract_error_message("plain text 500").is_none());
        assert!(extract_error_message("{not json}").is_none());
        assert!(extract_error_message(r#"{"detail":"not the error key"}"#).is_none());
    }

    #[test]
    fn send_chat_round_trip_against_stub() {
        // End-to-end: stub server returns a real-looking chat body;
        // adapter parses it and the wire request matches what
        // /api/chat expects. Captures the request bytes the way the
        // /api/show test does so the assertion isn't just on the
        // parser side.
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            CHAT_FIXTURE.len(),
            CHAT_FIXTURE,
        );
        let captured = Arc::new(Mutex::new(None::<Vec<u8>>));
        let captured_for_thread = captured.clone();
        thread::spawn(move || {
            if let Ok((mut sock, _)) = listener.accept() {
                let request = read_full_request(&mut sock);
                *captured_for_thread.lock().unwrap() = Some(request);
                let _ = sock.write_all(response.as_bytes());
            }
        });

        let msgs = vec![ChatMessage {
            role: ChatRole::User,
            content: "Hello?".into(),
        }];
        let resp = send_chat(
            "127.0.0.1",
            port,
            "llama3:latest",
            &msgs,
            Duration::from_millis(500),
        )
        .expect("send_chat");
        assert_eq!(resp.message.role, ChatRole::Assistant);
        assert_eq!(resp.message.content, "Hello! How can I help you today?");
        assert_eq!(resp.model_id, "llama3:latest");
        assert_eq!(resp.finish, ChatFinish::Stop);

        let request_bytes = captured
            .lock()
            .unwrap()
            .take()
            .expect("stub never received a request");
        let request = std::str::from_utf8(&request_bytes).expect("utf-8 request");
        assert!(
            request.starts_with("POST /api/chat HTTP/1.1\r\n"),
            "expected POST /api/chat, got start: {:?}",
            request.lines().next()
        );
        let body = request.split_once("\r\n\r\n").map(|(_, b)| b).unwrap_or("");
        assert!(
            body.contains("\"stream\":false"),
            "request body missing stream:false: {body:?}"
        );
        assert!(
            body.contains("\"model\":\"llama3:latest\""),
            "request body missing model: {body:?}"
        );
    }

    #[test]
    fn send_chat_maps_404_to_model_not_found() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let body = r#"{"error":"model 'ghost:tag' not found, try pulling it first"}"#;
        let response = format!(
            "HTTP/1.1 404 Not Found\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body,
        );
        thread::spawn(move || {
            if let Ok((mut sock, _)) = listener.accept() {
                use crate::providers::http::drain_request;
                drain_request(&mut sock);
                let _ = sock.write_all(response.as_bytes());
            }
        });

        let msgs = vec![ChatMessage {
            role: ChatRole::User,
            content: "hi".into(),
        }];
        let err = send_chat(
            "127.0.0.1",
            port,
            "ghost:tag",
            &msgs,
            Duration::from_millis(500),
        )
        .expect_err("should map 404 to ModelNotFound");
        match err {
            ChatError::ModelNotFound { model, message } => {
                assert_eq!(model, "ghost:tag");
                assert!(message.contains("not found"), "msg was: {message:?}");
            }
            other => panic!("expected ModelNotFound, got {other:?}"),
        }
    }

    #[test]
    fn send_chat_maps_5xx_to_bad_status() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let response =
            "HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
        thread::spawn(move || {
            if let Ok((mut sock, _)) = listener.accept() {
                use crate::providers::http::drain_request;
                drain_request(&mut sock);
                let _ = sock.write_all(response.as_bytes());
            }
        });

        let msgs = vec![ChatMessage {
            role: ChatRole::User,
            content: "hi".into(),
        }];
        let err = send_chat(
            "127.0.0.1",
            port,
            "anything",
            &msgs,
            Duration::from_millis(500),
        )
        .expect_err("should map 500 to BadStatus");
        match err {
            ChatError::BadStatus { status, .. } => assert_eq!(status, 500),
            other => panic!("expected BadStatus, got {other:?}"),
        }
    }

    #[test]
    fn send_chat_marks_done_false_as_length_finish() {
        // Ollama's non-streaming responses normally have done:true.
        // We treat done:false as a truncated reply so the UI can flag
        // it. Synthetic body — not common in the wild but documented.
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let body = r#"{"model":"x","message":{"role":"assistant","content":"..."},"done":false}"#;
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body,
        );
        thread::spawn(move || {
            if let Ok((mut sock, _)) = listener.accept() {
                use crate::providers::http::drain_request;
                drain_request(&mut sock);
                let _ = sock.write_all(response.as_bytes());
            }
        });

        let msgs = vec![ChatMessage {
            role: ChatRole::User,
            content: "hi".into(),
        }];
        let resp = send_chat("127.0.0.1", port, "x", &msgs, Duration::from_millis(500))
            .expect("send_chat");
        assert_eq!(resp.finish, ChatFinish::Length);
    }
}
