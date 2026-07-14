//! Durable Browser-workspace domain records.
//!
//! Live WebKit views do not belong here. This module describes only the
//! bounded, restorable state owned by one persisted chat session. The
//! command layer supplies `scope`; physical local/project separation is
//! still enforced by choosing the correct session database.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use super::SessionStoreError;

pub(super) const MIN_SPLIT_WIDTH_PX: i64 = 320;
pub(super) const MAX_SPLIT_WIDTH_PX: i64 = 1_600;

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum BrowserWorkspaceScope {
    Local,
    Project,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum BrowserLayoutMode {
    Split,
    Expanded,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum BrowserRestorationStatus {
    Restorable,
    ManualReopenRequired,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum BrowserWorkspaceRecovery {
    BrowserStateReset,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BrowserHistoryRecord {
    pub position: usize,
    pub url: String,
    pub recorded_at_ms: i64,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BrowserTabRecord {
    pub id: String,
    pub position: usize,
    pub current_history_index: usize,
    pub manual_reopen_required: bool,
    pub restoration_status: BrowserRestorationStatus,
    pub history: Vec<BrowserHistoryRecord>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BrowserWorkspaceRecord {
    pub session_id: String,
    pub scope: BrowserWorkspaceScope,
    pub layout_mode: BrowserLayoutMode,
    pub split_width_px: i64,
    pub active_tab_id: Option<String>,
    pub tabs: Vec<BrowserTabRecord>,
    pub recovery: Option<BrowserWorkspaceRecovery>,
}

pub(super) fn validate_split_width_px(value: i64) -> Result<i64, SessionStoreError> {
    if (MIN_SPLIT_WIDTH_PX..=MAX_SPLIT_WIDTH_PX).contains(&value) {
        Ok(value)
    } else {
        Err(SessionStoreError::Invalid(format!(
            "browser split width must be between {MIN_SPLIT_WIDTH_PX} and {MAX_SPLIT_WIDTH_PX} pixels"
        )))
    }
}

pub(super) fn mint_workspace_id() -> String {
    mint_browser_id("bw_")
}

pub(super) fn mint_tab_id() -> String {
    mint_browser_id("bt_")
}

fn mint_browser_id(prefix: &str) -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos() as u64)
        .unwrap_or(0);
    format!("{prefix}{nanos:016x}{:08x}{n:08x}", std::process::id())
}
