//! Capped stdout+stderr capture buffer for supervised children.
//! Extracted from `process.rs` (D117); the supervisor's reader
//! threads push into it and `lookup_diagnostics` snapshots it.

use std::collections::VecDeque;

/// Hard cap on the per-handle stdout+stderr ring buffer. Sized to
/// hold a typical Python traceback (a few hundred bytes) plus the
/// upstream "Loading model from …" lines mlx-lm prints during
/// startup. 16 KiB is far past either while still preventing a
/// runaway child from filling memory.
pub const RING_BUFFER_CAP: usize = 16 * 1024;

/// Bounded byte buffer for captured stdout + stderr. Push beyond
/// `capacity` drops the oldest bytes so a runaway child's output
/// can't grow memory unbounded. `snapshot` returns a lossy-UTF-8
/// view for inclusion in error messages and tracing logs.
#[derive(Debug)]
pub struct RingBuffer {
    capacity: usize,
    data: VecDeque<u8>,
}

impl RingBuffer {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            data: VecDeque::with_capacity(capacity),
        }
    }

    pub fn push_bytes(&mut self, bytes: &[u8]) {
        if self.capacity == 0 {
            return;
        }
        for &byte in bytes {
            if self.data.len() == self.capacity {
                self.data.pop_front();
            }
            self.data.push_back(byte);
        }
    }

    pub fn snapshot(&self) -> String {
        // Iterating + collecting through `from_utf8_lossy` would
        // require a contiguous slice; `make_contiguous` would
        // mutate the buffer and we want an immutable read. Build a
        // contiguous Vec and decode in one go.
        let bytes: Vec<u8> = self.data.iter().copied().collect();
        String::from_utf8_lossy(&bytes).into_owned()
    }

    /// Currently-resident bytes. D52 reads this to surface
    /// `log_bytes` in the diagnostics snapshot; the test suite uses it
    /// to assert overflow + cap behaviour.
    pub fn len(&self) -> usize {
        self.data.len()
    }

    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }
}
