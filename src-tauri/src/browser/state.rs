//! Process-owned visible state for the single sandbox browser window.

use std::sync::Mutex;

use serde::Serialize;

pub const BROWSER_SANDBOX_LABEL: &str = "browser-sandbox";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum BrowserNavigationFailureReason {
    NavigationFailed,
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

#[derive(Default)]
pub struct BrowserSandboxStore {
    inner: Mutex<BrowserSandboxState>,
}

impl BrowserSandboxStore {
    pub fn snapshot(&self) -> BrowserSandboxState {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    pub fn opening(&self, url: &tauri::Url) {
        self.with_state(|state| {
            let url = url.as_str().to_string();
            state.open = true;
            state.window_label = Some(BROWSER_SANDBOX_LABEL.to_string());
            state.requested_url = Some(url.clone());
            state.current_url = Some(url);
            state.title = None;
            state.loading = true;
            state.failure = None;
        });
    }

    pub fn navigation_started(&self, url: &tauri::Url) {
        self.with_state(|state| {
            state.current_url = Some(url.as_str().to_string());
            state.loading = true;
            state.failure = None;
        });
    }

    pub fn navigation_finished(&self, url: &tauri::Url) {
        self.with_state(|state| {
            state.current_url = Some(url.as_str().to_string());
            state.loading = false;
        });
    }

    pub fn title_changed(&self, title: String) {
        self.with_state(|state| state.title = Some(bounded(title, 512)));
    }

    pub fn navigation_failed(&self, message: String) {
        self.with_state(|state| {
            state.loading = false;
            state.failure = Some(BrowserNavigationFailure {
                reason: BrowserNavigationFailureReason::NavigationFailed,
                message: bounded(message, 1_024),
            });
        });
    }

    pub fn closed(&self) {
        self.with_state(|state| *state = BrowserSandboxState::default());
    }

    fn with_state(&self, mutate: impl FnOnce(&mut BrowserSandboxState)) {
        let mut state = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        mutate(&mut state);
    }
}

fn bounded(value: String, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}

#[cfg(test)]
mod tests {
    use super::{BrowserNavigationFailureReason, BrowserSandboxStore, BROWSER_SANDBOX_LABEL};

    fn url(raw: &str) -> tauri::Url {
        tauri::Url::parse(raw).unwrap()
    }

    #[test]
    fn initial_and_closed_state_contain_no_stale_page_data() {
        let store = BrowserSandboxStore::default();
        assert!(!store.snapshot().open);

        store.opening(&url("https://example.com/secret"));
        store.title_changed("Sensitive page title".to_string());
        store.navigation_failed("private failure details".to_string());
        store.closed();

        assert_eq!(store.snapshot(), Default::default());
        store.closed();
        assert_eq!(store.snapshot(), Default::default());
    }

    #[test]
    fn opening_records_one_visible_loading_window() {
        let store = BrowserSandboxStore::default();
        store.opening(&url("http://localhost:5173/"));

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
        store.opening(&url("https://example.com/"));
        store.navigation_started(&url("https://example.com/next"));
        store.title_changed("Next page".to_string());

        let loading = store.snapshot();
        assert_eq!(
            loading.current_url.as_deref(),
            Some("https://example.com/next")
        );
        assert_eq!(loading.title.as_deref(), Some("Next page"));
        assert!(loading.loading);

        store.navigation_finished(&url("https://example.com/next"));
        assert!(!store.snapshot().loading);
    }

    #[test]
    fn failure_is_typed_and_the_next_open_clears_it() {
        let store = BrowserSandboxStore::default();
        store.opening(&url("https://example.com/"));
        store.navigation_failed("network down".to_string());

        let failed = store.snapshot();
        assert!(!failed.loading);
        assert_eq!(
            failed.failure.unwrap().reason,
            BrowserNavigationFailureReason::NavigationFailed
        );

        store.opening(&url("https://example.org/"));
        assert!(store.snapshot().failure.is_none());
    }

    #[test]
    fn hostile_titles_and_errors_are_bounded_by_character_count() {
        let store = BrowserSandboxStore::default();
        store.opening(&url("https://example.com/"));
        store.title_changed("🪶".repeat(800));
        store.navigation_failed("🔥".repeat(2_000));

        let state = store.snapshot();
        assert!(state.title.unwrap().chars().count() <= 512);
        assert!(state.failure.unwrap().message.chars().count() <= 1_024);
    }

    #[test]
    fn state_serializes_with_camel_case_wire_fields() {
        let store = BrowserSandboxStore::default();
        store.opening(&url("https://example.com/"));
        let value = serde_json::to_value(store.snapshot()).unwrap();

        assert_eq!(value["windowLabel"], BROWSER_SANDBOX_LABEL);
        assert_eq!(value["requestedUrl"], "https://example.com/");
        assert!(value.get("window_label").is_none());
    }
}
