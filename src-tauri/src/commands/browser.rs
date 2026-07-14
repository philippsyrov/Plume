//! Trusted-main commands that own the isolated browser window lifecycle.

use serde::Deserialize;
use tauri::webview::{NewWindowResponse, PageLoadEvent};
use tauri::{AppHandle, Manager, State, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

use crate::browser::policy::{
    allow_download, allow_popup, loopback_origin, validate_browser_url, BrowserNetworkTarget,
    BrowserUrlError, ValidatedBrowserUrl,
};
use crate::browser::state::{BrowserSandboxState, BrowserSandboxStore, BROWSER_SANDBOX_LABEL};
use crate::commands::project::EmptyPayload;
use crate::error::IpcError;
use crate::error::IpcRequest;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BrowserOpenAction {
    Create,
    Reuse,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BrowserFixedAction {
    Back,
    Forward,
}

const fn fixed_navigation_script(action: BrowserFixedAction) -> &'static str {
    match action {
        BrowserFixedAction::Back => "history.back()",
        BrowserFixedAction::Forward => "history.forward()",
    }
}

fn missing_window_error() -> IpcError {
    IpcError::NotFound("browser.sandboxWindow".into())
}

fn require_main_webview(label: &str) -> Result<(), IpcError> {
    if label == "main" {
        Ok(())
    } else {
        Err(IpcError::Blocked("browser.mainWebviewRequired".into()))
    }
}

fn plan_open(window_exists: bool) -> BrowserOpenAction {
    if window_exists {
        BrowserOpenAction::Reuse
    } else {
        BrowserOpenAction::Create
    }
}

fn map_url_error(error: BrowserUrlError) -> IpcError {
    match error {
        BrowserUrlError::InvalidUrl => IpcError::BadArgument("browser.invalidUrl".into()),
        BrowserUrlError::SchemeBlocked => IpcError::Blocked("browser.schemeBlocked".into()),
        BrowserUrlError::CredentialsBlocked => {
            IpcError::Blocked("browser.credentialsBlocked".into())
        }
    }
}

fn lifecycle_failure(reason: &'static str) -> IpcError {
    IpcError::Internal(reason.into())
}

fn authorize_open_target(
    store: &BrowserSandboxStore,
    validated: &ValidatedBrowserUrl,
    approved_loopback_origin: Option<&str>,
) -> Result<(), IpcError> {
    let required_origin = loopback_origin(validated);
    match (required_origin.as_deref(), approved_loopback_origin) {
        (None, None) => Ok(()),
        (None, Some(_)) => Err(IpcError::BadArgument(
            "browser.unexpectedLoopbackApproval".into(),
        )),
        (Some(required), Some(supplied)) if required != supplied => Err(IpcError::BadArgument(
            "browser.loopbackApprovalMismatch".into(),
        )),
        (Some(required), Some(_)) => {
            store.approve_loopback_origin(required);
            Ok(())
        }
        (Some(required), None) if store.is_loopback_origin_approved(required) => Ok(()),
        (Some(_), None) => Err(IpcError::NeedsApproval),
    }
}

fn admit_page_navigation(
    store: &BrowserSandboxStore,
    generation: u64,
    validated: &ValidatedBrowserUrl,
) -> bool {
    if validated.target == BrowserNetworkTarget::Loopback {
        let Some(origin) = loopback_origin(validated) else {
            store.navigation_failed(generation, "browser.navigationBlocked".into());
            return false;
        };
        if !store.is_loopback_origin_approved(&origin) {
            store.loopback_approval_required(generation);
            return false;
        }
    }
    store.admit_navigation(generation, &validated.url)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BrowserSandboxOpenPayload {
    pub url: String,
    #[serde(default)]
    pub approved_loopback_origin: Option<String>,
}

#[tauri::command]
pub async fn browser_sandbox_open(
    req: IpcRequest<BrowserSandboxOpenPayload>,
    caller: WebviewWindow,
    app: AppHandle,
    store: State<'_, BrowserSandboxStore>,
) -> Result<BrowserSandboxState, IpcError> {
    req.check_version()?;
    require_main_webview(caller.label())?;
    let validated = validate_browser_url(&req.payload.url).map_err(map_url_error)?;

    store.run_exclusive(|| {
        authorize_open_target(
            &store,
            &validated,
            req.payload.approved_loopback_origin.as_deref(),
        )?;
        open_or_reuse(&app, &store, validated)
    })
}

fn open_or_reuse(
    app: &AppHandle,
    store: &BrowserSandboxStore,
    validated: ValidatedBrowserUrl,
) -> Result<BrowserSandboxState, IpcError> {
    let existing = app.get_webview_window(BROWSER_SANDBOX_LABEL);
    match plan_open(existing.is_some()) {
        BrowserOpenAction::Reuse => {
            let window = existing.ok_or_else(|| lifecycle_failure("browser.windowCreateFailed"))?;
            if store.is_loading_url(&validated.url) {
                window
                    .set_focus()
                    .map_err(|_| lifecycle_failure("browser.windowFocusFailed"))?;
                return Ok(store.snapshot());
            }
            let generation = store.opening_existing_window(&validated.url);
            if window.navigate(validated.url).is_err() {
                store.navigation_failed(generation, "browser.navigationFailed".into());
                return Err(lifecycle_failure("browser.navigationFailed"));
            }
            window
                .set_focus()
                .map_err(|_| lifecycle_failure("browser.windowFocusFailed"))?;
            Ok(store.snapshot())
        }
        BrowserOpenAction::Create => create_sandbox_window(app, store, validated),
    }
}

fn create_sandbox_window(
    app: &AppHandle,
    store: &BrowserSandboxStore,
    validated: ValidatedBrowserUrl,
) -> Result<BrowserSandboxState, IpcError> {
    let generation = store.opening_new_window(&validated.url);

    let navigation_app = app.clone();
    let load_app = app.clone();
    let window = WebviewWindowBuilder::new(
        app,
        BROWSER_SANDBOX_LABEL,
        WebviewUrl::External(validated.url),
    )
    .title("Plume Browser")
    .inner_size(1000.0, 720.0)
    .min_inner_size(640.0, 480.0)
    .incognito(true)
    .browser_extensions_enabled(false)
    .general_autofill_enabled(false)
    .devtools(false)
    .on_navigation(move |url| {
        let store = navigation_app.state::<BrowserSandboxStore>();
        match validate_browser_url(url.as_str()) {
            Ok(validated) => admit_page_navigation(&store, generation, &validated),
            Err(_) => {
                store.navigation_failed(generation, "browser.navigationBlocked".into());
                false
            }
        }
    })
    .on_new_window(|_, _| {
        debug_assert!(!allow_popup());
        NewWindowResponse::Deny
    })
    .on_download(|_, _| allow_download())
    .on_page_load(move |_, payload| {
        let store = load_app.state::<BrowserSandboxStore>();
        if matches!(payload.event(), PageLoadEvent::Finished) {
            store.navigation_finished(generation, payload.url());
        }
    })
    .build();

    let window = match window {
        Ok(window) => window,
        Err(_) => {
            store.closed_if_generation(generation);
            return Err(lifecycle_failure("browser.windowCreateFailed"));
        }
    };

    let destroyed_app = app.clone();
    window.on_window_event(move |event| {
        if matches!(event, tauri::WindowEvent::Destroyed) {
            destroyed_app
                .state::<BrowserSandboxStore>()
                .closed_if_generation(generation);
        }
    });

    Ok(store.snapshot())
}

#[tauri::command]
pub async fn browser_sandbox_close(
    req: IpcRequest<EmptyPayload>,
    caller: WebviewWindow,
    app: AppHandle,
    store: State<'_, BrowserSandboxStore>,
) -> Result<BrowserSandboxState, IpcError> {
    req.check_version()?;
    require_main_webview(caller.label())?;

    store.run_exclusive(|| {
        if let Some(window) = app.get_webview_window(BROWSER_SANDBOX_LABEL) {
            window
                .close()
                .map_err(|_| lifecycle_failure("browser.windowCloseFailed"))?;
        }
        store.closed();
        Ok(store.snapshot())
    })
}

#[tauri::command]
pub async fn browser_sandbox_state(
    req: IpcRequest<EmptyPayload>,
    caller: WebviewWindow,
    store: State<'_, BrowserSandboxStore>,
) -> Result<BrowserSandboxState, IpcError> {
    req.check_version()?;
    require_main_webview(caller.label())?;
    Ok(store.snapshot())
}

#[tauri::command]
pub async fn browser_sandbox_focus(
    req: IpcRequest<EmptyPayload>,
    caller: WebviewWindow,
    app: AppHandle,
    store: State<'_, BrowserSandboxStore>,
) -> Result<BrowserSandboxState, IpcError> {
    req.check_version()?;
    require_main_webview(caller.label())?;

    store.run_exclusive(|| {
        let window = app
            .get_webview_window(BROWSER_SANDBOX_LABEL)
            .ok_or_else(missing_window_error)?;
        window
            .set_focus()
            .map_err(|_| lifecycle_failure("browser.windowFocusFailed"))?;
        Ok(store.snapshot())
    })
}

async fn browser_history_action(
    req: IpcRequest<EmptyPayload>,
    caller: WebviewWindow,
    app: AppHandle,
    store: State<'_, BrowserSandboxStore>,
    action: BrowserFixedAction,
) -> Result<BrowserSandboxState, IpcError> {
    req.check_version()?;
    require_main_webview(caller.label())?;

    store.run_exclusive(|| {
        let window = app
            .get_webview_window(BROWSER_SANDBOX_LABEL)
            .ok_or_else(missing_window_error)?;
        window
            .eval(fixed_navigation_script(action))
            .map_err(|_| lifecycle_failure("browser.historyNavigationFailed"))?;
        Ok(store.snapshot())
    })
}

#[tauri::command]
pub async fn browser_sandbox_back(
    req: IpcRequest<EmptyPayload>,
    caller: WebviewWindow,
    app: AppHandle,
    store: State<'_, BrowserSandboxStore>,
) -> Result<BrowserSandboxState, IpcError> {
    browser_history_action(req, caller, app, store, BrowserFixedAction::Back).await
}

#[tauri::command]
pub async fn browser_sandbox_forward(
    req: IpcRequest<EmptyPayload>,
    caller: WebviewWindow,
    app: AppHandle,
    store: State<'_, BrowserSandboxStore>,
) -> Result<BrowserSandboxState, IpcError> {
    browser_history_action(req, caller, app, store, BrowserFixedAction::Forward).await
}

#[tauri::command]
pub async fn browser_sandbox_reload(
    req: IpcRequest<EmptyPayload>,
    caller: WebviewWindow,
    app: AppHandle,
    store: State<'_, BrowserSandboxStore>,
) -> Result<BrowserSandboxState, IpcError> {
    req.check_version()?;
    require_main_webview(caller.label())?;

    store.run_exclusive(|| {
        let window = app
            .get_webview_window(BROWSER_SANDBOX_LABEL)
            .ok_or_else(missing_window_error)?;
        window
            .reload()
            .map_err(|_| lifecycle_failure("browser.reloadFailed"))?;
        Ok(store.snapshot())
    })
}

#[cfg(test)]
mod tests {
    use crate::browser::policy::validate_browser_url;
    use crate::browser::state::{BrowserNavigationFailureReason, BrowserSandboxStore};
    use crate::error::IpcError;

    use super::{
        admit_page_navigation, authorize_open_target, fixed_navigation_script,
        missing_window_error, plan_open, require_main_webview, BrowserFixedAction,
        BrowserOpenAction,
    };

    fn url(raw: &str) -> tauri::Url {
        tauri::Url::parse(raw).unwrap()
    }

    #[test]
    fn only_the_trusted_main_webview_can_request_browser_lifecycle_changes() {
        assert!(require_main_webview("main").is_ok());
        for label in ["", "browser-sandbox", "other"] {
            assert!(
                require_main_webview(label).is_err(),
                "{label:?} must not control browser lifecycle"
            );
        }
    }

    #[test]
    fn opening_reuses_the_single_existing_sandbox_window() {
        assert_eq!(plan_open(false), BrowserOpenAction::Create);
        assert_eq!(plan_open(true), BrowserOpenAction::Reuse);
    }

    #[test]
    fn public_targets_reject_an_unexpected_loopback_approval_field() {
        let store = BrowserSandboxStore::default();
        let public = validate_browser_url("https://example.com/").unwrap();

        assert!(authorize_open_target(&store, &public, None).is_ok());
        assert!(matches!(
            authorize_open_target(&store, &public, Some("http://localhost:5173")),
            Err(IpcError::BadArgument(_))
        ));
    }

    #[test]
    fn loopback_open_requires_the_exact_normalized_origin_once_per_session() {
        let store = BrowserSandboxStore::default();
        store.opening_new_window(&url("https://example.com/"));
        let target = validate_browser_url("http://localhost:5173/path").unwrap();

        assert!(matches!(
            authorize_open_target(&store, &target, None),
            Err(IpcError::NeedsApproval)
        ));
        assert!(matches!(
            authorize_open_target(&store, &target, Some("http://localhost:5174")),
            Err(IpcError::BadArgument(_))
        ));
        assert!(authorize_open_target(&store, &target, Some("http://localhost:5173")).is_ok());
        assert!(authorize_open_target(&store, &target, None).is_ok());
    }

    #[test]
    fn page_authored_navigation_cannot_enter_an_unapproved_loopback_origin() {
        let store = BrowserSandboxStore::default();
        let generation = store.opening_new_window(&url("https://example.com/"));
        let loopback = validate_browser_url("http://localhost:5173/").unwrap();

        assert!(!admit_page_navigation(&store, generation, &loopback));
        assert_eq!(
            store.snapshot().failure.unwrap().reason,
            BrowserNavigationFailureReason::LoopbackApprovalRequired
        );

        store.approve_loopback_origin("http://localhost:5173");
        assert!(admit_page_navigation(&store, generation, &loopback));
    }

    #[test]
    fn history_controls_are_closed_fixed_purpose_actions() {
        assert_eq!(
            fixed_navigation_script(BrowserFixedAction::Back),
            "history.back()"
        );
        assert_eq!(
            fixed_navigation_script(BrowserFixedAction::Forward),
            "history.forward()"
        );
    }

    #[test]
    fn absent_browser_window_is_a_typed_not_found_error() {
        assert!(matches!(missing_window_error(), IpcError::NotFound(_)));
    }
}
