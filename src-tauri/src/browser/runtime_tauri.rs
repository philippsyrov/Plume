//! Tauri/WebKit adapter for the session-owned Browser runtime.

use std::sync::mpsc::SyncSender;

use tauri::webview::{NewWindowResponse, PageLoadEvent, WebviewBuilder};
use tauri::{AppHandle, Manager, PhysicalPosition, PhysicalSize, WebviewUrl};

use super::{
    BrowserBounds, BrowserChildPlan, BrowserNavigationCommit, BrowserRuntimeError,
    BrowserRuntimeManager, BrowserRuntimePort, MAIN_WINDOW_LABEL,
};
use crate::browser::policy::{allow_download, allow_popup, validate_browser_url};
use crate::commands::project::AppState;
use crate::commands::sessions::{scope_dir, SessionScope};
use crate::sessions::browser_workspace::{commit_browser_navigation, BrowserWorkspaceScope};

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
        let navigation_app = self.app.clone();
        let navigation_label = plan.label.clone();
        let load_app = self.app.clone();
        let load_label = plan.label.clone();
        let builder = WebviewBuilder::new(
            plan.label.clone(),
            WebviewUrl::External(plan.initial_url.clone()),
        )
        .browser_extensions_enabled(plan.extensions)
        .general_autofill_enabled(plan.autofill)
        .devtools(plan.devtools)
        .on_navigation(move |url| {
            if url.as_str() == "about:blank" {
                return true;
            }
            let Ok(validated) = validate_browser_url(url.as_str()) else {
                return false;
            };
            navigation_app
                .state::<BrowserRuntimeManager<TauriBrowserRuntimePort>>()
                .admit_page_navigation(&navigation_label, &validated)
        })
        .on_new_window(|_, _| {
            debug_assert!(!allow_popup());
            NewWindowResponse::Deny
        })
        .on_download(|_, _| allow_download())
        .on_page_load(move |_, payload| {
            if matches!(payload.event(), PageLoadEvent::Finished) {
                let commit = load_app
                    .state::<BrowserRuntimeManager<TauriBrowserRuntimePort>>()
                    .navigation_finished(&load_label, payload.url().as_str());
                if let Some(commit) = commit {
                    persist_navigation(&load_app, commit);
                }
            }
        });

        let webview = window
            .add_child(
                builder,
                physical_position(plan.bounds),
                physical_size(plan.bounds),
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
            .set_position(physical_position(bounds))
            .and_then(|_| webview.set_size(physical_size(bounds)))
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

    fn eval_with_callback(
        &self,
        label: &str,
        script: &str,
        sender: SyncSender<String>,
    ) -> Result<(), BrowserRuntimeError> {
        self.webview(label)?
            .eval_with_callback(script, move |raw| {
                let _ = sender.send(raw);
            })
            .map_err(|_| BrowserRuntimeError::Native("browser.childEvalFailed".into()))
    }

    fn reload(&self, label: &str) -> Result<(), BrowserRuntimeError> {
        self.webview(label)?
            .reload()
            .map_err(|_| BrowserRuntimeError::Native("browser.childReloadFailed".into()))
    }

    fn navigate(&self, label: &str, url: &tauri::Url) -> Result<(), BrowserRuntimeError> {
        self.webview(label)?
            .navigate(url.clone())
            .map_err(|_| BrowserRuntimeError::Native("browser.childNavigateFailed".into()))
    }

    fn close(&self, label: &str) -> Result<(), BrowserRuntimeError> {
        let Some(webview) = self.app.get_webview(label) else {
            return Ok(());
        };
        webview
            .close()
            .map_err(|_| BrowserRuntimeError::Native("browser.childCloseFailed".into()))
    }
}

fn physical_position(bounds: BrowserBounds) -> PhysicalPosition<i32> {
    PhysicalPosition::new(bounds.x.round() as i32, bounds.y.round() as i32)
}

fn physical_size(bounds: BrowserBounds) -> PhysicalSize<u32> {
    PhysicalSize::new(bounds.width.round() as u32, bounds.height.round() as u32)
}

fn persist_navigation(app: &AppHandle, commit: BrowserNavigationCommit) {
    let state = app.state::<AppState>();
    let scope = match commit.workspace.scope {
        BrowserWorkspaceScope::Local => SessionScope::Local,
        BrowserWorkspaceScope::Project => SessionScope::Project,
    };
    let Ok(sessions_dir) = scope_dir(scope, &state) else {
        tracing::warn!("discarded Browser navigation after its scope became unavailable");
        return;
    };
    if commit_browser_navigation(
        &sessions_dir,
        &commit.workspace.session_id,
        commit.workspace.scope,
        &commit.tab_id,
        &commit.url,
        commit.navigation,
    )
    .is_err()
    {
        tracing::warn!("failed to persist an admitted Browser navigation");
    }
}
