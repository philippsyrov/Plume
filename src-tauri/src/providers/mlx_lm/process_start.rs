//! Shared public entries into the MLX supervisor lifecycle.

use std::path::Path;

use super::{supervisor, MlxCommand, ServerHandle, ServerStartOptions, StartError, Supervisor};

/// Start an MLX server from the generic supervisor options. A health timeout
/// retries once inside the supervisor; reservation, cap, child cleanup, and
/// diagnostics all remain owned by `Supervisor::start_server`.
pub fn start_server(options: ServerStartOptions) -> Result<ServerHandle, StartError> {
    supervisor().start_server(options)
}

/// Start a server with an already-resolved interpreter. Both the generic
/// local-model command and the receipt-backed catalog command enter through
/// this wrapper, so they retain the same reservation, cap, health, recovery,
/// diagnostics, and shutdown lifecycle below path resolution.
pub fn start_server_with_command(
    command: MlxCommand,
    model_path: &Path,
    inventory_model_id: &str,
) -> Result<ServerHandle, StartError> {
    start_server(ServerStartOptions {
        model_path: model_path.to_path_buf(),
        command: Some(command),
        log_level: "INFO".to_string(),
        startup_timeout: None,
        model_id: inventory_model_id.to_string(),
    })
}

/// Start with a callback that runs after the supervisor's `Starting` slot has
/// landed but before its health poll. Catalog start uses it to release its
/// filesystem lifecycle gate only once removal is protected by the supervisor.
pub fn start_server_with_command_after_reservation(
    command: MlxCommand,
    model_path: &Path,
    inventory_model_id: &str,
    after_reservation: impl FnOnce(),
) -> Result<ServerHandle, StartError> {
    let options = ServerStartOptions {
        model_path: model_path.to_path_buf(),
        command: Some(command),
        log_level: "INFO".to_string(),
        startup_timeout: None,
        model_id: inventory_model_id.to_string(),
    };
    if options.model_path.as_os_str().is_empty() {
        return Err(StartError::InvalidModelPath);
    }
    // A retry after an unhealthy child would need a fresh catalog validation
    // and lifecycle guard. The catalog path therefore makes one guarded
    // attempt; generic starts retain their existing retry through `start_server`.
    supervisor().try_start_once_after_reservation(options, after_reservation)
}

impl Supervisor {
    pub(crate) fn start_server(
        &self,
        options: ServerStartOptions,
    ) -> Result<ServerHandle, StartError> {
        if options.model_path.as_os_str().is_empty() {
            return Err(StartError::InvalidModelPath);
        }
        let attempt1 = self.try_start_once(options.clone());
        match attempt1 {
            Ok(handle) => Ok(handle),
            Err(StartError::HealthTimeout { .. }) => self.try_start_once(options),
            Err(other) => Err(other),
        }
    }
}
