//! Shared public entries into the MLX supervisor lifecycle.

use std::path::Path;

use super::{supervisor, MlxCommand, ServerHandle, ServerStartOptions, StartError};

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
