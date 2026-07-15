use std::sync::mpsc::SyncSender;
use std::sync::Mutex;

use super::runtime::{
    BrowserBounds, BrowserChildPlan, BrowserRuntimeError, BrowserRuntimeIdentity,
    BrowserRuntimeManager, BrowserRuntimePort, LiveTabIdentity,
};
use crate::browser::policy::validate_browser_url;
use crate::sessions::browser_workspace::{BrowserHistoryNavigation, BrowserWorkspaceScope};

#[derive(Default)]
struct RecordingPort {
    added: Mutex<Vec<BrowserChildPlan>>,
    bounds: Mutex<Vec<(String, BrowserBounds)>>,
    visibility: Mutex<Vec<(String, bool)>>,
    navigation: Mutex<Vec<(String, String)>>,
    reloads: Mutex<Vec<String>>,
    closed: Mutex<Vec<String>>,
    capture_evals: Mutex<Vec<(String, String)>>,
}

impl BrowserRuntimePort for RecordingPort {
    fn add_child(&self, plan: &BrowserChildPlan) -> Result<(), BrowserRuntimeError> {
        self.added.lock().unwrap().push(plan.clone());
        Ok(())
    }

    fn set_bounds(&self, _label: &str, _bounds: BrowserBounds) -> Result<(), BrowserRuntimeError> {
        self.bounds
            .lock()
            .unwrap()
            .push((_label.to_string(), _bounds));
        Ok(())
    }

    fn set_visible(&self, label: &str, visible: bool) -> Result<(), BrowserRuntimeError> {
        self.visibility
            .lock()
            .unwrap()
            .push((label.to_string(), visible));
        Ok(())
    }

    fn reload(&self, _label: &str) -> Result<(), BrowserRuntimeError> {
        self.reloads.lock().unwrap().push(_label.to_string());
        Ok(())
    }

    fn navigate(&self, label: &str, url: &tauri::Url) -> Result<(), BrowserRuntimeError> {
        self.navigation
            .lock()
            .unwrap()
            .push((label.to_string(), url.to_string()));
        Ok(())
    }

    fn close(&self, _label: &str) -> Result<(), BrowserRuntimeError> {
        self.closed.lock().unwrap().push(_label.to_string());
        Ok(())
    }

    fn eval_with_callback(
        &self,
        label: &str,
        script: &str,
        sender: SyncSender<String>,
    ) -> Result<(), BrowserRuntimeError> {
        self.capture_evals
            .lock()
            .unwrap()
            .push((label.to_string(), script.to_string()));
        sender
            .send(r#"{"url":"https://example.com/page","title":"Example","content":"captured","truncated":false}"#.into())
            .map_err(|_| BrowserRuntimeError::Native("browser.captureCallbackFailed".into()))
    }
}

fn workspace(session_id: &str) -> BrowserRuntimeIdentity {
    BrowserRuntimeIdentity {
        scope: BrowserWorkspaceScope::Local,
        session_id: session_id.to_string(),
    }
}

fn tab(session_id: &str, tab_id: &str, generation: u64) -> LiveTabIdentity {
    LiveTabIdentity {
        workspace: workspace(session_id),
        tab_id: tab_id.to_string(),
        generation,
    }
}

#[test]
fn child_labels_are_deterministic_bounded_and_do_not_expose_session_ids() {
    let first = BrowserRuntimeManager::<RecordingPort>::child_label(&tab(
        "s_0123456789abcdef0123456789abcdef",
        "tab-private-name",
        7,
    ));
    let second = BrowserRuntimeManager::<RecordingPort>::child_label(&tab(
        "s_0123456789abcdef0123456789abcdef",
        "tab-private-name",
        7,
    ));
    let other_generation = BrowserRuntimeManager::<RecordingPort>::child_label(&tab(
        "s_0123456789abcdef0123456789abcdef",
        "tab-private-name",
        8,
    ));

    assert_eq!(first, second);
    assert_ne!(first, other_generation);
    assert!(first.starts_with("task-browser-"));
    assert!(first.len() <= 63);
    assert!(!first.contains("0123456789abcdef"));
    assert!(!first.contains("private-name"));
}

#[test]
fn child_plan_is_main_window_only_bounded_hidden_by_default_and_persistent() {
    let bounds = BrowserBounds::new(12.0, 24.0, 900.0, 640.0).unwrap();
    let plan = BrowserRuntimeManager::<RecordingPort>::plan_child(
        tab("s_0123456789abcdef0123456789abcdef", "tab_1", 1),
        "https://example.com/".parse().unwrap(),
        bounds,
    );

    assert_eq!(plan.parent_window_label, "main");
    assert_eq!(plan.bounds, bounds);
    assert!(!plan.visible);
    assert!(plan.persistent_data_store);
    assert!(!plan.devtools);
    assert!(!plan.extensions);
    assert!(!plan.autofill);
    assert!(!plan.allow_popups);
    assert!(!plan.allow_downloads);
}

#[test]
fn bounds_reject_non_finite_zero_negative_and_unreasonably_large_geometry() {
    for invalid in [
        (f64::NAN, 0.0, 100.0, 100.0),
        (0.0, f64::INFINITY, 100.0, 100.0),
        (0.0, 0.0, 0.0, 100.0),
        (0.0, 0.0, 100.0, -1.0),
        (-1.0, 0.0, 100.0, 100.0),
        (0.0, -1.0, 100.0, 100.0),
        (0.0, 0.0, 20_001.0, 100.0),
    ] {
        assert!(BrowserBounds::new(invalid.0, invalid.1, invalid.2, invalid.3).is_err());
    }
}

#[test]
fn activation_creates_tabs_hidden_then_geometry_reveals_only_the_active_tab() {
    let port = RecordingPort::default();
    let manager = BrowserRuntimeManager::new(port);
    let bounds = BrowserBounds::new(10.0, 20.0, 800.0, 600.0).unwrap();
    let one = BrowserRuntimeManager::<RecordingPort>::plan_child(
        tab("s_0123456789abcdef0123456789abcdef", "tab_1", 1),
        "https://example.com/one".parse().unwrap(),
        bounds,
    );
    let two = BrowserRuntimeManager::<RecordingPort>::plan_child(
        tab("s_0123456789abcdef0123456789abcdef", "tab_2", 1),
        "https://example.com/two".parse().unwrap(),
        bounds,
    );

    manager
        .activate(vec![one.clone(), two.clone()], &two.identity.tab_id)
        .unwrap();

    let added = manager.port().added.lock().unwrap().clone();
    assert_eq!(added, vec![one.clone(), two.clone()]);
    assert_eq!(
        *manager.port().visibility.lock().unwrap(),
        vec![(one.label.clone(), false), (two.label.clone(), false)]
    );

    let revealed = BrowserBounds::new(40.0, 50.0, 1_200.0, 800.0).unwrap();
    manager
        .set_bounds(&two.identity.workspace, revealed)
        .unwrap();

    assert_eq!(
        *manager.port().bounds.lock().unwrap(),
        vec![(one.label.clone(), revealed), (two.label.clone(), revealed)]
    );
    assert_eq!(
        *manager.port().visibility.lock().unwrap(),
        vec![
            (one.label.clone(), false),
            (two.label.clone(), false),
            (one.label.clone(), false),
            (two.label.clone(), false),
            (two.label.clone(), true),
        ],
        "geometry changes hide every native child before resizing and reveal only the active tab",
    );
}

#[test]
fn stale_workspace_cannot_mutate_the_selected_runtime() {
    let manager = BrowserRuntimeManager::new(RecordingPort::default());
    let bounds = BrowserBounds::new(0.0, 0.0, 640.0, 480.0).unwrap();
    let selected = workspace("s_0123456789abcdef0123456789abcdef");
    let stale = workspace("s_fedcba9876543210fedcba9876543210");
    let plan = BrowserRuntimeManager::<RecordingPort>::plan_child(
        tab(&selected.session_id, "tab_1", 1),
        "https://example.com/".parse().unwrap(),
        bounds,
    );
    manager.activate(vec![plan], "tab_1").unwrap();

    assert_eq!(
        manager.select_tab(&stale, "tab_1"),
        Err(BrowserRuntimeError::WorkspaceNotSelected)
    );
    assert_eq!(
        manager.reload(&stale, "tab_1"),
        Err(BrowserRuntimeError::WorkspaceNotSelected)
    );
    assert!(manager.port().reloads.lock().unwrap().is_empty());
}

#[test]
fn selecting_navigating_and_reloading_target_only_the_requested_live_tab() {
    let manager = BrowserRuntimeManager::new(RecordingPort::default());
    let identity = workspace("s_0123456789abcdef0123456789abcdef");
    let bounds = BrowserBounds::new(0.0, 0.0, 640.0, 480.0).unwrap();
    let one = BrowserRuntimeManager::<RecordingPort>::plan_child(
        tab(&identity.session_id, "tab_1", 1),
        "https://example.com/one".parse().unwrap(),
        bounds,
    );
    let two = BrowserRuntimeManager::<RecordingPort>::plan_child(
        tab(&identity.session_id, "tab_2", 1),
        "https://example.com/two".parse().unwrap(),
        bounds,
    );
    manager
        .activate(vec![one.clone(), two.clone()], "tab_1")
        .unwrap();
    manager.set_bounds(&identity, bounds).unwrap();
    manager.select_tab(&identity, "tab_2").unwrap();
    manager
        .navigate(
            &identity,
            "tab_2",
            "https://example.com/next".parse().unwrap(),
        )
        .unwrap();
    manager
        .navigate_history(
            &identity,
            "tab_2",
            "https://example.com/one".parse().unwrap(),
            BrowserHistoryNavigation::Back,
        )
        .unwrap();
    manager
        .navigate_history(
            &identity,
            "tab_2",
            "https://example.com/two".parse().unwrap(),
            BrowserHistoryNavigation::Forward,
        )
        .unwrap();
    manager.reload(&identity, "tab_2").unwrap();

    assert_eq!(
        *manager.port().navigation.lock().unwrap(),
        vec![
            (two.label.clone(), "https://example.com/next".into()),
            (two.label.clone(), "https://example.com/one".into()),
            (two.label.clone(), "https://example.com/two".into()),
        ]
    );
    assert_eq!(*manager.port().reloads.lock().unwrap(), vec![two.label]);
}

#[test]
fn closing_active_tab_selects_a_deterministic_fallback_and_deactivate_closes_rest() {
    let manager = BrowserRuntimeManager::new(RecordingPort::default());
    let identity = workspace("s_0123456789abcdef0123456789abcdef");
    let bounds = BrowserBounds::new(0.0, 0.0, 640.0, 480.0).unwrap();
    let one = BrowserRuntimeManager::<RecordingPort>::plan_child(
        tab(&identity.session_id, "tab_1", 1),
        "https://example.com/one".parse().unwrap(),
        bounds,
    );
    let two = BrowserRuntimeManager::<RecordingPort>::plan_child(
        tab(&identity.session_id, "tab_2", 1),
        "https://example.com/two".parse().unwrap(),
        bounds,
    );
    manager
        .activate(vec![one.clone(), two.clone()], "tab_2")
        .unwrap();
    manager.set_bounds(&identity, bounds).unwrap();

    assert_eq!(
        manager.close_tab(&identity, "tab_2").unwrap(),
        Some("tab_1".into())
    );
    manager.deactivate(&identity).unwrap();

    assert_eq!(
        *manager.port().closed.lock().unwrap(),
        vec![two.label, one.label]
    );
    assert_eq!(manager.selected_identity(), None);
}

#[test]
fn serialized_plan_contains_no_cookie_or_web_storage_material() {
    let plan = BrowserRuntimeManager::<RecordingPort>::plan_child(
        tab("s_0123456789abcdef0123456789abcdef", "tab_1", 1),
        "https://example.com/".parse().unwrap(),
        BrowserBounds::new(0.0, 0.0, 640.0, 480.0).unwrap(),
    );
    let debug = format!("{plan:?}").to_lowercase();

    assert!(!debug.contains("cookie"));
    assert!(!debug.contains("localstorage"));
    assert!(!debug.contains("sessionstorage"));
}

#[test]
fn native_navigation_callbacks_are_identity_bound_and_loopback_is_exact_origin_gated() {
    let manager = BrowserRuntimeManager::new(RecordingPort::default());
    let identity = workspace("s_0123456789abcdef0123456789abcdef");
    let bounds = BrowserBounds::new(0.0, 0.0, 640.0, 480.0).unwrap();
    let plan = BrowserRuntimeManager::<RecordingPort>::plan_child(
        tab(&identity.session_id, "tab_1", 1),
        "https://example.com/".parse().unwrap(),
        bounds,
    );
    let label = plan.label.clone();
    manager.activate(vec![plan], "tab_1").unwrap();

    assert!(manager.admit_page_navigation(
        &label,
        &validate_browser_url("https://example.com/").unwrap()
    ));
    assert_eq!(
        manager
            .navigation_finished(&label, "https://example.com/")
            .unwrap()
            .navigation,
        crate::sessions::browser_workspace::BrowserHistoryNavigation::Restore
    );
    assert!(manager.admit_page_navigation(
        &label,
        &validate_browser_url("https://example.com/page").unwrap()
    ));
    assert!(!manager.admit_page_navigation(
        &label,
        &validate_browser_url("http://localhost:3000/").unwrap()
    ));
    manager
        .approve_loopback_origin(&identity, "tab_1", "http://localhost:3000")
        .unwrap();
    assert!(manager.admit_page_navigation(
        &label,
        &validate_browser_url("http://localhost:3000/after-approval").unwrap()
    ));
    assert!(!manager.admit_page_navigation(
        &label,
        &validate_browser_url("http://localhost:4000/wrong-origin").unwrap()
    ));

    let commit = manager
        .navigation_finished(&label, "http://localhost:3000/after-approval")
        .unwrap();
    assert_eq!(commit.workspace, identity);
    assert_eq!(commit.tab_id, "tab_1");
    assert_eq!(commit.url, "http://localhost:3000/after-approval");
    assert_eq!(
        commit.navigation,
        crate::sessions::browser_workspace::BrowserHistoryNavigation::New
    );
    assert!(manager
        .navigation_finished(&label, "https://stale.example/")
        .is_none());
    assert!(!manager.admit_page_navigation(
        "task-browser-stale-label",
        &validate_browser_url("https://example.com/").unwrap()
    ));
}

#[test]
fn capture_ticket_is_bound_to_exact_workspace_tab_page_and_url() {
    let manager = BrowserRuntimeManager::new(RecordingPort::default());
    let identity = workspace("s_0123456789abcdef0123456789abcdef");
    let plan = BrowserRuntimeManager::<RecordingPort>::plan_child(
        tab(&identity.session_id, "tab_1", 1),
        "https://example.com/page".parse().unwrap(),
        BrowserBounds::new(0.0, 0.0, 640.0, 480.0).unwrap(),
    );
    let label = plan.label.clone();
    manager.activate(vec![plan], "tab_1").unwrap();
    assert!(manager.admit_page_navigation(
        &label,
        &validate_browser_url("https://example.com/page").unwrap()
    ));

    let ticket = manager.capture_ticket(&identity, "tab_1").unwrap();
    assert_eq!(ticket.workspace, identity);
    assert_eq!(ticket.tab_id, "tab_1");
    assert_eq!(ticket.current_url, "https://example.com/page");
    assert!(manager.capture_ticket_is_current(&ticket));

    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    manager
        .evaluate_capture(&ticket, "fixed-capture-script", sender)
        .unwrap();
    assert!(receiver.recv().unwrap().contains("captured"));
    assert_eq!(
        *manager.port().capture_evals.lock().unwrap(),
        vec![(label.clone(), "fixed-capture-script".into())]
    );

    assert!(manager.admit_page_navigation(
        &label,
        &validate_browser_url("https://example.com/next").unwrap()
    ));
    assert!(!manager.capture_ticket_is_current(&ticket));
    let (sender, _receiver) = std::sync::mpsc::sync_channel(1);
    assert_eq!(
        manager.evaluate_capture(&ticket, "fixed-capture-script", sender),
        Err(BrowserRuntimeError::CapturePageChanged)
    );
}
