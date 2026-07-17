//! Tests for the streaming Ollama adapter. Split out of
//! `streaming.rs` in the same sibling-file pattern as
//! `chat/mlx_lm_tests.rs` when the Thermos-L1 frame-cap
//! regressions pushed the combined file past the 800-line
//! decomposition cap. This is a CHILD module of `streaming`
//! (declared with `#[path]`), so `super::` resolves exactly as
//! it did when the module was inline.

use super::super::super::{ChatMessage, ChatRole};
use super::super::{ChatError, StreamOutcome};
use super::{build_request_body_streaming_with_images, stream_chat};

// ============ D7.1 streaming tests ============
//
// The streaming tests use a slower stub-server pattern: instead
// of writing the entire response in one `write_all`, they emit
// frames with deliberate gaps so we can assert that
// `stream_chat` calls `on_delta` per-frame (not once at the
// end) and that the cancel flag short-circuits mid-stream.

use std::io::Write;
use std::net::TcpListener;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

fn streaming_response(frames: &[&str]) -> Vec<u8> {
    // Chunked-style NDJSON response. The HTTP head plus one
    // frame per line; the connection closes after the last
    // frame so the client sees an EOF if no `done:true` arrived.
    let body: String = frames.iter().map(|f| format!("{f}\n")).collect();
    let header =
        "HTTP/1.1 200 OK\r\nContent-Type: application/x-ndjson\r\nConnection: close\r\n\r\n";
    let mut out = Vec::new();
    out.extend_from_slice(header.as_bytes());
    out.extend_from_slice(body.as_bytes());
    out
}

#[test]
fn stream_chat_round_trip_emits_per_frame_deltas() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let frames = vec![
        r#"{"model":"llama3:latest","created_at":"2024-04-19T10:00:00Z","message":{"role":"assistant","content":"Hel"},"done":false}"#,
        r#"{"model":"llama3:latest","created_at":"2024-04-19T10:00:01Z","message":{"role":"assistant","content":"lo"},"done":false}"#,
        r#"{"model":"llama3:latest","created_at":"2024-04-19T10:00:02Z","message":{"role":"assistant","content":"!"},"done":false}"#,
        // Final frame carries the D9 telemetry: 3 output tokens
        // (matches the three preceding content frames) in 600 ms,
        // a 12-token prompt evaluated in 100 ms. Sized so the
        // tok/s assertion is exact: 3 / 0.6 = 5.0.
        r#"{"model":"llama3:latest","created_at":"2024-04-19T10:00:03Z","message":{"role":"assistant","content":""},"done":true,"done_reason":"stop","total_duration":700000000,"prompt_eval_count":12,"prompt_eval_duration":100000000,"eval_count":3,"eval_duration":600000000}"#,
    ];
    let response = streaming_response(&frames);
    thread::spawn(move || {
        if let Ok((mut sock, _)) = listener.accept() {
            use crate::providers::http::drain_request;
            drain_request(&mut sock);
            let _ = sock.write_all(&response);
        }
    });

    let cancel = Arc::new(AtomicBool::new(false));
    let collected = Arc::new(Mutex::new(String::new()));
    let collected_for_cb = collected.clone();

    let outcome = stream_chat(
        "127.0.0.1",
        port,
        "llama3:latest",
        &[ChatMessage {
            role: ChatRole::User,
            content: "hi".into(),
        }],
        cancel,
        move |delta| collected_for_cb.lock().unwrap().push_str(delta),
        Duration::from_millis(500),
        Instant::now() + Duration::from_secs(5),
    )
    .expect("stream_chat");

    match outcome {
        StreamOutcome::Done { model_id, stats } => {
            assert_eq!(model_id, "llama3:latest");
            // D9: the four metric fields surface verbatim from
            // the final NDJSON frame. The parser doesn't yet
            // convert ns→ms; that happens in the command layer.
            assert_eq!(stats.eval_count, Some(3));
            assert_eq!(stats.eval_duration_ns, Some(600_000_000));
            assert_eq!(stats.prompt_eval_count, Some(12));
            assert_eq!(stats.prompt_eval_duration_ns, Some(100_000_000));
        }
        other => panic!("expected Done, got {other:?}"),
    }
    assert_eq!(collected.lock().unwrap().as_str(), "Hello!");
}

#[test]
fn stream_chat_done_without_telemetry_returns_all_none_stats() {
    // Defensive parse: a daemon (or test stub) that produces
    // `done:true` without the optional metrics fields should
    // still succeed and surface `None` for each metric. This
    // pins the `#[serde(default)]` behavior; without it a
    // minor Ollama release that dropped a field would 500 the
    // stream parse.
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let frames = vec![r#"{"model":"m","message":{"role":"assistant","content":""},"done":true}"#];
    let response = streaming_response(&frames);
    thread::spawn(move || {
        if let Ok((mut sock, _)) = listener.accept() {
            use crate::providers::http::drain_request;
            drain_request(&mut sock);
            let _ = sock.write_all(&response);
        }
    });

    let cancel = Arc::new(AtomicBool::new(false));
    let outcome = stream_chat(
        "127.0.0.1",
        port,
        "m",
        &[ChatMessage {
            role: ChatRole::User,
            content: "hi".into(),
        }],
        cancel,
        |_| {},
        Duration::from_millis(500),
        Instant::now() + Duration::from_secs(5),
    )
    .expect("stream_chat");

    match outcome {
        StreamOutcome::Done { stats, .. } => {
            assert_eq!(stats.eval_count, None);
            assert_eq!(stats.eval_duration_ns, None);
            assert_eq!(stats.prompt_eval_count, None);
            assert_eq!(stats.prompt_eval_duration_ns, None);
        }
        other => panic!("expected Done, got {other:?}"),
    }
}

#[test]
fn stream_chat_request_body_has_stream_true() {
    // Wire-level check: the streaming variant must set
    // `stream: true`. A regression to `false` would make Ollama
    // return a single non-NDJSON object and the line loop would
    // never advance.
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let frames = vec![r#"{"model":"m","message":{"role":"assistant","content":""},"done":true}"#];
    let response = streaming_response(&frames);
    let captured = Arc::new(Mutex::new(None::<Vec<u8>>));
    let captured_for_thread = captured.clone();
    thread::spawn(move || {
        if let Ok((mut sock, _)) = listener.accept() {
            let request = crate::providers::http::read_full_request(&mut sock);
            *captured_for_thread.lock().unwrap() = Some(request);
            let _ = sock.write_all(&response);
        }
    });

    let cancel = Arc::new(AtomicBool::new(false));
    let _ = stream_chat(
        "127.0.0.1",
        port,
        "m",
        &[ChatMessage {
            role: ChatRole::User,
            content: "x".into(),
        }],
        cancel,
        |_| {},
        Duration::from_millis(500),
        Instant::now() + Duration::from_secs(5),
    )
    .expect("stream_chat");

    let request_bytes = captured
        .lock()
        .unwrap()
        .take()
        .expect("stub never received a request");
    let request = std::str::from_utf8(&request_bytes).expect("utf-8 request");
    let body = request.split_once("\r\n\r\n").map(|(_, b)| b).unwrap_or("");
    assert!(
        body.contains("\"stream\":true"),
        "streaming request must set stream:true; body was: {body:?}"
    );
}

#[test]
fn image_bytes_attach_only_to_the_final_user_message() {
    let body = build_request_body_streaming_with_images(
        "vision-model",
        &[
            ChatMessage {
                role: ChatRole::User,
                content: "earlier".into(),
            },
            ChatMessage {
                role: ChatRole::Assistant,
                content: "reply".into(),
            },
            ChatMessage {
                role: ChatRole::User,
                content: "inspect this".into(),
            },
        ],
        &[vec![0, 1, 2, 3]],
    );
    let value: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert!(value["messages"][0].get("images").is_none());
    assert!(value["messages"][1].get("images").is_none());
    assert_eq!(
        value["messages"][2]["images"],
        serde_json::json!(["AAECAw=="])
    );
}

#[test]
fn stream_chat_maps_404_to_model_not_found() {
    // 404 path is the same as send_chat's: single JSON body
    // delivered before any NDJSON, so the body loop never runs.
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let body = r#"{"error":"model 'ghost' not found"}"#;
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

    let cancel = Arc::new(AtomicBool::new(false));
    let err = stream_chat(
        "127.0.0.1",
        port,
        "ghost",
        &[ChatMessage {
            role: ChatRole::User,
            content: "x".into(),
        }],
        cancel,
        |_| {},
        Duration::from_millis(500),
        Instant::now() + Duration::from_secs(5),
    )
    .expect_err("404 should error");
    match err {
        ChatError::ModelNotFound { model, message } => {
            assert_eq!(model, "ghost");
            assert!(message.contains("not found"));
        }
        other => panic!("expected ModelNotFound, got {other:?}"),
    }
}

#[test]
fn stream_chat_returns_cancelled_when_flag_trips_mid_stream() {
    // Stub server emits two frames then sleeps before emitting
    // `done`. The test flips the cancel flag while the server
    // is idle; the stream should return Cancelled rather than
    // wait for the (never-arriving) final frame.
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let cancel = Arc::new(AtomicBool::new(false));
    let cancel_for_test = cancel.clone();
    thread::spawn(move || {
        if let Ok((mut sock, _)) = listener.accept() {
            use crate::providers::http::drain_request;
            drain_request(&mut sock);
            let _ = sock.write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: application/x-ndjson\r\nConnection: close\r\n\r\n",
            );
            let _ = sock.write_all(
                b"{\"model\":\"m\",\"message\":{\"role\":\"assistant\",\"content\":\"He\"},\"done\":false}\n",
            );
            let _ = sock.write_all(
                b"{\"model\":\"m\",\"message\":{\"role\":\"assistant\",\"content\":\"llo\"},\"done\":false}\n",
            );
            // Hold the socket open without writing more; the
            // client's cancel-flag check should fire before we
            // ever send another frame.
            thread::sleep(Duration::from_secs(2));
        }
    });

    // Race the cancel: wait a bit so the first two frames land
    // in the buffer, then flip the flag.
    thread::spawn(move || {
        thread::sleep(Duration::from_millis(400));
        cancel_for_test.store(true, Ordering::SeqCst);
    });

    let collected = Arc::new(Mutex::new(String::new()));
    let collected_for_cb = collected.clone();
    let outcome = stream_chat(
        "127.0.0.1",
        port,
        "m",
        &[ChatMessage {
            role: ChatRole::User,
            content: "hi".into(),
        }],
        cancel,
        move |delta| collected_for_cb.lock().unwrap().push_str(delta),
        Duration::from_millis(500),
        Instant::now() + Duration::from_secs(5),
    )
    .expect("stream_chat returns cancelled, not errored");

    match outcome {
        StreamOutcome::Cancelled { model_id } => assert_eq!(model_id.as_deref(), Some("m")),
        other => panic!("expected Cancelled, got {other:?}"),
    }
    // Both pre-cancel frames should have been forwarded.
    let collected = collected.lock().unwrap();
    assert_eq!(collected.as_str(), "Hello");
}

#[test]
fn stream_chat_treats_in_stream_error_frame_as_bad_status() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let frames = vec![r#"{"error":"model crashed mid-generation"}"#];
    let response = streaming_response(&frames);
    thread::spawn(move || {
        if let Ok((mut sock, _)) = listener.accept() {
            use crate::providers::http::drain_request;
            drain_request(&mut sock);
            let _ = sock.write_all(&response);
        }
    });

    let cancel = Arc::new(AtomicBool::new(false));
    let err = stream_chat(
        "127.0.0.1",
        port,
        "m",
        &[ChatMessage {
            role: ChatRole::User,
            content: "hi".into(),
        }],
        cancel,
        |_| {},
        Duration::from_millis(500),
        Instant::now() + Duration::from_secs(5),
    )
    .expect_err("in-stream error frame should error");
    match err {
        ChatError::BadStatus { status, message } => {
            assert_eq!(status, 200);
            assert!(message.contains("crashed"));
        }
        other => panic!("expected BadStatus, got {other:?}"),
    }
}

// --- Thermos audit L1: bounded stream frames -----------------------
//
// The NDJSON read loop must reject a logical line that exceeds
// `stream_read::MAX_STREAM_LINE_BYTES` — including when the
// daemon never sends a newline and keeps the socket open, the
// case where the pre-fix `read_line` loop grew the buffer
// without bound and never surfaced control to the
// cancel/deadline checks.

#[test]
fn stream_chat_rejects_an_oversized_frame_while_the_socket_stays_open() {
    use crate::chat::stream_read::MAX_STREAM_LINE_BYTES;

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    thread::spawn(move || {
        if let Ok((mut sock, _)) = listener.accept() {
            use crate::providers::http::drain_request;
            drain_request(&mut sock);
            let _ = sock.write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: application/x-ndjson\r\nConnection: close\r\n\r\n",
            );
            // One giant frame, 64 KiB past the cap, with NO
            // terminating newline...
            let mut giant = vec![b'x'; MAX_STREAM_LINE_BYTES + 64 * 1024];
            giant[0] = b'{';
            let _ = sock.write_all(&giant);
            let _ = sock.flush();
            // ...and the socket stays OPEN, so the reject must
            // come from the cap — not EOF, not the deadline.
            thread::sleep(Duration::from_secs(3));
        }
    });

    let cancel = Arc::new(AtomicBool::new(false));
    let started = Instant::now();
    let err = stream_chat(
        "127.0.0.1",
        port,
        "m",
        &[ChatMessage {
            role: ChatRole::User,
            content: "hi".into(),
        }],
        cancel,
        |_| {},
        Duration::from_millis(500),
        Instant::now() + Duration::from_secs(8),
    )
    .expect_err("an oversized frame must be rejected");
    let elapsed = started.elapsed();

    match err {
        ChatError::Transport { source, .. } => {
            assert_eq!(
                source.kind(),
                std::io::ErrorKind::InvalidData,
                "cap breach maps to InvalidData, got: {source:?}"
            );
            assert!(
                source.to_string().contains("exceeded"),
                "error should name the cap: {source}"
            );
        }
        other => panic!("expected Transport(InvalidData), got {other:?}"),
    }
    assert!(
        elapsed < Duration::from_secs(3),
        "reject must fire at the cap, not at EOF or the deadline (took {elapsed:?})"
    );
}

#[test]
fn stream_chat_accepts_a_frame_exactly_at_the_cap() {
    use crate::chat::stream_read::MAX_STREAM_LINE_BYTES;

    // A real NDJSON delta frame whose content is EXACTLY the cap
    // (excluding the terminating newline) must still stream.
    let prefix = r#"{"model":"m","message":{"role":"assistant","content":""#;
    let suffix = r#""},"done":false}"#;
    let filler_len = MAX_STREAM_LINE_BYTES - prefix.len() - suffix.len();
    let big_frame = format!("{prefix}{}{suffix}", "a".repeat(filler_len));
    assert_eq!(big_frame.len(), MAX_STREAM_LINE_BYTES);
    let done_frame = r#"{"model":"m","message":{"role":"assistant","content":""},"done":true}"#;
    let response = streaming_response(&[big_frame.as_str(), done_frame]);

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    thread::spawn(move || {
        if let Ok((mut sock, _)) = listener.accept() {
            use crate::providers::http::drain_request;
            drain_request(&mut sock);
            let _ = sock.write_all(&response);
        }
    });

    let cancel = Arc::new(AtomicBool::new(false));
    let collected = Arc::new(Mutex::new(String::new()));
    let collected_for_cb = collected.clone();
    let outcome = stream_chat(
        "127.0.0.1",
        port,
        "m",
        &[ChatMessage {
            role: ChatRole::User,
            content: "hi".into(),
        }],
        cancel,
        move |delta| collected_for_cb.lock().unwrap().push_str(delta),
        Duration::from_millis(500),
        Instant::now() + Duration::from_secs(5),
    )
    .expect("a boundary-sized frame must stream cleanly");

    assert!(
        matches!(outcome, StreamOutcome::Done { .. }),
        "expected Done, got {outcome:?}"
    );
    assert_eq!(
        collected.lock().unwrap().len(),
        filler_len,
        "the full delta must arrive"
    );
}

#[test]
fn stream_chat_eof_without_done_returns_eof_before_done() {
    // Server closes the connection cleanly after a few frames
    // but never sends `done: true`. Reflects a real-world
    // truncation; we treat it as `EofBeforeDone` so the
    // command layer maps it to `ChatFinish::Length`.
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let frames = vec![
        r#"{"model":"m","message":{"role":"assistant","content":"par"},"done":false}"#,
        r#"{"model":"m","message":{"role":"assistant","content":"tial"},"done":false}"#,
    ];
    let response = streaming_response(&frames);
    thread::spawn(move || {
        if let Ok((mut sock, _)) = listener.accept() {
            use crate::providers::http::drain_request;
            drain_request(&mut sock);
            let _ = sock.write_all(&response);
            // Server drops; Connection: close header tells the
            // client to expect EOF.
        }
    });

    let cancel = Arc::new(AtomicBool::new(false));
    let collected = Arc::new(Mutex::new(String::new()));
    let collected_for_cb = collected.clone();
    let outcome = stream_chat(
        "127.0.0.1",
        port,
        "m",
        &[ChatMessage {
            role: ChatRole::User,
            content: "hi".into(),
        }],
        cancel,
        move |delta| collected_for_cb.lock().unwrap().push_str(delta),
        Duration::from_millis(500),
        Instant::now() + Duration::from_secs(5),
    )
    .expect("stream_chat should return Ok(EofBeforeDone), not error");
    match outcome {
        StreamOutcome::EofBeforeDone { model_id } => {
            assert_eq!(model_id.as_deref(), Some("m"))
        }
        other => panic!("expected EofBeforeDone, got {other:?}"),
    }
    assert_eq!(collected.lock().unwrap().as_str(), "partial");
}
