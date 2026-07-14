use crate::browser::policy::{validate_browser_url, BrowserNetworkTarget};
use crate::error::IpcError;
use crate::sessions::browser_workspace::BrowserWorkspaceRecord;

use super::{SessionScope, TaskBrowserTabPayload};

pub(super) fn activation_tabs(record: &BrowserWorkspaceRecord) -> Vec<TaskBrowserTabPayload> {
    record
        .tabs
        .iter()
        .map(|tab| TaskBrowserTabPayload {
            tab_id: tab.id.clone(),
            url: tab
                .current_history_index
                .map(|index| tab.history[index].url.clone()),
            manual_reopen_required: tab.manual_reopen_required,
        })
        .collect()
}

pub(super) fn activation_tabs_match(
    record: &BrowserWorkspaceRecord,
    supplied: &[TaskBrowserTabPayload],
    scope: SessionScope,
) -> bool {
    let expected = activation_tabs(record);
    expected.len() == supplied.len()
        && expected.iter().zip(supplied).all(|(expected, supplied)| {
            if expected == supplied {
                return true;
            }
            expected.tab_id == supplied.tab_id
                && expected.url == supplied.url
                && !expected.manual_reopen_required
                && supplied.manual_reopen_required
                && scope == SessionScope::Project
                && supplied.url.as_deref().is_some_and(|url| {
                    validate_browser_url(url)
                        .is_ok_and(|validated| validated.target == BrowserNetworkTarget::Loopback)
                })
        })
}

pub(super) fn initial_url(tab: &TaskBrowserTabPayload) -> Result<tauri::Url, IpcError> {
    if tab.manual_reopen_required || tab.url.is_none() {
        return tauri::Url::parse("about:blank")
            .map_err(|_| IpcError::Internal("browser.blankUrlInvalid".into()));
    }
    validate_browser_url(tab.url.as_deref().expect("checked above"))
        .map(|validated| validated.url)
        .map_err(|_| IpcError::BadArgument("browser.invalidUrl".into()))
}
