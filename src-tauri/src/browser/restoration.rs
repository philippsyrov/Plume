//! Safe, bounded top-level URL restoration for persisted Browser tabs.
//!
//! Plume restores its own admitted URL descriptors, not DOM state,
//! form values, JavaScript heaps, or WebKit's private back-forward list.

use super::policy::{contains_secret_shape, validate_browser_url, BrowserUrlError};

pub(super) const HISTORY_CAP: usize = 20;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct RestorableUrl {
    pub value: String,
    pub manual_reopen_required: bool,
}

impl RestorableUrl {
    pub(super) fn safe(value: impl Into<String>) -> Self {
        Self {
            value: value.into(),
            manual_reopen_required: false,
        }
    }
}

/// Admit a URL for persistence without ever returning a secret-bearing
/// value. Safe URLs retain ordinary query/fragment state. A secret in
/// query or fragment reduces the value to origin/path; a secret in the
/// path reduces it to origin; and a secret-shaped host uses a harmless
/// invalid placeholder. Every reduction asks the user to reopen rather
/// than pretending the sanitized URL is an exact restoration.
pub(super) fn admit_restorable_url(raw: &str) -> Result<RestorableUrl, BrowserUrlError> {
    let validated = validate_browser_url(raw)?;
    let mut url = validated.url;
    let host = url.host_str().ok_or(BrowserUrlError::InvalidUrl)?;

    if contains_secret_shape(host) {
        return Ok(RestorableUrl {
            value: format!("{}://redacted.invalid/", url.scheme()),
            manual_reopen_required: true,
        });
    }
    if contains_secret_shape(url.path()) {
        return Ok(RestorableUrl {
            value: format!("{}/", url.origin().ascii_serialization()),
            manual_reopen_required: true,
        });
    }
    let unsafe_tail = url.query().is_some_and(contains_secret_shape)
        || url.fragment().is_some_and(contains_secret_shape);
    if unsafe_tail {
        url.set_query(None);
        url.set_fragment(None);
        return Ok(RestorableUrl {
            value: url.as_str().to_string(),
            manual_reopen_required: true,
        });
    }

    Ok(RestorableUrl::safe(url.as_str()))
}

/// Append a top-level navigation to Plume's own bounded history.
/// Navigating after Back first discards the unreachable forward tail;
/// exceeding the cap then removes the oldest entries deterministically.
pub(super) fn append_history(
    mut history: Vec<RestorableUrl>,
    current_index: usize,
    admitted: RestorableUrl,
) -> (Vec<RestorableUrl>, usize) {
    if !history.is_empty() {
        history.truncate(current_index.saturating_add(1).min(history.len()));
    }
    history.push(admitted);
    if history.len() > HISTORY_CAP {
        let overflow = history.len() - HISTORY_CAP;
        history.drain(..overflow);
    }
    let current_index = history.len() - 1;
    (history, current_index)
}
