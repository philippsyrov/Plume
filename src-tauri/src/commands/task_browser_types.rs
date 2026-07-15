use serde::{Deserialize, Serialize};

use crate::browser::evidence::{BrowserCaptureKind, BrowserEvidenceSummary};
use crate::browser::screenshot_evidence::BrowserScreenshotSummary;
use crate::commands::browser_workspace::SessionIdentity;
use crate::prompts::ContextSourceRef;

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TaskBrowserTabPayload {
    pub tab_id: String,
    pub url: Option<String>,
    pub manual_reopen_required: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TaskBrowserActivatePayload {
    pub identity: SessionIdentity,
    pub tabs: Vec<TaskBrowserTabPayload>,
    pub active_tab_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TaskBrowserIdentityPayload {
    pub identity: SessionIdentity,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TaskBrowserSuspensionPayload {
    pub identity: SessionIdentity,
    pub suspended: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TaskBrowserOpenTabPayload {
    pub identity: SessionIdentity,
    pub tab: TaskBrowserTabPayload,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TaskBrowserTabActionPayload {
    pub identity: SessionIdentity,
    pub tab_id: String,
    pub approved_loopback_origin: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TaskBrowserNavigatePayload {
    pub identity: SessionIdentity,
    pub tab_id: String,
    pub url: String,
    pub approved_loopback_origin: Option<String>,
    #[serde(default)]
    pub explicit_reopen: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BrowserHostRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub scale_factor: f64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TaskBrowserSetGeometryPayload {
    pub identity: SessionIdentity,
    pub host: BrowserHostRect,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TaskBrowserCaptureTextPayload {
    pub identity: SessionIdentity,
    pub tab_id: String,
    pub capture_kind: BrowserCaptureKind,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskBrowserCaptureTextResponse {
    pub evidence: BrowserEvidenceSummary,
    pub source: ContextSourceRef,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TaskBrowserCaptureScreenshotPayload {
    pub identity: SessionIdentity,
    pub tab_id: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskBrowserCaptureScreenshotResponse {
    pub evidence: BrowserScreenshotSummary,
    pub source: ContextSourceRef,
}
