//! D7.1 stream registry: maps `ChatStreamId` → cancel flag.
//!
//! Cancellation is cooperative. The streaming adapter
//! (`ollama::stream_chat`) checks this flag between NDJSON line
//! reads. When `chat.cancel(id)` flips it, the next loop iteration
//! breaks out and the command emits a terminal `chat/done` event
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
    inner: Mutex<ChatStreamState>,
}

#[derive(Default)]
struct ChatStreamState {
    generation: u64,
    streams: HashMap<String, Arc<AtomicBool>>,
}

pub(crate) enum ChatStreamRegistration {
    Registered(Arc<AtomicBool>),
    Duplicate,
    StaleGeneration,
}

impl ChatStreamRegistry {
    /// Snapshot the project lifecycle generation before prompt context is read.
    pub(crate) fn generation(&self) -> u64 {
        self.inner
            .lock()
            .expect("chat stream registry poisoned")
            .generation
    }

    /// Reserve an entry for `id` with a fresh cancel flag.
    ///
    /// Returns `Some(flag)` if the id was free; `None` if another
    /// stream is already in flight with the same id. The caller
    /// rejects the second registration with `BadArgument` rather
    /// than silently overwriting — duplicate ids would let two
    /// streams race against the same cancel flag and confuse the
    /// event filtering on the frontend.
    ///
    /// Panics on lock poisoning — these are tiny, non-IO-touching
    /// sections.
    pub fn register(&self, id: String) -> Option<Arc<AtomicBool>> {
        let flag = Arc::new(AtomicBool::new(false));
        let mut guard = self.inner.lock().expect("chat stream registry poisoned");
        if guard.streams.contains_key(&id) {
            return None;
        }
        guard.streams.insert(id, flag.clone());
        Some(flag)
    }

    /// Register only if no project lifecycle transition completed since
    /// preflight began. The generation check and insertion share the same lock
    /// project transitions hold through identity mutation.
    pub(crate) fn register_for_generation(
        &self,
        id: String,
        expected_generation: u64,
    ) -> ChatStreamRegistration {
        let flag = Arc::new(AtomicBool::new(false));
        let mut guard = self.inner.lock().expect("chat stream registry poisoned");
        if guard.generation != expected_generation {
            return ChatStreamRegistration::StaleGeneration;
        }
        if guard.streams.contains_key(&id) {
            return ChatStreamRegistration::Duplicate;
        }
        guard.streams.insert(id, flag.clone());
        ChatStreamRegistration::Registered(flag)
    }

    /// Mark the stream as cancelled. Returns whether an entry was
    /// found — the IPC handler uses this to distinguish "we
    /// cancelled a live stream" from "id is unknown or already
    /// terminal" (the second is silent / idempotent per the contract).
    pub fn cancel(&self, id: &str) -> bool {
        let guard = self.inner.lock().expect("chat stream registry poisoned");
        if let Some(flag) = guard.streams.get(id) {
            flag.store(true, Ordering::SeqCst);
            true
        } else {
            false
        }
    }

    /// Mark every in-flight stream as cancelled without removing its entry.
    /// Each streaming task still owns terminal cleanup and emits its normal
    /// cancelled completion event before calling `finish`.
    pub fn cancel_all(&self) {
        let guard = self.inner.lock().expect("chat stream registry poisoned");
        for flag in guard.streams.values() {
            flag.store(true, Ordering::SeqCst);
        }
    }

    /// Cancel existing streams, advance the admission generation, and keep
    /// late registration blocked until the caller finishes changing project
    /// identity. The lock is released before any provider stream runs.
    pub(crate) fn cancel_all_and_transition<T>(&self, transition: impl FnOnce() -> T) -> T {
        let mut guard = self.inner.lock().expect("chat stream registry poisoned");
        for flag in guard.streams.values() {
            flag.store(true, Ordering::SeqCst);
        }
        guard.generation = guard.generation.wrapping_add(1);
        let result = transition();
        drop(guard);
        result
    }

    /// Drop the entry. Called from the streaming task on terminal
    /// exit (done / cancelled / error). After this, `cancel(id)` is
    /// a no-op.
    pub fn finish(&self, id: &str) {
        let mut guard = self.inner.lock().expect("chat stream registry poisoned");
        guard.streams.remove(id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_cancel_finish_round_trip() {
        let reg = ChatStreamRegistry::default();
        let flag = reg
            .register("abc".into())
            .expect("fresh id should register");
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

    #[test]
    fn register_rejects_duplicate_in_flight_id() {
        // Client-minted ids should be unique per session; a buggy
        // or malicious caller that sends two concurrent registers
        // for the same id must be rejected so the cancel flag stays
        // 1:1 with a live stream.
        let reg = ChatStreamRegistry::default();
        assert!(reg.register("dup".into()).is_some());
        assert!(
            reg.register("dup".into()).is_none(),
            "second register on a live id must fail"
        );
        // After finish, the id is reusable.
        reg.finish("dup");
        assert!(reg.register("dup".into()).is_some());
    }

    #[test]
    fn cancel_all_marks_every_registered_stream() {
        let reg = ChatStreamRegistry::default();
        let first = reg.register("first".into()).expect("first stream");
        let second = reg.register("second".into()).expect("second stream");

        reg.cancel_all();

        assert!(first.load(Ordering::SeqCst));
        assert!(second.load(Ordering::SeqCst));
    }

    #[test]
    fn transition_rejects_a_send_paused_before_registration() {
        let reg = Arc::new(ChatStreamRegistry::default());
        let generation = reg.generation();
        let (paused_tx, paused_rx) = std::sync::mpsc::sync_channel(1);
        let (resume_tx, resume_rx) = std::sync::mpsc::sync_channel(1);
        let send_registry = reg.clone();
        let send = std::thread::spawn(move || {
            paused_tx.send(()).expect("pause send");
            resume_rx.recv().expect("resume send");
            send_registry.register_for_generation("late".into(), generation)
        });

        paused_rx.recv().expect("send reached admission boundary");
        reg.cancel_all_and_transition(|| ());
        resume_tx.send(()).expect("release send");

        assert!(matches!(
            send.join().expect("send thread"),
            ChatStreamRegistration::StaleGeneration,
        ));
        assert!(!reg.cancel("late"));
        let current = reg.generation();
        let flag = match reg.register_for_generation("current".into(), current) {
            ChatStreamRegistration::Registered(flag) => flag,
            _ => panic!("current generation should register"),
        };
        assert!(!flag.load(Ordering::SeqCst));
    }
}
