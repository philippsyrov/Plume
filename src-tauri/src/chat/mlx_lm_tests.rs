//! D45 tests for the MLX-LM chat adapter.
//!
//! The strategy is the same one the Ollama streaming tests use:
//! stand up a one-shot `TcpListener` on an ephemeral port, hand the
//! port to `stream_chat`, and have the listener handle the inbound
//! HTTP request with a canned response. No actual mlx-lm install,
//! no Python, no model.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use super::*;

/// Bind 127.0.0.1:0 and return (listener, port). Same idiom as the
/// supervisor's `allocate_port` but the listener stays open so the
/// test thread can `accept()` on it.
fn bind_local() -> (TcpListener, u16) {
    let l = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral");
    let port = l.local_addr().unwrap().port();
    (l, port)
}

/// Read the request bytes off a connected client socket until the
/// `\r\n\r\n` header boundary plus exactly `content_length` body
/// bytes. We never read past the body so the kernel doesn't drop
/// bytes if the test writes a faster response than the client.
fn read_http_request(socket: &mut TcpStream) -> (String, String) {
    let mut reader = BufReader::new(socket);
    let mut headers = Vec::new();
    let mut content_length: usize = 0;
    loop {
        let mut line = String::new();
        reader.read_line(&mut line).expect("read header line");
        if line == "\r\n" || line == "\n" || line.is_empty() {
            break;
        }
        if let Some(rest) = line.to_ascii_lowercase().strip_prefix("content-length:") {
            content_length = rest.trim().parse().unwrap_or(0);
        }
        headers.push(line);
    }
    let mut body = vec![0u8; content_length];
    reader.read_exact(&mut body).expect("read body");
    (headers.concat(), String::from_utf8_lossy(&body).to_string())
}

/// Spawn a fake mlx-lm server. The closure decides what to write
/// back; it gets the parsed request (headers + body) and a writer
/// it must flush. Returns the listener port.
fn spawn_fake<F>(handle: F) -> u16
where
    F: FnOnce(String, String, &mut TcpStream) + Send + 'static,
{
    let (listener, port) = bind_local();
    thread::spawn(move || {
        let (mut socket, _addr) = listener.accept().expect("accept");
        let (headers, body) = read_http_request(&mut socket);
        handle(headers, body, &mut socket);
    });
    port
}

fn no_cancel() -> Arc<AtomicBool> {
    Arc::new(AtomicBool::new(false))
}

fn far_deadline() -> Instant {
    Instant::now() + Duration::from_secs(10)
}

fn user_msg(text: &str) -> ChatMessage {
    ChatMessage {
        role: ChatRole::User,
        content: text.to_string(),
    }
}

// --- request shape ------------------------------------------------------

#[test]
fn build_request_body_sets_stream_true_and_includes_usage() {
    let body = build_request_body("gemma-2b", &[user_msg("hi")]);
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["model"], "gemma-2b");
    assert_eq!(v["stream"], true);
    assert_eq!(v["stream_options"]["include_usage"], true);
    assert_eq!(v["messages"][0]["role"], "user");
    assert_eq!(v["messages"][0]["content"], "hi");
    // D129C: the output cap is explicit on the wire, never a silent
    // server default.
    assert_eq!(v["max_tokens"], super::MAX_OUTPUT_TOKENS);
}

#[test]
fn role_str_maps_every_variant() {
    assert_eq!(role_str(ChatRole::System), "system");
    assert_eq!(role_str(ChatRole::User), "user");
    assert_eq!(role_str(ChatRole::Assistant), "assistant");
    assert_eq!(role_str(ChatRole::Tool), "tool");
}

// --- happy path: deltas + done -----------------------------------------

#[test]
fn stream_chat_emits_each_delta_and_returns_done() {
    let port = spawn_fake(|headers, _body, socket| {
        // Verify the request advertises the OpenAI chat path and
        // SSE accept. Stream a small canned SSE response.
        assert!(headers.contains("POST /v1/chat/completions HTTP/1.1"));
        assert!(headers.contains("Accept: text/event-stream"));
        let sse = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n\
            data: {\"choices\":[{\"delta\":{\"role\":\"assistant\"},\"index\":0}]}\n\n\
            data: {\"choices\":[{\"delta\":{\"content\":\"Hel\"},\"index\":0}]}\n\n\
            data: {\"choices\":[{\"delta\":{\"content\":\"lo\"},\"index\":0,\"finish_reason\":\"stop\"}]}\n\n\
            data: {\"choices\":[],\"usage\":{\"prompt_tokens\":7,\"completion_tokens\":2,\"total_tokens\":9}}\n\n\
            data: [DONE]\n\n";
        socket.write_all(sse.as_bytes()).unwrap();
        socket.flush().unwrap();
    });

    let mut deltas: Vec<String> = Vec::new();
    let outcome = stream_chat(
        port,
        "gemma-2b",
        &[user_msg("hi")],
        no_cancel(),
        |d| deltas.push(d.to_string()),
        Duration::from_secs(2),
        far_deadline(),
    )
    .expect("ok");

    assert_eq!(deltas, vec!["Hel", "lo"]);
    let StreamOutcome::Done { model_id, stats } = outcome else {
        panic!("expected Done, got {outcome:?}");
    };
    assert_eq!(model_id, "gemma-2b");
    assert_eq!(stats.prompt_tokens, Some(7));
    assert_eq!(stats.completion_tokens, Some(2));
}

#[test]
fn stream_chat_handles_inlined_usage_alongside_stop_chunk() {
    // Some OpenAI-compat servers inline `usage` on the same chunk
    // that carries `finish_reason: "stop"`. D39's parser already
    // emits two events for that frame; this test pins the adapter's
    // handling end-to-end.
    let port = spawn_fake(|_h, _b, socket| {
        let sse = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n\
            data: {\"choices\":[{\"delta\":{\"content\":\"Hi\"},\"index\":0,\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":4,\"completion_tokens\":1}}\n\n\
            data: [DONE]\n\n";
        socket.write_all(sse.as_bytes()).unwrap();
    });

    let mut deltas = Vec::new();
    let outcome = stream_chat(
        port,
        "m",
        &[user_msg("hi")],
        no_cancel(),
        |d| deltas.push(d.to_string()),
        Duration::from_secs(2),
        far_deadline(),
    )
    .expect("ok");
    assert_eq!(deltas, vec!["Hi"]);
    let StreamOutcome::Done { stats, .. } = outcome else {
        panic!("expected Done");
    };
    assert_eq!(stats.prompt_tokens, Some(4));
    assert_eq!(stats.completion_tokens, Some(1));
}

#[test]
fn stream_chat_done_without_usage_still_completes_with_default_stats() {
    // mlx-lm without the include_usage flag (or a future build that
    // drops the chunk) — the adapter must still close cleanly,
    // just with `None`/`None` stats.
    let port = spawn_fake(|_h, _b, socket| {
        let sse = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n\
            data: {\"choices\":[{\"delta\":{\"content\":\"ok\"},\"index\":0,\"finish_reason\":\"stop\"}]}\n\n\
            data: [DONE]\n\n";
        socket.write_all(sse.as_bytes()).unwrap();
    });

    let outcome = stream_chat(
        port,
        "m",
        &[user_msg("hi")],
        no_cancel(),
        |_| {},
        Duration::from_secs(2),
        far_deadline(),
    )
    .expect("ok");
    let StreamOutcome::Done { stats, .. } = outcome else {
        panic!("expected Done");
    };
    assert!(stats.prompt_tokens.is_none());
    assert!(stats.completion_tokens.is_none());
}

// --- EOF and cancel paths ----------------------------------------------

#[test]
fn stream_chat_returns_eof_before_done_when_server_closes_mid_stream() {
    let port = spawn_fake(|_h, _b, socket| {
        let sse = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n\
            data: {\"choices\":[{\"delta\":{\"content\":\"part\"},\"index\":0}]}\n\n";
        socket.write_all(sse.as_bytes()).unwrap();
        // Drop the socket without sending [DONE] — simulates a
        // crashed server or a network drop.
    });

    let mut deltas = Vec::new();
    let outcome = stream_chat(
        port,
        "m",
        &[user_msg("hi")],
        no_cancel(),
        |d| deltas.push(d.to_string()),
        Duration::from_secs(2),
        far_deadline(),
    )
    .expect("ok");
    assert_eq!(deltas, vec!["part"]);
    assert!(matches!(outcome, StreamOutcome::EofBeforeDone { .. }));
}

#[test]
fn stream_chat_returns_cancelled_when_cancel_flag_set_mid_stream() {
    // The server writes one chunk, then waits forever before the
    // next one. The client trips the cancel flag from another thread
    // while the read loop is parked on the second read.
    let port = spawn_fake(|_h, _b, socket| {
        let sse = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n\
            data: {\"choices\":[{\"delta\":{\"content\":\"A\"},\"index\":0}]}\n\n";
        socket.write_all(sse.as_bytes()).unwrap();
        socket.flush().unwrap();
        // Park; the client's cancel path is what we're testing.
        thread::sleep(Duration::from_secs(5));
    });

    let cancel = Arc::new(AtomicBool::new(false));
    let cancel_clone = cancel.clone();
    thread::spawn(move || {
        thread::sleep(Duration::from_millis(300));
        cancel_clone.store(true, std::sync::atomic::Ordering::SeqCst);
    });

    let outcome = stream_chat(
        port,
        "m",
        &[user_msg("hi")],
        cancel,
        |_| {},
        Duration::from_secs(2),
        Instant::now() + Duration::from_secs(3),
    )
    .expect("ok");
    assert!(matches!(outcome, StreamOutcome::Cancelled { .. }));
}

// --- error mapping -----------------------------------------------------

#[test]
fn stream_chat_maps_404_to_model_not_found_with_openai_error_shape() {
    let port = spawn_fake(|_h, _b, socket| {
        let body = r#"{"error":{"message":"model 'nope' not found","type":"invalid_request"}}"#;
        let response = format!(
            "HTTP/1.1 404 Not Found\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        socket.write_all(response.as_bytes()).unwrap();
    });

    let err = stream_chat(
        port,
        "nope",
        &[user_msg("hi")],
        no_cancel(),
        |_| {},
        Duration::from_secs(2),
        far_deadline(),
    )
    .expect_err("404 must error");
    match err {
        ChatError::ModelNotFound { model, message } => {
            assert_eq!(model, "nope");
            assert!(message.contains("not found"));
        }
        other => panic!("expected ModelNotFound, got {other:?}"),
    }
}

#[test]
fn stream_chat_maps_404_with_string_error_field() {
    // Some OpenAI-compat servers return `{"error":"..."}` (string)
    // rather than `{"error":{"message":"..."}}` (object). Both must
    // be parsed.
    let port = spawn_fake(|_h, _b, socket| {
        let body = r#"{"error":"plain string error"}"#;
        let response = format!(
            "HTTP/1.1 404 Not Found\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        socket.write_all(response.as_bytes()).unwrap();
    });
    let err = stream_chat(
        port,
        "x",
        &[user_msg("hi")],
        no_cancel(),
        |_| {},
        Duration::from_secs(2),
        far_deadline(),
    )
    .expect_err("404 must error");
    match err {
        ChatError::ModelNotFound { message, .. } => {
            assert_eq!(message, "plain string error");
        }
        other => panic!("expected ModelNotFound, got {other:?}"),
    }
}

#[test]
fn stream_chat_maps_500_to_bad_status() {
    let port = spawn_fake(|_h, _b, socket| {
        let body = "boom";
        let response = format!(
            "HTTP/1.1 500 Internal Server Error\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        socket.write_all(response.as_bytes()).unwrap();
    });
    let err = stream_chat(
        port,
        "x",
        &[user_msg("hi")],
        no_cancel(),
        |_| {},
        Duration::from_secs(2),
        far_deadline(),
    )
    .expect_err("500 must error");
    match err {
        ChatError::BadStatus { status, message } => {
            assert_eq!(status, 500);
            assert!(message.contains("boom"));
        }
        other => panic!("expected BadStatus, got {other:?}"),
    }
}

#[test]
fn stream_chat_surfaces_transport_error_when_no_server() {
    // Connect to a port we know nothing is listening on. The OS
    // immediately returns ECONNREFUSED.
    let (listener, port) = bind_local();
    drop(listener); // make sure no one's bound to it now
    let err = stream_chat(
        port,
        "x",
        &[user_msg("hi")],
        no_cancel(),
        |_| {},
        Duration::from_millis(500),
        Instant::now() + Duration::from_secs(1),
    )
    .expect_err("unbound port must error");
    match err {
        ChatError::Transport { port: p, .. } => assert_eq!(p, port),
        other => panic!("expected Transport, got {other:?}"),
    }
}

// --- request validation ------------------------------------------------

#[test]
fn stream_chat_sends_messages_in_order_with_correct_roles() {
    // The order of messages and the role strings are part of the
    // wire contract; capture both.
    use std::sync::Mutex;
    let captured: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
    let captured_for_fake = captured.clone();
    let port = spawn_fake(move |_headers, body, socket| {
        *captured_for_fake.lock().unwrap() = Some(body.clone());
        let sse = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\ndata: [DONE]\n\n";
        socket.write_all(sse.as_bytes()).unwrap();
    });

    let msgs = vec![
        ChatMessage {
            role: ChatRole::System,
            content: "be helpful".into(),
        },
        ChatMessage {
            role: ChatRole::User,
            content: "hi".into(),
        },
        ChatMessage {
            role: ChatRole::Assistant,
            content: "hello".into(),
        },
        ChatMessage {
            role: ChatRole::User,
            content: "thanks".into(),
        },
    ];
    let _ = stream_chat(
        port,
        "m",
        &msgs,
        no_cancel(),
        |_| {},
        Duration::from_secs(2),
        far_deadline(),
    );

    let body = captured.lock().unwrap().clone().expect("body captured");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    let msgs_v = v["messages"].as_array().unwrap();
    assert_eq!(msgs_v.len(), 4);
    assert_eq!(msgs_v[0]["role"], "system");
    assert_eq!(msgs_v[0]["content"], "be helpful");
    assert_eq!(msgs_v[1]["role"], "user");
    assert_eq!(msgs_v[2]["role"], "assistant");
    assert_eq!(msgs_v[3]["content"], "thanks");
}

#[test]
fn extract_error_message_handles_both_openai_shapes() {
    assert_eq!(
        extract_error_message(r#"{"error":"just a string"}"#),
        Some("just a string".to_string())
    );
    assert_eq!(
        extract_error_message(r#"{"error":{"message":"nested","type":"x"}}"#),
        Some("nested".to_string())
    );
    assert_eq!(extract_error_message(r#"{"not_error":"x"}"#), None);
    assert_eq!(extract_error_message("not json"), None);
}
