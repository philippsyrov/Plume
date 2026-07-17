use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use super::{
    mlx_lm::process::mlx_python_env_lock,
    mlx_runtime::{resolve_mlx_runtime, RuntimeError},
};

static TEMP_DIR_SEQUENCE: AtomicUsize = AtomicUsize::new(0);

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(label: &str) -> Self {
        let sequence = TEMP_DIR_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "plume-mlx-runtime-{label}-{}-{sequence}",
            std::process::id()
        ));
        fs::create_dir_all(&path).expect("create isolated runtime fixture");
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn fake_bundle_with_runtime() -> TempDir {
    let bundle = TempDir::new("bundle");
    let interpreter = bundle.path().join("mlx-runtime/bin/python3");
    fs::create_dir_all(interpreter.parent().expect("interpreter parent"))
        .expect("create runtime bin directory");
    fs::write(&interpreter, b"fixture interpreter").expect("write fixture interpreter");
    bundle
}

#[test]
fn release_prefers_bundled_interpreter_and_never_path_python() {
    let bundle = fake_bundle_with_runtime();

    let command = resolve_mlx_runtime(bundle.path(), false).expect("bundle resolves in release");

    assert_eq!(
        command.program,
        bundle.path().join("mlx-runtime/bin/python3"),
        "release may only launch the packaged interpreter"
    );
    assert_eq!(command.args_prefix, vec!["-m", "mlx_lm", "server"]);
}

#[test]
fn release_without_bundle_fails_closed() {
    let missing = TempDir::new("missing");

    let result = resolve_mlx_runtime(missing.path(), false);

    assert!(matches!(result, Err(RuntimeError::BundledRuntimeMissing)));
}

#[test]
fn debug_accepts_the_explicit_python_override_but_release_ignores_it() {
    let _guard = mlx_python_env_lock()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let bundle = fake_bundle_with_runtime();
    let override_path = PathBuf::from("/tmp/plume-debug-mlx-python");
    std::env::set_var("PLUME_MLX_PYTHON", &override_path);

    let debug = resolve_mlx_runtime(bundle.path(), true).expect("debug override resolves");
    let release = resolve_mlx_runtime(bundle.path(), false).expect("release bundle resolves");

    std::env::remove_var("PLUME_MLX_PYTHON");
    assert_eq!(debug.program, override_path);
    assert_eq!(
        release.program,
        bundle.path().join("mlx-runtime/bin/python3"),
        "release must ignore the development override"
    );
}
