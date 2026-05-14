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

/// Maximum nesting depth the inventory walker will descend into
/// before stopping. Files at this depth are still surfaced; subdirs
/// at this depth are not entered. The cap is defensive — symlinks
/// are already skipped so there is no cycle risk, but a pathologically
/// nested model dir would otherwise let the scan run unbounded. Eight
/// levels is comfortably past every model layout we have seen in the
/// wild (typical: 1-3 levels for shards, 2-4 for Hugging Face cache
/// trees).
const MAX_SCAN_DEPTH: usize = 8;

pub fn scan_model_dir(model_dir: &Path) -> Vec<LocalModel> {
    let mut models = Vec::new();
    scan_dir(model_dir, model_dir, &mut models, 0);
    models.sort_by(|a, b| a.path.cmp(&b.path));
    models
}

fn scan_dir(root: &Path, dir: &Path, models: &mut Vec<LocalModel>, depth: usize) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };

    let mut entries = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .collect::<Vec<_>>();
    entries.sort();

    for path in entries {
        if is_noise_path(&path) {
            continue;
        }
        let Ok(metadata) = fs::symlink_metadata(&path) else {
            continue;
        };
        if metadata.file_type().is_symlink() {
            continue;
        }
        if metadata.is_dir() {
            if let Some(model) = transformer_folder(root, &path) {
                models.push(model);
            } else if depth < MAX_SCAN_DEPTH {
                scan_dir(root, &path, models, depth + 1);
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

/// Skip the most common filesystem noise — hidden dirs and metadata
/// files like `.git`, `.DS_Store`, `.cache`, dotfile configs. Any
/// entry whose final name component starts with `.` is treated as
/// noise. This is deliberately narrow: not a full ignore engine,
/// just a defence against the false positives we know about. A user
/// who deliberately stores weights inside a hidden directory has
/// opted out of being seen by the inventory.
fn is_noise_path(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(|name| name.starts_with('.'))
        .unwrap_or(false)
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
        if is_noise_path(&path) {
            continue;
        }
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

    /// A model file nested several levels deep — well within the
    /// configured cap — should still be discovered. Exercises the
    /// "normal nested-cache" case so the cap doesn't accidentally
    /// shadow legitimate layouts.
    #[test]
    fn nested_models_within_depth_cap_are_found() {
        let td = TempDir::new("nested-within");

        // Build a chain of `MAX_SCAN_DEPTH` nested folders and drop a
        // .gguf at the deepest level. The deepest dir is at depth N,
        // which `scan_dir` enters because `depth < MAX_SCAN_DEPTH` is
        // checked before recursing.
        let mut deep = td.path().to_path_buf();
        for i in 0..MAX_SCAN_DEPTH {
            deep = deep.join(format!("lvl-{i}"));
        }
        fs::create_dir_all(&deep).expect("create nested chain");
        fs::write(deep.join("nested.gguf"), b"wgts").expect("write nested gguf");

        let models = scan_model_dir(td.path());

        assert_eq!(models.len(), 1, "expected one nested model: {models:?}");
        assert_eq!(models[0].name, "nested.gguf");
        assert_eq!(models[0].kind, LocalModelKind::Gguf);
    }

    /// One level past `MAX_SCAN_DEPTH` the scanner refuses to descend,
    /// so the model file is silently invisible. The verb is read-only
    /// inventory, so "ignored" is the right failure mode — not an
    /// error.
    #[test]
    fn models_beyond_depth_cap_are_ignored() {
        let td = TempDir::new("nested-beyond");

        // One step past the cap: depth = MAX_SCAN_DEPTH + 1. The
        // parent dir at that depth would only be entered from a
        // scan_dir running at depth == MAX_SCAN_DEPTH, which the
        // guard explicitly rejects.
        let mut deep = td.path().to_path_buf();
        for i in 0..=MAX_SCAN_DEPTH {
            deep = deep.join(format!("lvl-{i}"));
        }
        fs::create_dir_all(&deep).expect("create over-cap chain");
        fs::write(deep.join("too-deep.gguf"), b"wgts").expect("write deep gguf");

        let models = scan_model_dir(td.path());

        assert!(
            models.is_empty(),
            "model file past the depth cap must not surface: {models:?}"
        );
    }

    /// `.git`, `.DS_Store`, `.cache`, and other dot-prefixed entries
    /// are filesystem noise. They must not be recursed into and must
    /// not pollute the result, even when they contain extensions the
    /// scanner would otherwise recognise.
    #[test]
    fn dot_entries_are_skipped() {
        let td = TempDir::new("dotnoise");

        // .git/ subdir with a .gguf file inside — must not be
        // recursed into.
        let dot_git = td.path().join(".git");
        fs::create_dir_all(&dot_git).expect("create .git");
        fs::write(dot_git.join("hidden.gguf"), b"wgts").expect("write inside .git");

        // .cache/ subdir with a .safetensors file inside — same.
        let dot_cache = td.path().join(".cache");
        fs::create_dir_all(&dot_cache).expect("create .cache");
        fs::write(dot_cache.join("hidden.safetensors"), b"wgts").expect("write inside .cache");

        // Top-level .DS_Store — must not be reported as a model
        // (no recognised extension), and must not crash the scan.
        fs::write(td.path().join(".DS_Store"), b"junk").expect("write .DS_Store");

        // Real model at the root for the positive control.
        fs::write(td.path().join("model.gguf"), b"weights").expect("write real model");

        let models = scan_model_dir(td.path());

        assert_eq!(
            models.len(),
            1,
            "only the non-dot model should surface: {models:?}"
        );
        assert_eq!(models[0].name, "model.gguf");
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
