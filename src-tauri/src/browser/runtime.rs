//! Native child-WebView seam for the session-owned task Browser.
//!
//! React owns layout descriptors. This module owns native child lifetime and
//! keeps Tauri-specific calls behind `BrowserRuntimePort`, so identity,
//! visibility, and geometry rules remain unit-testable without a real window.

use sha2::{Digest, Sha256};
use tauri::webview::{NewWindowResponse, WebviewBuilder};
use tauri::{AppHandle, LogicalPosition, LogicalSize, Manager, WebviewUrl};

use crate::browser::policy::{allow_download, allow_popup, validate_browser_url};
use crate::sessions::browser_workspace::BrowserWorkspaceScope;

const MAIN_WINDOW_LABEL: &str = "main";
const MAX_NATIVE_TABS: usize = 5;
const MAX_GEOMETRY_PX: f64 = 20_000.0;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct BrowserRuntimeIdentity {
    pub scope: BrowserWorkspaceScope,
    pub session_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct LiveTabIdentity {
    pub workspace: BrowserRuntimeIdentity,
    pub tab_id: String,
    pub generation: u64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct BrowserBounds {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl BrowserBounds {
    pub(crate) fn new(
        x: f64,
        y: f64,
        width: f64,
        height: f64,
    ) -> Result<Self, BrowserRuntimeError> {
        let values = [x, y, width, height];
        if values.iter().any(|value| !value.is_finite())
            || x < 0.0
            || y < 0.0
            || width <= 0.0
            || height <= 0.0
            || values.iter().any(|value| *value > MAX_GEOMETRY_PX)
        {
            return Err(BrowserRuntimeError::InvalidBounds);
        }
        Ok(Self {
            x,
            y,
            width,
            height,
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct BrowserChildPlan {
    pub identity: LiveTabIdentity,
    pub label: String,
    pub parent_window_label: &'static str,
    pub initial_url: tauri::Url,
    pub bounds: BrowserBounds,
    pub visible: bool,
    pub persistent_data_store: bool,
    pub devtools: bool,
    pub extensions: bool,
    pub autofill: bool,
    pub allow_popups: bool,
    pub allow_downloads: bool,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub(crate) enum BrowserRuntimeError {
    #[error("browser bounds are invalid")]
    InvalidBounds,
    #[error("browser tab limit exceeded")]
    TabLimit,
    #[error("active browser tab was not found")]
    ActiveTabMissing,
    #[error("main window is unavailable")]
    MainWindowMissing,
    #[error("native browser operation failed: {0}")]
    Native(String),
}

pub(crate) trait BrowserRuntimePort: Send + Sync {
    fn add_child(&self, plan: &BrowserChildPlan) -> Result<(), BrowserRuntimeError>;
    fn set_bounds(&self, label: &str, bounds: BrowserBounds) -> Result<(), BrowserRuntimeError>;
    fn set_visible(&self, label: &str, visible: bool) -> Result<(), BrowserRuntimeError>;
    fn eval(&self, label: &str, script: &str) -> Result<(), BrowserRuntimeError>;
    fn reload(&self, label: &str) -> Result<(), BrowserRuntimeError>;
    fn close(&self, label: &str) -> Result<(), BrowserRuntimeError>;
}

pub(crate) struct BrowserRuntimeManager<P> {
    port: P,
}

impl<P: BrowserRuntimePort> BrowserRuntimeManager<P> {
    pub(crate) fn new(port: P) -> Self {
        Self { port }
    }

    #[cfg(test)]
    pub(crate) fn port(&self) -> &P {
        &self.port
    }

    pub(crate) fn child_label(identity: &LiveTabIdentity) -> String {
        let scope = match identity.workspace.scope {
            BrowserWorkspaceScope::Local => "local",
            BrowserWorkspaceScope::Project => "project",
        };
        let mut hash = Sha256::new();
        hash.update(scope.as_bytes());
        hash.update([0]);
        hash.update(identity.workspace.session_id.as_bytes());
        hash.update([0]);
        hash.update(identity.tab_id.as_bytes());
        hash.update([0]);
        hash.update(identity.generation.to_le_bytes());
        let digest = format!("{:x}", hash.finalize());
        format!("task-browser-{}", &digest[..32])
    }

    pub(crate) fn plan_child(
        identity: LiveTabIdentity,
        initial_url: tauri::Url,
        bounds: BrowserBounds,
    ) -> BrowserChildPlan {
        BrowserChildPlan {
            label: Self::child_label(&identity),
            identity,
            parent_window_label: MAIN_WINDOW_LABEL,
            initial_url,
            bounds,
            visible: false,
            persistent_data_store: true,
            devtools: false,
            extensions: false,
            autofill: false,
            allow_popups: false,
            allow_downloads: false,
        }
    }

    pub(crate) fn activate(
        &self,
        plans: Vec<BrowserChildPlan>,
        active_tab_id: &str,
    ) -> Result<(), BrowserRuntimeError> {
        if plans.len() > MAX_NATIVE_TABS {
            return Err(BrowserRuntimeError::TabLimit);
        }
        if !plans
            .iter()
            .any(|plan| plan.identity.tab_id == active_tab_id)
        {
            return Err(BrowserRuntimeError::ActiveTabMissing);
        }

        let mut created = Vec::with_capacity(plans.len());
        for plan in &plans {
            if let Err(error) = self.port.add_child(plan) {
                close_all(&self.port, &created);
                return Err(error);
            }
            created.push(plan.label.clone());
        }
        for plan in &plans {
            if let Err(error) = self
                .port
                .set_visible(&plan.label, plan.identity.tab_id == active_tab_id)
            {
                close_all(&self.port, &created);
                return Err(error);
            }
        }
        Ok(())
    }
}

fn close_all(port: &impl BrowserRuntimePort, labels: &[String]) {
    for label in labels.iter().rev() {
        let _ = port.close(label);
    }
}

#[derive(Clone)]
pub(crate) struct TauriBrowserRuntimePort {
    app: AppHandle,
}

impl TauriBrowserRuntimePort {
    pub(crate) fn new(app: AppHandle) -> Self {
        Self { app }
    }

    fn webview(&self, label: &str) -> Result<tauri::Webview, BrowserRuntimeError> {
        self.app
            .get_webview(label)
            .ok_or_else(|| BrowserRuntimeError::Native("browser.childMissing".into()))
    }
}

impl BrowserRuntimePort for TauriBrowserRuntimePort {
    fn add_child(&self, plan: &BrowserChildPlan) -> Result<(), BrowserRuntimeError> {
        if plan.parent_window_label != MAIN_WINDOW_LABEL {
            return Err(BrowserRuntimeError::MainWindowMissing);
        }
        let window = self
            .app
            .get_window(MAIN_WINDOW_LABEL)
            .ok_or(BrowserRuntimeError::MainWindowMissing)?;
        let builder = WebviewBuilder::new(
            plan.label.clone(),
            WebviewUrl::External(plan.initial_url.clone()),
        )
        .browser_extensions_enabled(plan.extensions)
        .general_autofill_enabled(plan.autofill)
        .devtools(plan.devtools)
        .on_navigation(|url| validate_browser_url(url.as_str()).is_ok())
        .on_new_window(|_, _| {
            debug_assert!(!allow_popup());
            NewWindowResponse::Deny
        })
        .on_download(|_, _| allow_download());

        let webview = window
            .add_child(
                builder,
                LogicalPosition::new(plan.bounds.x, plan.bounds.y),
                LogicalSize::new(plan.bounds.width, plan.bounds.height),
            )
            .map_err(|_| BrowserRuntimeError::Native("browser.childCreateFailed".into()))?;
        if !plan.visible {
            webview
                .hide()
                .map_err(|_| BrowserRuntimeError::Native("browser.childHideFailed".into()))?;
        }
        Ok(())
    }

    fn set_bounds(&self, label: &str, bounds: BrowserBounds) -> Result<(), BrowserRuntimeError> {
        let webview = self.webview(label)?;
        webview
            .set_position(LogicalPosition::new(bounds.x, bounds.y))
            .and_then(|_| webview.set_size(LogicalSize::new(bounds.width, bounds.height)))
            .map_err(|_| BrowserRuntimeError::Native("browser.childBoundsFailed".into()))
    }

    fn set_visible(&self, label: &str, visible: bool) -> Result<(), BrowserRuntimeError> {
        let webview = self.webview(label)?;
        let result = if visible {
            webview.show()
        } else {
            webview.hide()
        };
        result.map_err(|_| BrowserRuntimeError::Native("browser.childVisibilityFailed".into()))
    }

    fn eval(&self, label: &str, script: &str) -> Result<(), BrowserRuntimeError> {
        self.webview(label)?
            .eval(script)
            .map_err(|_| BrowserRuntimeError::Native("browser.childEvalFailed".into()))
    }

    fn reload(&self, label: &str) -> Result<(), BrowserRuntimeError> {
        self.webview(label)?
            .reload()
            .map_err(|_| BrowserRuntimeError::Native("browser.childReloadFailed".into()))
    }

    fn close(&self, label: &str) -> Result<(), BrowserRuntimeError> {
        self.webview(label)?
            .close()
            .map_err(|_| BrowserRuntimeError::Native("browser.childCloseFailed".into()))
    }
}
