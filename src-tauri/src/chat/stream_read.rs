//! Bounded logical-line reader shared by the two streaming chat
//! adapters (Thermos audit L1).
//!
//! Both MLX-LM SSE and Ollama NDJSON frame their streams as
//! newline-terminated logical lines. The pre-fix read loops wrapped
//! `BufReader::read_line`, which appends to the line buffer without
//! any bound *inside a single call* — a buggy or malicious local
//! server that streams bytes with no newline would grow the buffer
//! indefinitely, and because `read_line` only returns on newline,
//! EOF, or error, the loop's cancel/deadline checks never ran while
//! data kept arriving. `read_line_bounded` replaces it: same polling
//! contract (cancel flag + overall deadline re-checked on every
//! socket timeout, partial bytes retained across `WouldBlock`), plus
//! a hard cap on the logical line.

use std::io::{self, BufRead, BufReader};
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

/// Hard cap on one logical stream line (one SSE `data:` line or one
/// NDJSON frame), measured on the line's content EXCLUDING the
/// terminating `\n` — so a valid frame of exactly this size still
/// parses. Real frames are a few KiB of delta text at most (the
/// largest legitimate line is Ollama's final telemetry frame);
/// 1 MiB is orders of magnitude above anything either wire format
/// produces while still bounding a runaway server. Documented in
/// `docs/IPC_CONTRACT.md § chat`.
pub(crate) const MAX_STREAM_LINE_BYTES: usize = 1024 * 1024;

/// What one bounded line read produced. Same three-way shape both
/// adapters previously declared privately.
#[derive(Debug)]
pub(crate) enum ReadOutcome {
    /// A logical line landed in `buf` (terminating `\n` included,
    /// `read_line` parity — callers strip it). At EOF a final
    /// unterminated line is also delivered this way; the NEXT call
    /// returns `Eof`.
    Line,
    /// Clean end of stream with no buffered partial line.
    Eof,
    /// The cancel flag tripped between socket reads.
    Cancelled,
}

/// The `WouldBlock`/`TimedOut` pair the short per-read socket
/// timeout produces constantly while a model is "thinking".
pub(crate) fn is_timeout_kind(kind: io::ErrorKind) -> bool {
    matches!(kind, io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut)
}

/// Read one newline-terminated logical line into `buf`, bounded by
/// `MAX_STREAM_LINE_BYTES`. Drives `fill_buf`/`consume` directly
/// instead of `read_line` so the cap is enforced *while the line
/// accumulates* — `read_line` only returns on newline/EOF/error, so
/// a server streaming newline-less bytes would grow the buffer
/// without bound before any caller-side length check could run.
///
/// Contract (all `read_line_polled` parity, pinned by the existing
/// adapter tests):
///   * The cancel flag and the overall deadline are re-checked on
///     every socket timeout (`WouldBlock`/`TimedOut`), and partial
///     bytes accumulated before a timeout are retained so the next
///     poll resumes the same line.
///   * A line of content exactly `MAX_STREAM_LINE_BYTES` (excluding
///     the `\n`) is delivered; one byte past it is rejected with an
///     `InvalidData` io error naming the cap, which both adapters
///     surface through their existing `ChatError::Transport` arm.
///   * EOF with a non-empty partial line delivers that line first
///     (without a trailing `\n`); EOF with nothing buffered returns
///     `Eof`.
///   * Invalid UTF-8 in a completed line maps to `InvalidData`,
///     matching `read_line`'s error kind.
pub(crate) fn read_line_bounded(
    reader: &mut BufReader<TcpStream>,
    buf: &mut String,
    cancel: &Arc<AtomicBool>,
    deadline: Instant,
) -> io::Result<ReadOutcome> {
    let mut bytes: Vec<u8> = Vec::new();
    loop {
        if cancel.load(Ordering::SeqCst) {
            return Ok(ReadOutcome::Cancelled);
        }
        if Instant::now() > deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "chat stream deadline elapsed",
            ));
        }
        let available = match reader.fill_buf() {
            Ok(available) => available,
            Err(e) if is_timeout_kind(e.kind()) => continue,
            Err(e) => return Err(e),
        };
        if available.is_empty() {
            // EOF. Deliver a buffered partial line as the final
            // (unterminated) line; otherwise signal end of stream.
            if bytes.is_empty() {
                return Ok(ReadOutcome::Eof);
            }
            buf.push_str(&into_utf8(bytes)?);
            return Ok(ReadOutcome::Line);
        }
        let newline_at = available.iter().position(|&b| b == b'\n');
        let take = newline_at.map(|i| i + 1).unwrap_or(available.len());
        bytes.extend_from_slice(&available[..take]);
        reader.consume(take);

        let content_len = match newline_at {
            Some(_) => bytes.len() - 1,
            None => bytes.len(),
        };
        if content_len > MAX_STREAM_LINE_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("stream frame exceeded {MAX_STREAM_LINE_BYTES} bytes without a line break"),
            ));
        }
        if newline_at.is_some() {
            buf.push_str(&into_utf8(bytes)?);
            return Ok(ReadOutcome::Line);
        }
    }
}

fn into_utf8(bytes: Vec<u8>) -> io::Result<String> {
    String::from_utf8(bytes).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "stream did not contain valid UTF-8",
        )
    })
}
