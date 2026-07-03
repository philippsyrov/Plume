//! Launch shape for the D40 supervisor: ephemeral port allocation
//! plus the `python -m mlx_lm server …` command builder. Extracted
//! from `process.rs` (D117); consumed only by `try_start_once` and
//! the test suite. No registry or lifecycle state lives here.

use std::net::TcpListener;

/// Allocate an ephemeral TCP port on 127.0.0.1. Binds, reads the
/// OS-assigned port, then immediately drops the listener so the
/// child can rebind. Returns an `io::Error` if the bind itself
/// fails — that's a system-level failure (no free ports, etc),
/// not the race window.
///
/// The race window between drop and the child's bind is documented
/// in `docs/MLX_RUNTIME.md § Port allocation`; `start_server`
/// addresses it by retrying once on a health-probe timeout.
pub fn allocate_port() -> std::io::Result<u16> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let port = listener.local_addr()?.port();
    drop(listener);
    Ok(port)
}

/// Non-deprecated mlx-lm launcher per `docs/MLX_RUNTIME.md § CLI`.
/// Splits the program (which we want testable) from the
/// args-prefix (the subcommand shape `mlx-lm` requires). Tests
/// override `program` to a path that does not need `mlx-lm`
/// installed.
#[derive(Debug, Clone)]
pub struct MlxLmCommand {
    /// Executable to run. Production default: `"python"` resolved
    /// via `$PATH`, but honors the D58 `PLUME_MLX_PYTHON` env var
    /// when set (typically an absolute path to a venv interpreter
    /// like `~/.venvs/mlx-env/bin/python`). Tests: an absolute
    /// path to a binary that can read the same args (e.g.
    /// `/usr/bin/python3` for a fake HTTP server, or `/bin/sleep`
    /// for shutdown-only tests).
    pub program: String,
    /// Args inserted before the `--model` / `--host` / `--port`
    /// args `build_command_args` produces. Production:
    /// `["-m", "mlx_lm", "server"]`. Tests: whatever args their
    /// fake binary expects (often empty for a `sleep N` stub).
    pub args_prefix: Vec<String>,
}

/// The production launcher: `python -m mlx_lm server …`. See the
/// rationale on the subcommand form in `docs/MLX_RUNTIME.md`.
///
/// D58: the `program` field is resolved via `resolve_python_program`,
/// which honors the `PLUME_MLX_PYTHON` env override. The user can
/// set `PLUME_MLX_PYTHON=~/.venvs/mlx-env/bin/python` so Plume's
/// supervisor spawns the venv's interpreter directly — no
/// LaunchServices / PATH magic required. Args stay
/// `-m mlx_lm server` regardless of the program.
pub fn default_mlx_lm_command() -> MlxLmCommand {
    MlxLmCommand {
        program: resolve_python_program(),
        args_prefix: vec!["-m".into(), "mlx_lm".into(), "server".into()],
    }
}

/// D58: pick the Python interpreter to invoke `mlx_lm.server` under.
///
/// Resolution order:
///
/// 1. `PLUME_MLX_PYTHON` env var, if set AND non-empty after `trim`.
///    The value is taken verbatim — typically an absolute path like
///    `~/.venvs/mlx-env/bin/python` (the shell-expanded form). We do
///    NOT expand `~` ourselves; that's the calling shell's job.
/// 2. Bare `"python"` (resolved via `$PATH` at spawn time) otherwise.
///    Matches the pre-D58 default; an mlx-lm install on the user's
///    normal PATH continues to work without setting the env var.
///
/// Validation posture: we only filter "unset" and "empty after trim".
/// We do NOT check that the resolved path is executable, exists, or
/// has `mlx_lm` importable — `Command::spawn` will surface those as
/// a clear `StartError::Spawn(io::Error)` with the OS message
/// ("No such file or directory", "permission denied", etc.), which
/// the IPC layer already maps to a useful error string. Pre-
/// checking here would be racy (TOCTOU) and duplicate the work.
fn resolve_python_program() -> String {
    if let Some(raw) = std::env::var_os("PLUME_MLX_PYTHON") {
        let s = raw.to_string_lossy();
        let trimmed = s.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }
    "python".to_string()
}

/// Compose the trailing args for an mlx-lm-style chat server:
/// `--model PATH --host 127.0.0.1 --port N --log-level LEVEL`.
/// Pulled out so tests can assert on the exact shape without
/// running the supervisor. The `model_path` is rendered with
/// `to_string_lossy` so a path containing a non-UTF-8 byte still
/// goes through (the kernel won't lose it).
pub fn build_command_args(model_path: &std::path::Path, port: u16, log_level: &str) -> Vec<String> {
    vec![
        "--model".to_string(),
        model_path.to_string_lossy().into_owned(),
        "--host".to_string(),
        "127.0.0.1".to_string(),
        "--port".to_string(),
        port.to_string(),
        "--log-level".to_string(),
        log_level.to_string(),
    ]
}
