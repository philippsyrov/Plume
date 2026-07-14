use tauri::ipc::{CallbackFn, InvokeBody};
use tauri::test::{get_ipc_response, mock_builder, MockRuntime, INVOKE_KEY};
use tauri::webview::InvokeRequest;
use tauri::{Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

use crate::{__cmd__ping, __tauri_command_name_ping, ping, plume_context};

fn test_app() -> tauri::App<MockRuntime> {
    mock_builder()
        .invoke_handler(tauri::generate_handler![ping])
        .build(plume_context())
        .expect("mock app must build with production capabilities")
}

fn webview(app: &tauri::App<MockRuntime>, label: &str) -> WebviewWindow<MockRuntime> {
    app.get_webview_window(label).unwrap_or_else(|| {
        WebviewWindowBuilder::new(app, label, WebviewUrl::App("index.html".into()))
            .build()
            .expect("mock webview must build")
    })
}

fn request(command: &str, origin: &str) -> InvokeRequest {
    InvokeRequest {
        cmd: command.into(),
        callback: CallbackFn(0),
        error: CallbackFn(1),
        url: origin.parse().unwrap(),
        body: InvokeBody::default(),
        headers: Default::default(),
        invoke_key: INVOKE_KEY.to_string(),
    }
}

fn assert_acl_denied(
    result: Result<tauri::ipc::InvokeResponseBody, serde_json::Value>,
    command_name: &str,
) {
    let error = result.expect_err("sandbox request must be denied");
    let message = error
        .as_str()
        .expect("Tauri ACL denial must be a string error");
    assert!(message.contains(&format!("{command_name} not allowed")));
    assert!(message.contains("webview \"browser-sandbox\""));
    assert!(message.contains("allowed on: [webviews: \"main\""));
}

#[test]
fn production_acl_grants_main_and_denies_the_browser_sandbox() {
    let app = test_app();
    let main = webview(&app, "main");
    let sandbox = webview(&app, "browser-sandbox");
    let local_origin = if cfg!(any(windows, target_os = "android")) {
        "http://tauri.localhost"
    } else {
        "tauri://localhost"
    };

    let main_ping = get_ipc_response(&main, request("ping", local_origin))
        .expect("trusted main webview must retain ping authority")
        .deserialize::<String>()
        .unwrap();
    assert_eq!(main_ping, "pong");

    if let Err(error) = get_ipc_response(&main, request("plugin:event|listen", local_origin)) {
        assert!(
            !error.to_string().contains("not allowed"),
            "main event request may fail argument parsing but must pass ACL: {error}"
        );
    }

    let local_sandbox_ping = get_ipc_response(&sandbox, request("ping", local_origin));
    assert_acl_denied(local_sandbox_ping, "ping");

    let remote_sandbox_ping =
        get_ipc_response(&sandbox, request("ping", "https://attacker.example"));
    assert_acl_denied(remote_sandbox_ping, "ping");

    let sandbox_event = get_ipc_response(&sandbox, request("plugin:event|listen", local_origin));
    assert_acl_denied(sandbox_event, "event.listen");
}
