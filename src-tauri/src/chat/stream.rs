//! D7.1 stream registry: maps `ChatStreamId` → cancel flag.
//!
//! Cancellation is cooperative. The streaming adapter
//! (`ollama::stream_chat`) checks this flag between NDJSON line
//! reads. When `chat.cancel(id)` flips it, the next loop iteration
//! breaks out and the command emits a terminal `chat.done` event
//! with `finish: 'cancelled'`. The actual blocking HTTP read of the
//! next line can still buffer one more frame after cancellation —
//! the limitation is documented in `docs/IPC_CONTRACT.md § chat`.
//!
//! The registry is shared app state managed by Tauri and accessed
//! from both `chat_send` (which inserts before spawning the task)
//! and `chat_cancel` (which sets the flag). The task that runs the
//! stream owns its own `Arc` and removes the entry from the
//! registry on terminal exit.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// Process-wide registry of in-flight chat streams. Lives inside
/// `AppState` so handlers can reach it through `tauri::State`.
#[derive(Default)]
pub struct ChatStreamRegistry {
    inner: Mutex<HashMap<String, Arc<AtomicBool>>>,
}

impl ChatStreamRegistry {
    /// Reserve an entry for `id` with a fresh cancel flag and return
    /// an `Arc` to it. Caller hands this `Arc` to the spawned
    /// streaming task. Panics on lock poisoning — these are tiny,
    /// non-IO-touching sections.
    pub fn register(&self, id: String) -> Arc<AtomicBool> {
        let flag = Arc::new(AtomicBool::new(false));
        let mut guard = self.inner.lock().expect("chat stream registry poisoned");
        guard.insert(id, flag.clone());
        flag
    }

    /// Mark the stream as cancelled. Returns whether an entry was
    /// found — the IPC handler uses this to distinguish "we
    /// cancelled a live stream" from "id is unknown or already
    /// terminal" (the second is silent / idempotent per the contract).
    pub fn cancel(&self, id: &str) -> bool {
        let guard = self.inner.lock().expect("chat stream registry poisoned");
        if let Some(flag) = guard.get(id) {
            flag.store(true, Ordering::SeqCst);
            true
        } else {
            false
        }
    }

    /// Drop the entry. Called from the streaming task on terminal
    /// exit (done / cancelled / error). After this, `cancel(id)` is
    /// a no-op.
    pub fn finish(&self, id: &str) {
        let mut guard = self.inner.lock().expect("chat stream registry poisoned");
        guard.remove(id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_cancel_finish_round_trip() {
        let reg = ChatStreamRegistry::default();
        let flag = reg.register("abc".into());
        assert!(!flag.load(Ordering::SeqCst));
        assert!(reg.cancel("abc"));
        assert!(flag.load(Ordering::SeqCst));
        reg.finish("abc");
        // Second cancel is a no-op (idempotent per the contract).
        assert!(!reg.cancel("abc"));
    }

    #[test]
    fn cancel_unknown_id_returns_false() {
        let reg = ChatStreamRegistry::default();
        assert!(!reg.cancel("never-registered"));
    }

    #[test]
    fn finish_unknown_id_is_idempotent() {
        let reg = ChatStreamRegistry::default();
        reg.finish("never-registered");
        // No panic, no leak.
    }
}
