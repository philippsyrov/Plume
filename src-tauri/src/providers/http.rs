//! Tiny shared HTTP/1.1 client for adapter probes.
//!
//! Plume's adapter HTTP probes (Ollama `/api/tags` + `/api/show`,
//! LM Studio `/v1/models`, llama.cpp `/v1/models`) all share the same
//! shape: localhost, no TLS, `Connection: close`, GET or JSON-body
//! POST. Pulling in a real HTTP client (`reqwest`, `ureq`) would add
//! a lot of weight for a handful of small calls, so we keep a
//! hand-rolled `std::net`-only implementation here.
//!
//! What this is NOT: a general-purpose HTTP client. No chunked
//! `Transfer-Encoding`, no redirects, no TLS, no proxies, no
//! HTTP/2. If a future adapter needs richer behavior, replace this
//! whole module with `ureq` or similar and route every adapter
//! through it.

use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

/// Issue a one-shot HTTP/1.1 request with `Connection: close` and
/// return the decoded body string. Same caller contract as
/// `health::probe_tcp`: timeout applies per-syscall, errors are
/// `io::Error` so callers can fold them into the existing offline
/// path.
///
/// `body` is sent verbatim when `method` is POST/PUT/etc; pass `None`
/// for a plain GET. We do NOT serialize JSON here — the caller has
/// already built whatever wire body it wants.
pub fn http_request(
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

    // Carry Content-Type + Content-Length only when a body is sent.
    // GET requests omit both, which matches what `/api/tags` and
    // `/v1/models` expect.
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
    // ceiling against a runaway response — realistic localhost
    // payloads for tags/show/models are tens of KB; 4 MiB is far
    // above that.
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

// --- test helpers ----------------------------------------------------
//
// Both `ollama.rs` and `openai_compat.rs` spin up stub TCP listeners
// for round-trip tests. The helpers below are exposed so each adapter
// test can reuse them instead of duplicating header-drain logic.

/// Read enough of the client's request to clear the kernel receive
/// buffer before closing the socket. Closing while bytes are
/// unread causes the kernel to send RST instead of FIN, which
/// surfaces on the client as `ConnectionReset` and hides whatever
/// response we just wrote. Reading until the header terminator
/// (`\r\n\r\n`) is sufficient for the tiny GET probes in this
/// module; POST probes that need body assertion should use
/// `read_full_request` below.
#[cfg(test)]
pub(crate) fn drain_request(sock: &mut TcpStream) {
    let mut buf = [0u8; 1024];
    loop {
        match sock.read(&mut buf) {
            Ok(0) => return,
            Ok(_n) => {
                if buf.windows(4).any(|w| w == b"\r\n\r\n") {
                    return;
                }
            }
            Err(_) => return,
        }
    }
}

/// Read a complete client request (headers + Content-Length body)
/// into a buffer so tests can assert on method / path / body shape.
/// Use when the round-trip test needs to verify what the client
/// actually sent; `drain_request` is enough otherwise.
#[cfg(test)]
pub(crate) fn read_full_request(sock: &mut TcpStream) -> Vec<u8> {
    let mut buf = Vec::new();
    let mut tmp = [0u8; 1024];

    let header_end = loop {
        match sock.read(&mut tmp) {
            Ok(0) => return buf,
            Ok(n) => buf.extend_from_slice(&tmp[..n]),
            Err(_) => return buf,
        }
        if let Some(idx) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break idx + 4;
        }
    };

    let header_text = std::str::from_utf8(&buf[..header_end]).unwrap_or("");
    let content_length = header_text
        .lines()
        .find_map(|line| {
            let mut parts = line.splitn(2, ':');
            let key = parts.next()?.trim();
            let val = parts.next()?.trim();
            if key.eq_ignore_ascii_case("content-length") {
                val.parse::<usize>().ok()
            } else {
                None
            }
        })
        .unwrap_or(0);

    let mut remaining = content_length.saturating_sub(buf.len() - header_end);
    while remaining > 0 {
        let want = remaining.min(tmp.len());
        match sock.read(&mut tmp[..want]) {
            Ok(0) => break,
            Ok(n) => {
                buf.extend_from_slice(&tmp[..n]);
                remaining -= n;
            }
            Err(_) => break,
        }
    }
    buf
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::thread;

    #[test]
    fn http_request_returns_body_for_canned_get_response() {
        // Bind an ephemeral port and pretend to be a localhost daemon
        // for one request. The stub ensures the module works end-to-
        // end without depending on a real Ollama/LM Studio/llama.cpp.
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let canned = "{\"data\":[]}";
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
            "/v1/models",
            None,
            Duration::from_millis(500),
        )
        .expect("http_request");
        assert_eq!(body, canned);
    }

    #[test]
    fn http_request_surfaces_non_2xx_status() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let response =
            "HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
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
            "/v1/models",
            None,
            Duration::from_millis(500),
        )
        .expect_err("err");
        assert!(err.to_string().contains("500"));
    }
}
