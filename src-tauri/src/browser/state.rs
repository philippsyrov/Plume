//! Process-owned visible state for the single sandbox browser window.

use std::collections::HashSet;
use std::sync::Mutex;

use serde::Serialize;

use crate::error::IpcError;

pub const BROWSER_SANDBOX_LABEL: &str = "browser-sandbox";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum BrowserNavigationFailureReason {
    NavigationFailed,
    LoopbackApprovalRequired,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserNavigationFailure {
    pub reason: BrowserNavigationFailureReason,
    pub message: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserSandboxState {
    pub open: bool,
    pub window_label: Option<String>,
    pub requested_url: Option<String>,
    pub current_url: Option<String>,
    pub title: Option<String>,
    pub loading: bool,
    pub failure: Option<BrowserNavigationFailure>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BrowserCaptureTicket {
    pub generation: u64,
    pub current_url: String,
}

#[derive(Default)]
struct BrowserSandboxStoreInner {
    state: BrowserSandboxState,
    window_generation: u64,
    page_generation: u64,
    navigation_admitted: bool,
    approved_loopback_origins: HashSet<String>,
}

#[derive(Default)]
pub struct BrowserSandboxStore {
    inner: Mutex<BrowserSandboxStoreInner>,
    operation: Mutex<()>,
}

impl BrowserSandboxStore {
    pub fn snapshot(&self) -> BrowserSandboxState {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .state
            .clone()
    }

    pub fn capture_ticket(&self) -> Result<BrowserCaptureTicket, IpcError> {
        let inner = self.lock_inner();
        if !inner.state.open {
            return Err(IpcError::NotFound("browser.sandboxWindow".into()));
        }
        if inner.state.loading {
            return Err(IpcError::Blocked("browser.captureWhileLoading".into()));
        }
        if inner.state.failure.is_some() {
            return Err(IpcError::Blocked("browser.captureUnavailable".into()));
        }
        let current_url = inner
            .state
            .current_url
            .clone()
            .ok_or_else(|| IpcError::Blocked("browser.captureUnavailable".into()))?;
        Ok(BrowserCaptureTicket {
            generation: inner.page_generation,
            current_url,
        })
    }

    pub fn capture_ticket_is_current(&self, ticket: &BrowserCaptureTicket) -> bool {
        let inner = self.lock_inner();
        inner.state.open
            && !inner.state.loading
            && inner.state.failure.is_none()
            && inner.page_generation == ticket.generation
            && inner.state.current_url.as_deref() == Some(ticket.current_url.as_str())
    }

    pub fn opening_new_window(&self, url: &tauri::Url) -> u64 {
        let mut inner = self.lock_inner();
        inner.window_generation = next_generation(inner.window_generation);
        inner.page_generation = next_generation(inner.page_generation);
        opening_state(&mut inner.state, url);
        inner.navigation_admitted = false;
        inner.window_generation
    }

    pub fn opening_existing_window(&self, url: &tauri::Url) -> u64 {
        let mut inner = self.lock_inner();
        if inner.window_generation == 0 {
            inner.window_generation = next_generation(inner.window_generation);
        }
        inner.page_generation = next_generation(inner.page_generation);
        opening_state(&mut inner.state, url);
        inner.navigation_admitted = false;
        inner.window_generation
    }

    pub fn admit_navigation(&self, generation: u64, url: &tauri::Url) -> bool {
        let mut inner = self.lock_inner();
        if inner.window_generation != generation || !inner.state.open {
            return false;
        }
        if inner.navigation_admitted
            && inner.state.loading
            && inner.state.current_url.as_deref() == Some(url.as_str())
        {
            return false;
        }

        inner.navigation_admitted = true;
        inner.page_generation = next_generation(inner.page_generation);
        inner.state.current_url = Some(url.as_str().to_string());
        inner.state.title = None;
        inner.state.loading = true;
        inner.state.failure = None;
        true
    }

    pub fn is_loading_url(&self, url: &tauri::Url) -> bool {
        let inner = self.lock_inner();
        inner.state.open
            && inner.state.loading
            && inner.state.current_url.as_deref() == Some(url.as_str())
    }

    pub fn navigation_finished(&self, generation: u64, url: &tauri::Url) {
        self.with_current_generation(generation, |state| {
            if state.current_url.as_deref() == Some(url.as_str()) {
                state.loading = false;
            }
        });
    }

    pub fn navigation_failed(&self, generation: u64, message: String) {
        self.with_current_generation(generation, |state| {
            state.loading = false;
            state.failure = Some(BrowserNavigationFailure {
                reason: BrowserNavigationFailureReason::NavigationFailed,
                message: bounded(message, 1_024),
            });
        });
    }

    pub fn loopback_approval_required(&self, generation: u64) {
        self.with_current_generation(generation, |state| {
            state.loading = false;
            state.failure = Some(BrowserNavigationFailure {
                reason: BrowserNavigationFailureReason::LoopbackApprovalRequired,
                message: "browser.loopbackApprovalRequired".into(),
            });
        });
    }

    pub fn approve_loopback_origin(&self, origin: &str) {
        self.lock_inner()
            .approved_loopback_origins
            .insert(origin.to_string());
    }

    pub fn is_loopback_origin_approved(&self, origin: &str) -> bool {
        self.lock_inner().approved_loopback_origins.contains(origin)
    }

    pub fn closed(&self) {
        let mut inner = self.lock_inner();
        inner.state = BrowserSandboxState::default();
        inner.navigation_admitted = false;
        inner.approved_loopback_origins.clear();
    }

    pub fn closed_if_generation(&self, generation: u64) {
        let mut inner = self.lock_inner();
        if inner.window_generation == generation {
            inner.state = BrowserSandboxState::default();
            inner.navigation_admitted = false;
            inner.approved_loopback_origins.clear();
        }
    }

    pub fn run_exclusive<T>(&self, action: impl FnOnce() -> T) -> T {
        let _operation = self
            .operation
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        action()
    }

    fn with_current_generation(
        &self,
        generation: u64,
        mutate: impl FnOnce(&mut BrowserSandboxState),
    ) {
        let mut inner = self.lock_inner();
        if inner.window_generation == generation && inner.state.open {
            mutate(&mut inner.state);
        }
    }

    fn lock_inner(&self) -> std::sync::MutexGuard<'_, BrowserSandboxStoreInner> {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

fn opening_state(state: &mut BrowserSandboxState, url: &tauri::Url) {
    let url = url.as_str().to_string();
    state.open = true;
    state.window_label = Some(BROWSER_SANDBOX_LABEL.to_string());
    state.requested_url = Some(url.clone());
    state.current_url = Some(url);
    state.title = None;
    state.loading = true;
    state.failure = None;
}

fn next_generation(current: u64) -> u64 {
    current.wrapping_add(1).max(1)
}

fn bounded(value: String, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Barrier};
    use std::thread;
    use std::time::Duration;

    use super::{BrowserNavigationFailureReason, BrowserSandboxStore, BROWSER_SANDBOX_LABEL};

    fn url(raw: &str) -> tauri::Url {
        tauri::Url::parse(raw).unwrap()
    }

    #[test]
    fn initial_and_closed_state_contain_no_stale_page_data() {
        let store = BrowserSandboxStore::default();
        assert!(!store.snapshot().open);

        let generation = store.opening_new_window(&url("https://example.com/secret"));
        store.navigation_failed(generation, "private failure details".to_string());
        store.closed();

        assert_eq!(store.snapshot(), Default::default());
        store.closed();
        assert_eq!(store.snapshot(), Default::default());
    }

    #[test]
    fn opening_records_one_visible_loading_window() {
        let store = BrowserSandboxStore::default();
        store.opening_new_window(&url("http://localhost:5173/"));

        let state = store.snapshot();
        assert!(state.open);
        assert_eq!(state.window_label.as_deref(), Some(BROWSER_SANDBOX_LABEL));
        assert_eq!(
            state.requested_url.as_deref(),
            Some("http://localhost:5173/")
        );
        assert_eq!(state.current_url, state.requested_url);
        assert!(state.loading);
        assert!(state.failure.is_none());
    }

    #[test]
    fn navigation_callbacks_update_only_visible_page_state() {
        let store = BrowserSandboxStore::default();
        let generation = store.opening_new_window(&url("https://example.com/"));
        assert!(store.admit_navigation(generation, &url("https://example.com/next")));

        let loading = store.snapshot();
        assert_eq!(
            loading.current_url.as_deref(),
            Some("https://example.com/next")
        );
        assert!(loading.title.is_none());
        assert!(loading.loading);

        store.navigation_finished(generation, &url("https://example.com/next"));
        assert!(!store.snapshot().loading);
    }

    #[test]
    fn failure_is_typed_and_the_next_open_clears_it() {
        let store = BrowserSandboxStore::default();
        let generation = store.opening_new_window(&url("https://example.com/"));
        store.navigation_failed(generation, "network down".to_string());

        let failed = store.snapshot();
        assert!(!failed.loading);
        assert_eq!(
            failed.failure.unwrap().reason,
            BrowserNavigationFailureReason::NavigationFailed
        );

        store.opening_existing_window(&url("https://example.org/"));
        assert!(store.snapshot().failure.is_none());
    }

    #[test]
    fn hostile_errors_are_bounded_by_character_count() {
        let store = BrowserSandboxStore::default();
        let generation = store.opening_new_window(&url("https://example.com/"));
        store.navigation_failed(generation, "🔥".repeat(2_000));

        let state = store.snapshot();
        assert!(state.title.is_none());
        assert!(state.failure.unwrap().message.chars().count() <= 1_024);
    }

    #[test]
    fn state_serializes_with_camel_case_wire_fields() {
        let store = BrowserSandboxStore::default();
        store.opening_new_window(&url("https://example.com/"));
        let value = serde_json::to_value(store.snapshot()).unwrap();

        assert_eq!(value["windowLabel"], BROWSER_SANDBOX_LABEL);
        assert_eq!(value["requestedUrl"], "https://example.com/");
        assert!(value.get("window_label").is_none());
    }

    #[test]
    fn lifecycle_operations_are_serialized() {
        let store = Arc::new(BrowserSandboxStore::default());
        let barrier = Arc::new(Barrier::new(3));
        let active = Arc::new(AtomicUsize::new(0));
        let max_active = Arc::new(AtomicUsize::new(0));
        let mut workers = Vec::new();

        for _ in 0..2 {
            let store = Arc::clone(&store);
            let barrier = Arc::clone(&barrier);
            let active = Arc::clone(&active);
            let max_active = Arc::clone(&max_active);
            workers.push(thread::spawn(move || {
                barrier.wait();
                store.run_exclusive(|| {
                    let now = active.fetch_add(1, Ordering::SeqCst) + 1;
                    max_active.fetch_max(now, Ordering::SeqCst);
                    thread::sleep(Duration::from_millis(20));
                    active.fetch_sub(1, Ordering::SeqCst);
                });
            }));
        }

        barrier.wait();
        for worker in workers {
            worker.join().unwrap();
        }
        assert_eq!(max_active.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn stale_window_callbacks_cannot_clear_a_reopened_window() {
        let store = BrowserSandboxStore::default();
        let old_generation = store.opening_new_window(&url("https://old.example/"));
        store.closed_if_generation(old_generation);

        let new_generation = store.opening_new_window(&url("https://new.example/"));
        assert_ne!(new_generation, old_generation);
        store.closed_if_generation(old_generation);

        let state = store.snapshot();
        assert!(state.open);
        assert_eq!(state.current_url.as_deref(), Some("https://new.example/"));
    }

    #[test]
    fn stale_navigation_callbacks_cannot_overwrite_the_newer_page() {
        let store = BrowserSandboxStore::default();
        let generation = store.opening_new_window(&url("https://old.example/"));
        assert!(store.admit_navigation(generation, &url("https://new.example/")));

        store.navigation_finished(generation, &url("https://old.example/"));

        let loading = store.snapshot();
        assert!(loading.loading);
        assert_eq!(loading.current_url.as_deref(), Some("https://new.example/"));
        assert!(loading.title.is_none());

        store.navigation_finished(generation, &url("https://new.example/"));
        let finished = store.snapshot();
        assert!(!finished.loading);
        assert!(finished.title.is_none());
    }

    #[test]
    fn overlapping_same_url_navigation_is_denied_until_the_page_finishes() {
        let store = BrowserSandboxStore::default();
        let generation = store.opening_new_window(&url("https://example.com/"));

        assert!(store.admit_navigation(generation, &url("https://example.com/")));
        assert!(store.is_loading_url(&url("https://example.com/")));
        assert!(!store.admit_navigation(generation, &url("https://example.com/")));

        store.navigation_finished(generation, &url("https://example.com/"));
        assert!(store.admit_navigation(generation, &url("https://example.com/")));
    }

    #[test]
    fn loopback_approval_is_exact_and_lives_only_for_the_window_session() {
        let store = BrowserSandboxStore::default();
        store.opening_new_window(&url("https://example.com/"));

        store.approve_loopback_origin("http://localhost:5173");
        assert!(store.is_loopback_origin_approved("http://localhost:5173"));
        assert!(!store.is_loopback_origin_approved("http://localhost:5174"));

        store.closed();
        assert!(!store.is_loopback_origin_approved("http://localhost:5173"));
    }

    #[test]
    fn stale_destroy_does_not_clear_the_current_window_approvals() {
        let store = BrowserSandboxStore::default();
        let old_generation = store.opening_new_window(&url("https://old.example/"));
        store.closed_if_generation(old_generation);

        let new_generation = store.opening_new_window(&url("https://new.example/"));
        store.approve_loopback_origin("http://localhost:5173");
        store.closed_if_generation(old_generation);

        assert_ne!(new_generation, old_generation);
        assert!(store.is_loopback_origin_approved("http://localhost:5173"));
    }

    #[test]
    fn capture_ticket_requires_one_finished_current_page() {
        let store = BrowserSandboxStore::default();
        assert!(store.capture_ticket().is_err());

        let generation = store.opening_new_window(&url("https://example.com/"));
        assert!(store.capture_ticket().is_err());
        assert!(store.admit_navigation(generation, &url("https://example.com/")));
        store.navigation_finished(generation, &url("https://example.com/"));

        let ticket = store.capture_ticket().unwrap();
        assert_eq!(ticket.current_url, "https://example.com/");
        assert!(store.capture_ticket_is_current(&ticket));
    }

    #[test]
    fn navigation_or_window_replacement_invalidates_capture_ticket() {
        let store = BrowserSandboxStore::default();
        let generation = store.opening_new_window(&url("https://example.com/"));
        assert!(store.admit_navigation(generation, &url("https://example.com/")));
        store.navigation_finished(generation, &url("https://example.com/"));
        let ticket = store.capture_ticket().unwrap();
        assert!(store.capture_ticket_is_current(&ticket));

        assert!(store.admit_navigation(generation, &url("https://example.com/next")));
        assert!(!store.capture_ticket_is_current(&ticket));
        store.navigation_finished(generation, &url("https://example.com/next"));
        assert!(store.admit_navigation(generation, &url("https://example.com/")));
        store.navigation_finished(generation, &url("https://example.com/"));
        assert!(!store.capture_ticket_is_current(&ticket));
        store.closed();
        assert!(!store.capture_ticket_is_current(&ticket));
    }
}
