//! Read-only inventory for model artifacts Plume can import later.

use serde::Serialize;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LocalModel {
    pub id: String,
    pub name: String,
    pub path: String,
    pub kind: LocalModelKind,
    pub size_bytes: u64,
    pub source: LocalModelSource,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum LocalModelKind {
    Gguf,
    Safetensors,
    /// HuggingFace-style transformer checkpoint folder: `config.json`,
    /// a `tokenizer*` file, and at least one weight file (`.safetensors`
    /// / `.gguf` / `.npz`). The heuristic does NOT prove the weights
    /// are MLX-format — a vanilla PyTorch download from `huggingface-cli`
    /// also satisfies it. A future slice can downgrade specific folders
    /// to a stricter `MlxFolder` variant after parsing `config.json` or
    /// detecting MLX-specific markers (e.g. `.npz` shards).
    TransformerFolder,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum LocalModelSource {
    PlumeModelDir,
}

pub fn default_model_dir() -> PathBuf {
    let current_dir = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    resolve_model_dir(
        current_dir,
        env::var_os("PLUME_MODEL_DIR").map(PathBuf::from),
    )
}

fn resolve_model_dir(current_dir: PathBuf, env_dir: Option<PathBuf>) -> PathBuf {
    match env_dir {
        Some(path) if path.is_absolute() => path,
        Some(path) => current_dir.join(path),
        None => current_dir.join("plume-models"),
    }
}

pub fn scan_model_dir(model_dir: &Path) -> Vec<LocalModel> {
    let mut models = Vec::new();
    scan_dir(model_dir, model_dir, &mut models);
    models.sort_by(|a, b| a.path.cmp(&b.path));
    models
}

fn scan_dir(root: &Path, dir: &Path, models: &mut Vec<LocalModel>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };

    let mut entries = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .collect::<Vec<_>>();
    entries.sort();

    for path in entries {
        let Ok(metadata) = fs::symlink_metadata(&path) else {
            continue;
        };
        if metadata.file_type().is_symlink() {
            continue;
        }
        if metadata.is_dir() {
            if let Some(model) = transformer_folder(root, &path) {
                models.push(model);
            } else {
                scan_dir(root, &path, models);
            }
            continue;
        }
        if !metadata.is_file() {
            continue;
        }
        let Some(kind) = file_kind(&path) else {
            continue;
        };
        models.push(local_model(root, &path, kind, metadata.len()));
    }
}

fn file_kind(path: &Path) -> Option<LocalModelKind> {
    match path.extension().and_then(|ext| ext.to_str()) {
        Some(ext) if ext.eq_ignore_ascii_case("gguf") => Some(LocalModelKind::Gguf),
        Some(ext) if ext.eq_ignore_ascii_case("safetensors") => Some(LocalModelKind::Safetensors),
        _ => None,
    }
}

fn transformer_folder(root: &Path, folder: &Path) -> Option<LocalModel> {
    if !folder.join("config.json").is_file() {
        return None;
    }

    let Ok(entries) = fs::read_dir(folder) else {
        return None;
    };

    let mut has_tokenizer = false;
    let mut has_model = false;
    let mut size_bytes = 0;

    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        let Ok(metadata) = fs::symlink_metadata(&path) else {
            continue;
        };
        if !metadata.is_file() {
            continue;
        }
        size_bytes += metadata.len();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        let lower = name.to_ascii_lowercase();
        if lower.starts_with("tokenizer") {
            has_tokenizer = true;
        }
        if lower.ends_with(".safetensors") || lower.ends_with(".gguf") || lower.ends_with(".npz") {
            has_model = true;
        }
    }

    if !has_tokenizer || !has_model {
        return None;
    }

    Some(local_model(
        root,
        folder,
        LocalModelKind::TransformerFolder,
        size_bytes,
    ))
}

fn local_model(root: &Path, path: &Path, kind: LocalModelKind, size_bytes: u64) -> LocalModel {
    let relative = path.strip_prefix(root).unwrap_or(path);
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("local-model")
        .to_string();
    LocalModel {
        id: relative.to_string_lossy().to_string(),
        name,
        path: path.to_string_lossy().to_string(),
        kind,
        size_bytes,
        source: LocalModelSource::PlumeModelDir,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TempDir {
        path: PathBuf,
    }

    impl TempDir {
        fn new(prefix: &str) -> Self {
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock")
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "plume-local-models-{prefix}-{}-{nanos}",
                std::process::id()
            ));
            fs::create_dir_all(&path).expect("create tempdir");
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

    #[test]
    fn scans_gguf_and_safetensors_files() {
        let td = TempDir::new("files");
        fs::write(td.path().join("tiny.gguf"), b"gguf").expect("write gguf");
        fs::write(td.path().join("adapter.safetensors"), b"safe").expect("write safetensors");
        fs::write(td.path().join("notes.txt"), b"ignore").expect("write ignored");

        let models = scan_model_dir(td.path());

        assert_eq!(models.len(), 2);
        assert_eq!(models[0].name, "adapter.safetensors");
        assert_eq!(models[0].kind, LocalModelKind::Safetensors);
        assert_eq!(models[0].size_bytes, 4);
        assert_eq!(models[1].name, "tiny.gguf");
        assert_eq!(models[1].kind, LocalModelKind::Gguf);
        assert_eq!(models[1].size_bytes, 4);
        assert!(models
            .iter()
            .all(|m| m.source == LocalModelSource::PlumeModelDir));
    }

    #[test]
    fn detects_transformer_folder_once() {
        let td = TempDir::new("transformer");
        let folder = td.path().join("qwen-mlx");
        fs::create_dir_all(&folder).expect("create model folder");
        fs::write(folder.join("config.json"), b"{}").expect("write config");
        fs::write(folder.join("tokenizer.json"), b"{}").expect("write tokenizer");
        fs::write(folder.join("model.safetensors"), b"weights").expect("write weights");

        let models = scan_model_dir(td.path());

        assert_eq!(models.len(), 1);
        assert_eq!(models[0].name, "qwen-mlx");
        assert_eq!(models[0].kind, LocalModelKind::TransformerFolder);
        assert_eq!(models[0].size_bytes, 2 + 2 + 7);
    }

    #[test]
    fn config_json_only_folder_is_not_detected() {
        let td = TempDir::new("config-only");
        let folder = td.path().join("naked");
        fs::create_dir_all(&folder).expect("create folder");
        fs::write(folder.join("config.json"), b"{}").expect("write config");

        let models = scan_model_dir(td.path());

        assert!(
            models.is_empty(),
            "config.json alone must not register as a model: {models:?}"
        );
    }

    #[test]
    fn tokenizer_without_weights_is_not_detected() {
        let td = TempDir::new("no-weights");
        let folder = td.path().join("tokenized");
        fs::create_dir_all(&folder).expect("create folder");
        fs::write(folder.join("config.json"), b"{}").expect("write config");
        fs::write(folder.join("tokenizer.json"), b"{}").expect("write tokenizer");

        let models = scan_model_dir(td.path());

        assert!(
            models.is_empty(),
            "config.json + tokenizer without a weight file must not register: {models:?}"
        );
    }

    /// Symlink skip is the security boundary of this verb — a malicious
    /// or sloppy `plume-models/` layout should not exfiltrate filenames
    /// from elsewhere on disk. We verify the negative case directly.
    #[cfg(unix)]
    #[test]
    fn symlinks_inside_model_dir_are_skipped() {
        use std::os::unix::fs::symlink;

        let td = TempDir::new("symlink");

        // Real target outside the model dir.
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let outside = std::env::temp_dir().join(format!(
            "plume-symlink-target-{}-{nanos}.gguf",
            std::process::id()
        ));
        fs::write(&outside, b"weights").expect("write outside target");

        // Symlink inside the model dir pointing at it.
        let link = td.path().join("link.gguf");
        symlink(&outside, &link).expect("create symlink");

        let models = scan_model_dir(td.path());

        // Cleanup the outside target before asserting so a failure
        // doesn't leak a tempfile.
        let _ = fs::remove_file(&outside);

        assert!(
            models.is_empty(),
            "symlink inside model dir must not surface: {models:?}"
        );
    }

    #[test]
    fn missing_model_dir_is_an_empty_library() {
        let td = TempDir::new("missing");
        let models = scan_model_dir(&td.path().join("not-created"));

        assert!(models.is_empty());
    }

    #[test]
    fn plume_model_dir_env_wins_over_default() {
        let current_dir = PathBuf::from("/project");
        let env_dir = PathBuf::from("/custom/models");

        let resolved = resolve_model_dir(current_dir, Some(env_dir.clone()));

        assert_eq!(resolved, env_dir);
    }

    #[test]
    fn relative_plume_model_dir_env_stays_under_current_project() {
        let current_dir = PathBuf::from("/project");

        let resolved = resolve_model_dir(current_dir, Some(PathBuf::from("models")));

        assert_eq!(resolved, PathBuf::from("/project/models"));
    }

    #[test]
    fn fallback_model_dir_lives_under_current_project() {
        let current_dir = PathBuf::from("/project");

        let resolved = resolve_model_dir(current_dir, None);

        assert_eq!(resolved, PathBuf::from("/project/plume-models"));
    }
}
