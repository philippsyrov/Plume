//! `providers.list` and `providers.health` Tauri command handlers.
//!
//! See `docs/IPC_CONTRACT.md` § providers for the wire shape and
//! `docs/MODEL_PROVIDERS.md § Runtime categories` for what the
//! `category` field means.
//!
//! Neither verb requires an open project. The provider registry is
//! global, and reachability is global state about local daemons. UI
//! surfaces them inside the project view, but the backend doesn't
//! gate them on trust.

use crate::commands::project::EmptyPayload;
use crate::error::{IpcError, IpcRequest};
use crate::providers::{default_providers, probe_all, ProviderHealth, ProviderInfo};

#[tauri::command]
pub async fn providers_list(req: IpcRequest<EmptyPayload>) -> Result<Vec<ProviderInfo>, IpcError> {
    req.check_version()?;
    Ok(default_providers())
}

#[tauri::command]
pub async fn providers_health(
    req: IpcRequest<EmptyPayload>,
) -> Result<Vec<ProviderHealth>, IpcError> {
    req.check_version()?;
    Ok(probe_all().await)
}
