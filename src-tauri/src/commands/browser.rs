//! Trusted-main commands that own the isolated browser window lifecycle.

use serde::Deserialize;
use tauri::webview::{NewWindowResponse, PageLoadEvent};
use tauri::{AppHandle, Manager, State, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

use crate::browser::policy::{
    allow_download, allow_popup, validate_browser_url, BrowserUrlError, ValidatedBrowserUrl,
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

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BrowserSandboxOpenPayload {
    pub url: String,
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

    store.run_exclusive(|| open_or_reuse(&app, &store, validated))
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
        let allowed = validate_browser_url(url.as_str()).is_ok();
        let store = navigation_app.state::<BrowserSandboxStore>();
        if allowed {
            store.admit_navigation(generation, url)
        } else {
            store.navigation_failed(generation, "browser.navigationBlocked".into());
            false
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

#[cfg(test)]
mod tests {
    use super::{plan_open, require_main_webview, BrowserOpenAction};

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
}
