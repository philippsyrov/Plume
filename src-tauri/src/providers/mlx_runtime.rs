//! Resolve the MLX interpreter without allowing release builds to fall back
//! to a caller-controlled PATH entry.

use std::path::Path;

use super::mlx_lm::process::{configured_mlx_python_program, default_mlx_lm_command, MlxCommand};

const BUNDLED_INTERPRETER: &str = "mlx-runtime/bin/python3";

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub(crate) enum RuntimeError {
    #[error("the bundled MLX runtime is missing or is not a regular interpreter file")]
    BundledRuntimeMissing,
}

pub(crate) fn resolve_mlx_runtime(
    resource_dir: &Path,
    debug_build: bool,
) -> Result<MlxCommand, RuntimeError> {
    // Development keeps the explicit documented override first, so contributors
    // can test an external venv without assembling a release payload.
    if debug_build {
        if let Some(program) = configured_mlx_python_program() {
            return Ok(MlxCommand {
                program,
                args_prefix: vec!["-m".into(), "mlx_lm".into(), "server".into()],
            });
        }
    }

    let bundled = resource_dir.join(BUNDLED_INTERPRETER);
    let metadata =
        std::fs::symlink_metadata(&bundled).map_err(|_| RuntimeError::BundledRuntimeMissing)?;
    if metadata.is_file() && !metadata.file_type().is_symlink() {
        return Ok(MlxCommand {
            program: bundled,
            args_prefix: vec!["-m".into(), "mlx_lm".into(), "server".into()],
        });
    }

    // Debug builds preserve the pre-bundle developer fallback. Release builds
    // never reach this branch, so Finder/LaunchServices PATH cannot choose the
    // interpreter that runs an app-level catalog model.
    if debug_build {
        return Ok(default_mlx_lm_command());
    }

    Err(RuntimeError::BundledRuntimeMissing)
}
