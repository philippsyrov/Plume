use std::sync::Mutex;

use super::runtime::{
    BrowserBounds, BrowserChildPlan, BrowserRuntimeError, BrowserRuntimeIdentity,
    BrowserRuntimeManager, BrowserRuntimePort, LiveTabIdentity,
};
use crate::sessions::browser_workspace::BrowserWorkspaceScope;

#[derive(Default)]
struct RecordingPort {
    added: Mutex<Vec<BrowserChildPlan>>,
    visibility: Mutex<Vec<(String, bool)>>,
}

impl BrowserRuntimePort for RecordingPort {
    fn add_child(&self, plan: &BrowserChildPlan) -> Result<(), BrowserRuntimeError> {
        self.added.lock().unwrap().push(plan.clone());
        Ok(())
    }

    fn set_bounds(&self, _label: &str, _bounds: BrowserBounds) -> Result<(), BrowserRuntimeError> {
        Ok(())
    }

    fn set_visible(&self, label: &str, visible: bool) -> Result<(), BrowserRuntimeError> {
        self.visibility
            .lock()
            .unwrap()
            .push((label.to_string(), visible));
        Ok(())
    }

    fn eval(&self, _label: &str, _script: &str) -> Result<(), BrowserRuntimeError> {
        Ok(())
    }

    fn reload(&self, _label: &str) -> Result<(), BrowserRuntimeError> {
        Ok(())
    }

    fn close(&self, _label: &str) -> Result<(), BrowserRuntimeError> {
        Ok(())
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
fn activation_creates_all_tabs_then_shows_only_the_active_tab() {
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
        vec![(one.label, false), (two.label, true)]
    );
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
