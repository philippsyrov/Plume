//! Exact model capability gate for screenshot evidence.

use std::time::Duration;

use crate::error::IpcError;
use crate::providers::catalog::QWEN2_VL_CATALOG_ID;

use super::{OLLAMA_HOST, OLLAMA_PORT};

pub(super) async fn require_screenshot_support(
    provider_id: Option<&str>,
    model_id: Option<&str>,
) -> Result<(), IpcError> {
    if provider_id == Some("mlx-vlm") && model_id == Some(QWEN2_VL_CATALOG_ID) {
        return Ok(());
    }
    if provider_id != Some("ollama") {
        return Err(IpcError::Blocked(
            "This model cannot use screenshots.".into(),
        ));
    }
    let model_id = model_id
        .map(str::trim)
        .filter(|model| !model.is_empty())
        .ok_or_else(|| IpcError::Blocked("This model cannot use screenshots.".into()))?
        .to_string();
    let details = tauri::async_runtime::spawn_blocking(move || {
        crate::providers::ollama::probe_model_details(
            OLLAMA_HOST,
            OLLAMA_PORT,
            &model_id,
            Duration::from_secs(2),
        )
    })
    .await
    .map_err(|_| IpcError::Blocked("Could not verify screenshot support.".into()))?
    .map_err(|_| IpcError::Blocked("Could not verify screenshot support.".into()))?;
    if capabilities_include_vision(&details.capabilities) {
        Ok(())
    } else {
        Err(IpcError::Blocked(
            "This model cannot use screenshots.".into(),
        ))
    }
}

fn capabilities_include_vision(capabilities: &[String]) -> bool {
    capabilities
        .iter()
        .any(|capability| capability.eq_ignore_ascii_case("vision"))
}

#[cfg(test)]
mod tests {
    use super::capabilities_include_vision;

    #[test]
    fn vision_support_comes_only_from_the_exact_reported_capability() {
        assert!(capabilities_include_vision(&[
            "completion".into(),
            "vision".into()
        ]));
        assert!(capabilities_include_vision(&["VISION".into()]));
        assert!(!capabilities_include_vision(&[]));
        assert!(!capabilities_include_vision(&["completion".into()]));
        assert!(!capabilities_include_vision(&["visionary".into()]));
    }
}
