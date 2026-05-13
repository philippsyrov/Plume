//! HTTP framing helpers shared by the blocking and streaming Ollama
//! adapters. These functions are concerned with the wire — header
//! parsing, error-body extraction, and small role-name helpers — and
//! deliberately know nothing about NDJSON framing or chat semantics.

use std::io::{self, BufReader, Read};
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

use super::super::ChatRole;
use super::streaming::is_timeout_kind;

pub(super) const CHAT_PATH: &str = "/api/chat";

pub(super) fn role_str(role: ChatRole) -> &'static str {
    match role {
        ChatRole::System => "system",
        ChatRole::User => "user",
        ChatRole::Assistant => "assistant",
        ChatRole::Tool => "tool",
    }
}

/// Pull `{"error": "..."}` out of an Ollama error body when present.
/// Ollama's error responses are not strictly schema'd; if the body
/// isn't JSON or doesn't have an `error` field we return `None` and
/// the caller falls back to the raw body.
pub(super) fn extract_error_message(body: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(body).ok()?;
    value
        .get("error")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

/// Read HTTP response head from the raw TCP stream one byte at a
/// time, stopping exactly at the `\r\n\r\n` separator. Reading
/// byte-by-byte avoids ever over-reading into the body — once
/// `stream_chat` wraps the stream in a `BufReader`, every byte the
/// reader pulls from the kernel will be a body byte. The cost is
/// ~200 syscalls per chat call (one per header byte), which is
/// negligible on localhost.
///
/// Honors the same cancel + deadline contract as the body loop.
pub(super) fn read_response_head(
    stream: &mut &TcpStream,
    cancel: &Arc<AtomicBool>,
    deadline: Instant,
) -> io::Result<(u16, String)> {
    let mut buf: Vec<u8> = Vec::with_capacity(256);
    let mut one = [0u8; 1];
    loop {
        if cancel.load(Ordering::SeqCst) {
            return Err(io::Error::other("cancelled during header read"));
        }
        if Instant::now() > deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "chat stream deadline elapsed during header read",
            ));
        }
        match stream.read(&mut one) {
            Ok(0) => {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "connection closed before HTTP headers",
                ));
            }
            Ok(_) => {
                buf.push(one[0]);
                if buf.len() >= 4 && &buf[buf.len() - 4..] == b"\r\n\r\n" {
                    let header_text = String::from_utf8_lossy(&buf).to_string();
                    let status = parse_status_line(&header_text).ok_or_else(|| {
                        io::Error::new(
                            io::ErrorKind::InvalidData,
                            format!(
                                "could not parse status line: {:?}",
                                header_text.lines().next().unwrap_or("")
                            ),
                        )
                    })?;
                    return Ok((status, header_text));
                }
                if buf.len() > 64 * 1024 {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "response headers exceeded 64 KiB",
                    ));
                }
            }
            Err(e) if is_timeout_kind(e.kind()) => continue,
            Err(e) => return Err(e),
        }
    }
}

pub(super) fn parse_status_line(header_text: &str) -> Option<u16> {
    let line = header_text.lines().next()?;
    let mut parts = line.split_whitespace();
    let _version = parts.next();
    parts.next()?.parse::<u16>().ok()
}

/// Pull whatever's left on the stream and return it as a UTF-8
/// string. Used only on error paths to surface Ollama's
/// `{"error": "..."}` body. The 64 KiB cap keeps a misbehaving
/// daemon from filling memory with a non-2xx body.
pub(super) fn drain_body_to_string(
    reader: &mut BufReader<TcpStream>,
    cancel: &Arc<AtomicBool>,
    deadline: Instant,
) -> io::Result<String> {
    let mut buf = Vec::with_capacity(512);
    let mut tmp = [0u8; 512];
    loop {
        if cancel.load(Ordering::SeqCst) {
            break;
        }
        if Instant::now() > deadline {
            break;
        }
        match reader.read(&mut tmp) {
            Ok(0) => break,
            Ok(n) => {
                buf.extend_from_slice(&tmp[..n]);
                if buf.len() > 64 * 1024 {
                    break;
                }
            }
            Err(e) if is_timeout_kind(e.kind()) => continue,
            Err(e) => return Err(e),
        }
    }
    Ok(String::from_utf8_lossy(&buf).to_string())
}
