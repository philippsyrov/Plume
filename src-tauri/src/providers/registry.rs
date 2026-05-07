//! Static provider registry.
//!
//! D1 ships a hand-written list. When real adapters land each one will
//! contribute its own entry from inside its module; this function will
//! become a `Vec::extend` over those.

use super::{ProviderCapabilities, ProviderCategory, ProviderInfo};

/// Fixed provider list. The Tauri command returns this verbatim.
///
/// Capability values are deliberately conservative for D1 — `streaming
/// = false`, `tool_calls = None`, `max_context = 0` ("unknown"). Real
/// values land with each adapter so the picker UI can show honest
/// capability badges. See `docs/MODEL_PROVIDERS.md`.
pub fn default_providers() -> Vec<ProviderInfo> {
    vec![
        ProviderInfo {
            id: "mlx-lm".into(),
            display_name: "MLX-LM".into(),
            category: ProviderCategory::PlumeManaged,
            capabilities: ProviderCapabilities {
                owned_process: true,
                ..ProviderCapabilities::default()
            },
        },
        ProviderInfo {
            id: "ollama".into(),
            display_name: "Ollama".into(),
            // Ollama can flip to PlumeManaged once Plume itself starts
            // `ollama serve`. For D1 (no daemon supervision) it is
            // strictly Connected.
            category: ProviderCategory::Connected,
            capabilities: ProviderCapabilities::default(),
        },
        ProviderInfo {
            id: "lm-studio".into(),
            display_name: "LM Studio".into(),
            category: ProviderCategory::Connected,
            capabilities: ProviderCapabilities::default(),
        },
        ProviderInfo {
            id: "llama-cpp".into(),
            display_name: "llama.cpp".into(),
            category: ProviderCategory::PlumeManaged,
            capabilities: ProviderCapabilities {
                owned_process: true,
                ..ProviderCapabilities::default()
            },
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_has_expected_ids() {
        let providers = default_providers();
        let ids: Vec<&str> = providers.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(ids, ["mlx-lm", "ollama", "lm-studio", "llama-cpp"]);
    }

    #[test]
    fn registry_categories_match_runtime_doc() {
        // Cross-check with docs/MODEL_PROVIDERS.md § Runtime categories.
        // Plume-managed runtimes are the ones whose process Plume
        // intends to spawn; connected runtimes connect to a daemon
        // the user has already started.
        let by_id: std::collections::HashMap<_, _> = default_providers()
            .into_iter()
            .map(|p| (p.id.clone(), p.category))
            .collect();
        assert_eq!(by_id["mlx-lm"], ProviderCategory::PlumeManaged);
        assert_eq!(by_id["ollama"], ProviderCategory::Connected);
        assert_eq!(by_id["lm-studio"], ProviderCategory::Connected);
        assert_eq!(by_id["llama-cpp"], ProviderCategory::PlumeManaged);
    }

    #[test]
    fn owned_process_flag_aligns_with_category() {
        // Sanity contract: every PlumeManaged provider declares
        // `owned_process: true`. Connected providers declare false.
        // The capability flag is what process supervision reads;
        // category is what UI surfaces. They must agree.
        for p in default_providers() {
            match p.category {
                ProviderCategory::PlumeManaged => {
                    assert!(
                        p.capabilities.owned_process,
                        "{} is PlumeManaged but owned_process=false",
                        p.id
                    );
                }
                ProviderCategory::Connected => {
                    assert!(
                        !p.capabilities.owned_process,
                        "{} is Connected but owned_process=true",
                        p.id
                    );
                }
            }
        }
    }
}
