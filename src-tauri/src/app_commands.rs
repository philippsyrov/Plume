//! Build-time registry for every Plume application command.

/// Complete ordered list of commands registered by `tauri::generate_handler!`.
pub const APP_COMMANDS: &[&str] = &[
    "ping",
    "browser_sandbox_open",
    "browser_sandbox_close",
    "browser_sandbox_state",
    "browser_sandbox_focus",
    "browser_sandbox_back",
    "browser_sandbox_forward",
    "browser_sandbox_reload",
    "browser_sandbox_capture_text",
    "browser_sandbox_capture_screenshot",
    "browser_workspace_load",
    "browser_workspace_save",
    "browser_workspace_reset",
    "task_browser_activate",
    "task_browser_deactivate",
    "task_browser_open_tab",
    "task_browser_close_tab",
    "task_browser_select_tab",
    "task_browser_navigate",
    "task_browser_back",
    "task_browser_forward",
    "task_browser_reload",
    "task_browser_set_geometry",
    "task_browser_capture_text",
    "task_browser_capture_screenshot",
    "project_open",
    "project_refresh",
    "project_trust",
    "project_trust_state",
    "fs_list",
    "fs_read",
    "providers_list",
    "providers_health",
    "providers_local_models",
    "providers_local_model_details",
    "providers_model_details",
    "providers_start_server",
    "providers_stop_server",
    "providers_server_diagnostics",
    "system_snapshot",
    "chat_send",
    "chat_cancel",
    "chat_context",
    "patch_validate",
    "patch_apply",
    "patch_revert",
    "memory_index",
    "memory_remember",
    "memory_update",
    "memory_forget",
    "memory_search",
    "memory_distill_preview",
    "memory_distill_apply",
    "memory_distill_log",
    "memory_topics",
    "memory_set_links",
    "memory_user_index",
    "memory_user_remember",
    "memory_user_update",
    "memory_user_forget",
    "memory_user_search",
    "session_set_mode",
    "session_set_approval_policy",
    "session_set_allowlist",
    "session_state",
    "sessions_list",
    "sessions_fork",
    "sessions_rollback",
    "sessions_create",
    "sessions_load",
    "sessions_rename",
    "sessions_archive",
    "sessions_delete",
    "sessions_save_transcript",
    "sessions_search",
    "skills_list",
    "skills_load",
    "skills_preview",
    "skills_apply",
    "skills_promote_preview",
    "skills_promotion_context",
    "tools_list",
    "tools_search",
    "agent_dry_run",
    "agent_single_step",
];

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::APP_COMMANDS;

    fn handler_names(source: &str) -> Vec<&str> {
        let marker = "tauri::generate_handler![";
        let start = source
            .find(marker)
            .expect("lib.rs must contain one generate_handler block")
            + marker.len();
        let rest = &source[start..];
        let end = rest
            .find(']')
            .expect("generate_handler block must be closed");
        rest[..end]
            .split(',')
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .collect()
    }

    #[test]
    fn registered_handlers_match_the_application_manifest() {
        let registered = handler_names(include_str!("lib.rs"));
        let unique = registered.iter().copied().collect::<HashSet<_>>();
        assert_eq!(
            unique.len(),
            registered.len(),
            "generate_handler must not register duplicate commands"
        );
        assert_eq!(registered, APP_COMMANDS);
    }

    #[test]
    fn trusted_capability_targets_only_main_webview() {
        let capability: serde_json::Value =
            serde_json::from_str(include_str!("../capabilities/default.json"))
                .expect("default capability must be valid json");

        assert_eq!(capability["webviews"], serde_json::json!(["main"]));
        assert!(capability.get("windows").is_none());
        assert!(capability.get("remote").is_none());
    }

    #[test]
    fn trusted_capability_grants_every_application_command_once() {
        let capability: serde_json::Value =
            serde_json::from_str(include_str!("../capabilities/default.json"))
                .expect("default capability must be valid json");
        let permissions = capability["permissions"]
            .as_array()
            .expect("permissions must be an array");

        let actual_app_permissions = permissions
            .iter()
            .filter_map(serde_json::Value::as_str)
            .filter(|permission| !permission.contains(':'))
            .collect::<Vec<_>>();
        let expected_app_permissions = APP_COMMANDS
            .iter()
            .map(|command| format!("allow-{}", command.replace('_', "-")))
            .collect::<Vec<_>>();
        assert_eq!(
            actual_app_permissions,
            expected_app_permissions
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            "trusted capability must contain exactly the registered app commands"
        );

        for command in APP_COMMANDS {
            let wanted = format!("allow-{}", command.replace('_', "-"));
            assert_eq!(
                permissions
                    .iter()
                    .filter(|value| value.as_str() == Some(wanted.as_str()))
                    .count(),
                1,
                "{wanted} must be granted exactly once"
            );
        }
    }

    #[test]
    fn browser_sandbox_matches_no_capability_file() {
        let capability_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("capabilities");
        let entries = std::fs::read_dir(capability_dir).expect("capabilities directory must exist");

        for entry in entries {
            let path = entry.expect("capability entry must be readable").path();
            if path.extension().and_then(|extension| extension.to_str()) != Some("json") {
                continue;
            }
            let capability: serde_json::Value = serde_json::from_slice(
                &std::fs::read(&path).expect("capability file must be readable"),
            )
            .expect("capability file must be valid json");
            for selector in ["windows", "webviews"] {
                let labels = capability
                    .get(selector)
                    .and_then(serde_json::Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(serde_json::Value::as_str)
                    .collect::<Vec<_>>();
                assert!(
                    !labels.iter().any(|label| {
                        *label == "browser-sandbox"
                            || label.starts_with("task-browser-")
                            || *label == "*"
                            || label.contains('*')
                    }),
                    "{} must not grant authority to remote Browser webviews via {selector}",
                    path.display()
                );
            }
        }
    }

    #[test]
    fn browser_workspace_commands_are_all_registered() {
        for command in [
            "browser_sandbox_focus",
            "browser_sandbox_back",
            "browser_sandbox_forward",
            "browser_sandbox_reload",
            "browser_sandbox_capture_text",
        ] {
            assert!(APP_COMMANDS.contains(&command), "missing {command}");
        }
    }
}
