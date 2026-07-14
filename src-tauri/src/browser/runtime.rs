//! Native child-WebView seam for the session-owned task Browser.
//!
//! React owns layout descriptors. This module owns native child lifetime and
//! keeps Tauri-specific calls behind `BrowserRuntimePort`, so identity,
//! visibility, and geometry rules remain unit-testable without a real window.

use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::sync::mpsc::SyncSender;
use std::sync::{Mutex, MutexGuard};

use crate::browser::policy::{loopback_origin, BrowserNetworkTarget, ValidatedBrowserUrl};
use crate::sessions::browser_workspace::{BrowserHistoryNavigation, BrowserWorkspaceScope};

const MAIN_WINDOW_LABEL: &str = "main";
pub(crate) const MAX_NATIVE_TABS: usize = 5;
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
    pub initial_navigation: BrowserHistoryNavigation,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub(crate) enum BrowserRuntimeError {
    #[error("browser bounds are invalid")]
    InvalidBounds,
    #[error("browser tab limit exceeded")]
    TabLimit,
    #[error("active browser tab was not found")]
    ActiveTabMissing,
    #[error("browser workspace is not selected")]
    WorkspaceNotSelected,
    #[error("browser child plan belongs to a different workspace")]
    WorkspaceMismatch,
    #[error("browser tab was not found")]
    TabNotFound,
    #[error("browser tab already exists")]
    TabAlreadyExists,
    #[error("main window is unavailable")]
    MainWindowMissing,
    #[error("native browser operation failed: {0}")]
    Native(String),
    #[error("browser page changed before capture completed")]
    CapturePageChanged,
}

pub(crate) trait BrowserRuntimePort: Send + Sync {
    fn add_child(&self, plan: &BrowserChildPlan) -> Result<(), BrowserRuntimeError>;
    fn set_bounds(&self, label: &str, bounds: BrowserBounds) -> Result<(), BrowserRuntimeError>;
    fn set_visible(&self, label: &str, visible: bool) -> Result<(), BrowserRuntimeError>;
    fn eval_with_callback(
        &self,
        label: &str,
        script: &str,
        sender: SyncSender<String>,
    ) -> Result<(), BrowserRuntimeError> {
        let _ = (label, script, sender);
        Err(BrowserRuntimeError::Native(
            "browser.captureUnsupported".into(),
        ))
    }
    fn reload(&self, label: &str) -> Result<(), BrowserRuntimeError>;
    fn navigate(&self, label: &str, url: &tauri::Url) -> Result<(), BrowserRuntimeError>;
    fn close(&self, label: &str) -> Result<(), BrowserRuntimeError>;
}

pub(crate) struct BrowserRuntimeManager<P> {
    port: P,
    state: Mutex<BrowserRuntimeState>,
}

#[derive(Default)]
struct BrowserRuntimeState {
    selected: Option<BrowserRuntimeIdentity>,
    tabs: Vec<LiveTab>,
    active_tab_id: Option<String>,
    revealed: bool,
}

#[derive(Clone)]
struct LiveTab {
    identity: LiveTabIdentity,
    label: String,
    page_generation: u64,
    current_url: Option<String>,
    approved_loopback_origins: HashSet<String>,
    pending_navigation: Option<BrowserHistoryNavigation>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct BrowserCaptureTicket {
    pub workspace: BrowserRuntimeIdentity,
    pub tab_id: String,
    pub label: String,
    pub page_generation: u64,
    pub current_url: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct BrowserNavigationCommit {
    pub workspace: BrowserRuntimeIdentity,
    pub tab_id: String,
    pub page_generation: u64,
    pub url: String,
    pub navigation: BrowserHistoryNavigation,
}

impl<P: BrowserRuntimePort> BrowserRuntimeManager<P> {
    pub(crate) fn new(port: P) -> Self {
        Self {
            port,
            state: Mutex::new(BrowserRuntimeState::default()),
        }
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
            initial_navigation: BrowserHistoryNavigation::Restore,
        }
    }

    pub(crate) fn plan_new_child(
        identity: LiveTabIdentity,
        initial_url: tauri::Url,
        bounds: BrowserBounds,
    ) -> BrowserChildPlan {
        let mut plan = Self::plan_child(identity, initial_url, bounds);
        plan.initial_navigation = BrowserHistoryNavigation::New;
        plan
    }

    pub(crate) fn activate(
        &self,
        plans: Vec<BrowserChildPlan>,
        active_tab_id: &str,
    ) -> Result<(), BrowserRuntimeError> {
        if plans.len() > MAX_NATIVE_TABS {
            return Err(BrowserRuntimeError::TabLimit);
        }
        let workspace = plans
            .first()
            .map(|plan| plan.identity.workspace.clone())
            .ok_or(BrowserRuntimeError::ActiveTabMissing)?;
        if plans
            .iter()
            .any(|plan| plan.identity.workspace != workspace)
        {
            return Err(BrowserRuntimeError::WorkspaceMismatch);
        }
        if !plans
            .iter()
            .any(|plan| plan.identity.tab_id == active_tab_id)
        {
            return Err(BrowserRuntimeError::ActiveTabMissing);
        }

        let mut state = self.lock_state();
        close_runtime_state(&self.port, &mut state)?;

        state.selected = Some(workspace);
        state.active_tab_id = Some(active_tab_id.to_string());
        state.tabs = plans
            .iter()
            .map(|plan| LiveTab {
                identity: plan.identity.clone(),
                label: plan.label.clone(),
                page_generation: 0,
                current_url: None,
                approved_loopback_origins: HashSet::new(),
                pending_navigation: (plan.initial_url.scheme() != "about")
                    .then_some(plan.initial_navigation),
            })
            .collect();

        let mut created = Vec::with_capacity(plans.len());
        for plan in &plans {
            if let Err(error) = self.port.add_child(plan) {
                close_all(&self.port, &created);
                *state = BrowserRuntimeState::default();
                return Err(error);
            }
            created.push(plan.label.clone());
            if let Err(error) = self.port.set_visible(&plan.label, false) {
                close_all(&self.port, &created);
                *state = BrowserRuntimeState::default();
                return Err(error);
            }
        }
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn selected_identity(&self) -> Option<BrowserRuntimeIdentity> {
        self.lock_state().selected.clone()
    }

    pub(crate) fn set_bounds(
        &self,
        workspace: &BrowserRuntimeIdentity,
        bounds: BrowserBounds,
    ) -> Result<(), BrowserRuntimeError> {
        let mut state = self.lock_selected(workspace)?;
        for tab in &state.tabs {
            self.port.set_bounds(&tab.label, bounds)?;
        }
        let active = state
            .active_tab_id
            .as_deref()
            .and_then(|tab_id| state.tabs.iter().find(|tab| tab.identity.tab_id == tab_id))
            .ok_or(BrowserRuntimeError::ActiveTabMissing)?;
        self.port.set_visible(&active.label, true)?;
        state.revealed = true;
        Ok(())
    }

    pub(crate) fn open_tab(
        &self,
        workspace: &BrowserRuntimeIdentity,
        plan: BrowserChildPlan,
        select: bool,
    ) -> Result<(), BrowserRuntimeError> {
        let mut state = self.lock_selected(workspace)?;
        if plan.identity.workspace != *workspace {
            return Err(BrowserRuntimeError::WorkspaceMismatch);
        }
        if state.tabs.len() >= MAX_NATIVE_TABS {
            return Err(BrowserRuntimeError::TabLimit);
        }
        if state
            .tabs
            .iter()
            .any(|tab| tab.identity.tab_id == plan.identity.tab_id)
        {
            return Err(BrowserRuntimeError::TabAlreadyExists);
        }
        let tab = LiveTab {
            identity: plan.identity.clone(),
            label: plan.label.clone(),
            page_generation: 0,
            current_url: None,
            approved_loopback_origins: HashSet::new(),
            pending_navigation: (plan.initial_url.scheme() != "about")
                .then_some(plan.initial_navigation),
        };
        state.tabs.push(tab.clone());
        if let Err(error) = self.port.add_child(&plan) {
            state.tabs.pop();
            return Err(error);
        }
        if let Err(error) = self.port.set_visible(&plan.label, false) {
            let _ = self.port.close(&plan.label);
            state.tabs.pop();
            return Err(error);
        }
        if select && state.revealed {
            if let Some(current) = active_tab(&state) {
                self.port.set_visible(&current.label, false)?;
            }
            self.port.set_visible(&tab.label, true)?;
        }
        if select {
            state.active_tab_id = Some(tab.identity.tab_id.clone());
        }
        Ok(())
    }

    pub(crate) fn select_tab(
        &self,
        workspace: &BrowserRuntimeIdentity,
        tab_id: &str,
    ) -> Result<(), BrowserRuntimeError> {
        let mut state = self.lock_selected(workspace)?;
        let target = find_tab(&state, tab_id)?.clone();
        if state.active_tab_id.as_deref() == Some(tab_id) {
            return Ok(());
        }
        if state.revealed {
            if let Some(current) = active_tab(&state) {
                self.port.set_visible(&current.label, false)?;
            }
            self.port.set_visible(&target.label, true)?;
        }
        state.active_tab_id = Some(tab_id.to_string());
        Ok(())
    }

    pub(crate) fn close_tab(
        &self,
        workspace: &BrowserRuntimeIdentity,
        tab_id: &str,
    ) -> Result<Option<String>, BrowserRuntimeError> {
        let mut state = self.lock_selected(workspace)?;
        let index = state
            .tabs
            .iter()
            .position(|tab| tab.identity.tab_id == tab_id)
            .ok_or(BrowserRuntimeError::TabNotFound)?;
        let closing = state.tabs[index].clone();
        self.port.close(&closing.label)?;
        state.tabs.remove(index);
        if state.active_tab_id.as_deref() == Some(tab_id) {
            let fallback = state
                .tabs
                .get(index.min(state.tabs.len().saturating_sub(1)))
                .cloned();
            state.active_tab_id = fallback.as_ref().map(|tab| tab.identity.tab_id.clone());
            if state.revealed {
                if let Some(fallback) = fallback {
                    self.port.set_visible(&fallback.label, true)?;
                }
            }
        }
        Ok(state.active_tab_id.clone())
    }

    pub(crate) fn navigate(
        &self,
        workspace: &BrowserRuntimeIdentity,
        tab_id: &str,
        url: tauri::Url,
    ) -> Result<(), BrowserRuntimeError> {
        self.navigate_with_intent(workspace, tab_id, url, BrowserHistoryNavigation::New)
    }

    pub(crate) fn reopen(
        &self,
        workspace: &BrowserRuntimeIdentity,
        tab_id: &str,
        url: tauri::Url,
    ) -> Result<(), BrowserRuntimeError> {
        self.navigate_with_intent(workspace, tab_id, url, BrowserHistoryNavigation::Reopen)
    }

    pub(crate) fn navigate_history(
        &self,
        workspace: &BrowserRuntimeIdentity,
        tab_id: &str,
        url: tauri::Url,
        navigation: BrowserHistoryNavigation,
    ) -> Result<(), BrowserRuntimeError> {
        debug_assert!(matches!(
            navigation,
            BrowserHistoryNavigation::Back | BrowserHistoryNavigation::Forward
        ));
        self.navigate_with_intent(workspace, tab_id, url, navigation)
    }

    fn navigate_with_intent(
        &self,
        workspace: &BrowserRuntimeIdentity,
        tab_id: &str,
        url: tauri::Url,
        navigation: BrowserHistoryNavigation,
    ) -> Result<(), BrowserRuntimeError> {
        let mut state = self.lock_selected(workspace)?;
        let tab = find_tab_mut(&mut state, tab_id)?;
        let previous = tab.pending_navigation;
        tab.pending_navigation = Some(navigation);
        if let Err(error) = self.port.navigate(&tab.label, &url) {
            tab.pending_navigation = previous;
            return Err(error);
        }
        Ok(())
    }

    pub(crate) fn approve_loopback_origin(
        &self,
        workspace: &BrowserRuntimeIdentity,
        tab_id: &str,
        origin: &str,
    ) -> Result<(), BrowserRuntimeError> {
        let mut state = self.lock_selected(workspace)?;
        let tab = state
            .tabs
            .iter_mut()
            .find(|tab| tab.identity.tab_id == tab_id)
            .ok_or(BrowserRuntimeError::TabNotFound)?;
        tab.approved_loopback_origins.insert(origin.to_string());
        Ok(())
    }

    pub(crate) fn admit_page_navigation(
        &self,
        label: &str,
        validated: &ValidatedBrowserUrl,
    ) -> bool {
        let mut state = self.lock_state();
        let Some(tab) = state.tabs.iter_mut().find(|tab| tab.label == label) else {
            return false;
        };
        if validated.target == BrowserNetworkTarget::Loopback {
            let Some(origin) = loopback_origin(validated) else {
                return false;
            };
            if !tab.approved_loopback_origins.contains(&origin) {
                return false;
            }
        }
        tab.page_generation = tab.page_generation.wrapping_add(1).max(1);
        tab.current_url = Some(validated.url.as_str().to_string());
        tab.pending_navigation
            .get_or_insert(BrowserHistoryNavigation::New);
        true
    }

    pub(crate) fn navigation_finished(
        &self,
        label: &str,
        url: &str,
    ) -> Option<BrowserNavigationCommit> {
        let mut state = self.lock_state();
        let tab = state
            .tabs
            .iter_mut()
            .find(|tab| tab.label == label && tab.current_url.as_deref() == Some(url))?;
        let navigation = tab.pending_navigation.take()?;
        Some(BrowserNavigationCommit {
            workspace: tab.identity.workspace.clone(),
            tab_id: tab.identity.tab_id.clone(),
            page_generation: tab.page_generation,
            url: url.to_string(),
            navigation,
        })
    }

    pub(crate) fn capture_ticket(
        &self,
        workspace: &BrowserRuntimeIdentity,
        tab_id: &str,
    ) -> Result<BrowserCaptureTicket, BrowserRuntimeError> {
        let state = self.lock_selected(workspace)?;
        let tab = find_tab(&state, tab_id)?;
        let current_url = tab
            .current_url
            .clone()
            .ok_or(BrowserRuntimeError::CapturePageChanged)?;
        Ok(BrowserCaptureTicket {
            workspace: tab.identity.workspace.clone(),
            tab_id: tab.identity.tab_id.clone(),
            label: tab.label.clone(),
            page_generation: tab.page_generation,
            current_url,
        })
    }

    pub(crate) fn capture_ticket_is_current(&self, ticket: &BrowserCaptureTicket) -> bool {
        let state = self.lock_state();
        state.selected.as_ref() == Some(&ticket.workspace)
            && state.tabs.iter().any(|tab| {
                tab.identity.tab_id == ticket.tab_id
                    && tab.label == ticket.label
                    && tab.page_generation == ticket.page_generation
                    && tab.current_url.as_deref() == Some(ticket.current_url.as_str())
            })
    }

    pub(crate) fn evaluate_capture(
        &self,
        ticket: &BrowserCaptureTicket,
        script: &str,
        sender: SyncSender<String>,
    ) -> Result<(), BrowserRuntimeError> {
        if !self.capture_ticket_is_current(ticket) {
            return Err(BrowserRuntimeError::CapturePageChanged);
        }
        self.port.eval_with_callback(&ticket.label, script, sender)
    }

    pub(crate) fn reload(
        &self,
        workspace: &BrowserRuntimeIdentity,
        tab_id: &str,
    ) -> Result<(), BrowserRuntimeError> {
        let mut state = self.lock_selected(workspace)?;
        let tab = find_tab_mut(&mut state, tab_id)?;
        let previous = tab.pending_navigation;
        tab.pending_navigation = Some(BrowserHistoryNavigation::Reload);
        if let Err(error) = self.port.reload(&tab.label) {
            tab.pending_navigation = previous;
            return Err(error);
        }
        Ok(())
    }

    pub(crate) fn deactivate(
        &self,
        workspace: &BrowserRuntimeIdentity,
    ) -> Result<(), BrowserRuntimeError> {
        let mut state = self.lock_selected(workspace)?;
        close_runtime_state(&self.port, &mut state)
    }

    pub(crate) fn deactivate_if_selected(
        &self,
        workspace: &BrowserRuntimeIdentity,
    ) -> Result<bool, BrowserRuntimeError> {
        let mut state = self.lock_state();
        if state.selected.as_ref() != Some(workspace) {
            return Ok(false);
        }
        close_runtime_state(&self.port, &mut state)?;
        Ok(true)
    }

    pub(crate) fn deactivate_all(&self) -> Result<(), BrowserRuntimeError> {
        let mut state = self.lock_state();
        close_runtime_state(&self.port, &mut state)
    }

    fn lock_selected<'a>(
        &'a self,
        workspace: &BrowserRuntimeIdentity,
    ) -> Result<MutexGuard<'a, BrowserRuntimeState>, BrowserRuntimeError> {
        let state = self.lock_state();
        if state.selected.as_ref() != Some(workspace) {
            return Err(BrowserRuntimeError::WorkspaceNotSelected);
        }
        Ok(state)
    }

    fn lock_state(&self) -> MutexGuard<'_, BrowserRuntimeState> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

fn find_tab<'a>(
    state: &'a BrowserRuntimeState,
    tab_id: &str,
) -> Result<&'a LiveTab, BrowserRuntimeError> {
    state
        .tabs
        .iter()
        .find(|tab| tab.identity.tab_id == tab_id)
        .ok_or(BrowserRuntimeError::TabNotFound)
}

fn find_tab_mut<'a>(
    state: &'a mut BrowserRuntimeState,
    tab_id: &str,
) -> Result<&'a mut LiveTab, BrowserRuntimeError> {
    state
        .tabs
        .iter_mut()
        .find(|tab| tab.identity.tab_id == tab_id)
        .ok_or(BrowserRuntimeError::TabNotFound)
}

fn active_tab(state: &BrowserRuntimeState) -> Option<&LiveTab> {
    state
        .active_tab_id
        .as_deref()
        .and_then(|tab_id| state.tabs.iter().find(|tab| tab.identity.tab_id == tab_id))
}

fn close_all(port: &impl BrowserRuntimePort, labels: &[String]) {
    for label in labels.iter().rev() {
        let _ = port.close(label);
    }
}

fn close_runtime_state(
    port: &impl BrowserRuntimePort,
    state: &mut BrowserRuntimeState,
) -> Result<(), BrowserRuntimeError> {
    for tab in state.tabs.iter().rev() {
        port.close(&tab.label)?;
    }
    *state = BrowserRuntimeState::default();
    Ok(())
}

#[path = "runtime_tauri.rs"]
mod tauri_port;
pub(crate) use tauri_port::TauriBrowserRuntimePort;
