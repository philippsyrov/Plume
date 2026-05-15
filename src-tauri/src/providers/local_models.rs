//! Read-only inventory for model artifacts Plume can import later.

use serde::Serialize;
use std::env;
use std::fs;
use std::io::Read;
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
    /// also satisfies it. D36 added the stricter `MlxFolder` variant
    /// below for folders that carry actual MLX evidence; the absence
    /// of that evidence keeps a folder in this category.
    TransformerFolder,
    /// Transformer-folder shape AND verified MLX evidence — either a
    /// top-level `weights.npz` shard (legacy MLX format) or a
    /// `config.json` carrying an `{"quantization": {"group_size": _,
    /// "bits": _}}` object (the MLX-LM quantization shape). D36 added
    /// this as a STRICTER classification: every `mlx-folder` is also
    /// a transformer-folder in layout. The product rule is "Plume
    /// must not label a model as MLX unless it has checked enough on
    /// disk to justify that claim" — see
    /// `docs/LOCAL_AGENT_NORTH_STAR.md § MLX-first`.
    MlxFolder,
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

/// Deepest entry depth (root-relative) the inventory walker will
/// surface. Walkdir / GNU `find -maxdepth` semantics: the model
/// directory itself is depth 0, its immediate children are depth 1,
/// and so on. Entries strictly past `MAX_SCAN_DEPTH` are invisible
/// — files, plain folders, and transformer folders alike. The cap
/// is defensive: symlinks are already skipped (no cycle risk), but
/// a pathologically nested model dir would otherwise let the scan
/// run unbounded. Eight levels is comfortably past every model
/// layout we have seen in the wild (typical: 1-3 for shards, 2-4
/// for Hugging Face cache trees).
const MAX_SCAN_DEPTH: usize = 8;

pub fn scan_model_dir(model_dir: &Path) -> Vec<LocalModel> {
    let mut models = Vec::new();
    // Immediate children of `model_dir` live at depth 1; the model
    // dir itself is depth 0 (never surfaced).
    scan_dir(model_dir, model_dir, &mut models, 1);
    models.sort_by(|a, b| a.path.cmp(&b.path));
    models
}

/// `depth` is the root-relative depth of the entries being iterated
/// by this call (NOT the depth of `dir`). The early-return is the
/// single, symmetric cap: it gates files, plain folders, and
/// transformer-folder detection together, eliminating the "folders
/// past the cap can still be detected" hole the previous shape had.
fn scan_dir(root: &Path, dir: &Path, models: &mut Vec<LocalModel>, depth: usize) {
    if depth > MAX_SCAN_DEPTH {
        return;
    }

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
            } else {
                // Recursion is unconditional; the early-return at
                // the top of the recursed call handles the cap so
                // every kind of entry is gated by the same rule.
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
    let mut has_weights_npz = false;
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
        if lower == "weights.npz" {
            has_weights_npz = true;
        }
    }

    if !has_tokenizer || !has_model {
        return None;
    }

    // D36: upgrade to `MlxFolder` only when we can prove MLX. Two
    // signals, either sufficient:
    //   * `weights.npz` at the folder root — the legacy MLX-LM
    //     shape produced by older converters and some community
    //     uploads.
    //   * `config.json` carrying a top-level `quantization` object
    //     with both `group_size` and `bits` keys — the shape
    //     `mlx-lm` writes for quantized models. HuggingFace /
    //     bitsandbytes use the DIFFERENT key `quantization_config`,
    //     which deliberately does not satisfy the check.
    // Unquantized MLX safetensors uploads can be on-disk-identical
    // to a vanilla HF safetensors upload; those stay classified as
    // `TransformerFolder` rather than risk a false-positive MLX
    // claim. See `docs/LOCAL_AGENT_NORTH_STAR.md § MLX-first`.
    let kind = if has_weights_npz || config_json_has_mlx_quantization(folder) {
        LocalModelKind::MlxFolder
    } else {
        LocalModelKind::TransformerFolder
    };

    Some(local_model(root, folder, kind, size_bytes))
}

/// Max bytes of `config.json` we'll try to parse for MLX evidence.
/// Real configs are well under 50 KiB; the 256 KiB cap is generous
/// while still bounding the worst case (a hostile or corrupt file
/// pointing at a huge inline schema).
const CONFIG_JSON_BYTE_CAP: u64 = 256 * 1024;

/// Returns `true` iff `<folder>/config.json` parses as JSON and
/// contains a top-level `quantization` object with both `group_size`
/// (a positive integer) and `bits` (a positive integer) keys. All
/// other shapes — missing file, oversize file, malformed JSON, key
/// present but the wrong type, key named `quantization_config`
/// instead — return `false` without surfacing an error.
///
/// Safety: read is bounded by `CONFIG_JSON_BYTE_CAP` and we never
/// panic on parser output. The folder path itself comes from the
/// scanner's symlink-skipping walker, so we don't have to re-check
/// `is_symlink` here.
fn config_json_has_mlx_quantization(folder: &Path) -> bool {
    let path = folder.join("config.json");
    let Ok(meta) = fs::metadata(&path) else {
        return false;
    };
    if !meta.is_file() {
        return false;
    }
    if meta.len() > CONFIG_JSON_BYTE_CAP {
        return false;
    }
    let Ok(mut file) = fs::File::open(&path) else {
        return false;
    };
    // Belt-and-braces: `take` caps the read even if the file grew
    // between the metadata stat and the open.
    let mut buf = String::new();
    if file
        .by_ref()
        .take(CONFIG_JSON_BYTE_CAP)
        .read_to_string(&mut buf)
        .is_err()
    {
        return false;
    }
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&buf) else {
        return false;
    };
    let Some(q) = value.get("quantization").and_then(|v| v.as_object()) else {
        return false;
    };
    let group_size_ok = q
        .get("group_size")
        .and_then(|v| v.as_u64())
        .is_some_and(|n| n > 0);
    let bits_ok = q
        .get("bits")
        .and_then(|v| v.as_u64())
        .is_some_and(|n| n > 0);
    group_size_ok && bits_ok
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

    /// A model file at exactly `MAX_SCAN_DEPTH` — the deepest entry
    /// depth the cap still admits — should be discovered. Exercises
    /// the "normal nested-cache" case so the cap doesn't accidentally
    /// shadow legitimate layouts.
    #[test]
    fn nested_models_within_depth_cap_are_found() {
        let td = TempDir::new("nested-within");

        // Chain of `MAX_SCAN_DEPTH - 1` nested folders places the
        // weight file at exactly depth `MAX_SCAN_DEPTH` (model_dir
        // is depth 0; each `join` adds one). Under walkdir-style
        // semantics that is the deepest admitted entry depth.
        let mut deep = td.path().to_path_buf();
        for i in 0..(MAX_SCAN_DEPTH - 1) {
            deep = deep.join(format!("lvl-{i}"));
        }
        fs::create_dir_all(&deep).expect("create nested chain");
        fs::write(deep.join("nested.gguf"), b"wgts").expect("write nested gguf");

        let models = scan_model_dir(td.path());

        assert_eq!(models.len(), 1, "expected one nested model: {models:?}");
        assert_eq!(models[0].name, "nested.gguf");
        assert_eq!(models[0].kind, LocalModelKind::Gguf);
    }

    /// One level past the cap — depth `MAX_SCAN_DEPTH + 1` — the
    /// weight file is silently invisible. The verb is read-only
    /// inventory, so "ignored" is the right failure mode, not an
    /// error.
    #[test]
    fn models_beyond_depth_cap_are_ignored() {
        let td = TempDir::new("nested-beyond");

        // Chain of `MAX_SCAN_DEPTH` nested folders puts the weight
        // file at depth `MAX_SCAN_DEPTH + 1` — one past the cap.
        let mut deep = td.path().to_path_buf();
        for i in 0..MAX_SCAN_DEPTH {
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

    /// Pin the symmetry the previous shape lacked: a transformer
    /// folder past the cap must be invisible, the same as a plain
    /// folder past the cap. The earlier code ran the
    /// `transformer_folder` check before the depth gate, so a
    /// well-formed folder one step past the cap could still be
    /// reported. The walkdir-style early-return at the top of
    /// `scan_dir` is what makes this rule consistent.
    #[test]
    fn transformer_folders_beyond_depth_cap_are_ignored() {
        let td = TempDir::new("transformer-beyond");

        // Chain of `MAX_SCAN_DEPTH` plain folders, then a fully-
        // formed transformer folder at the end. The folder itself
        // is at depth `MAX_SCAN_DEPTH + 1`.
        let mut parent = td.path().to_path_buf();
        for i in 0..MAX_SCAN_DEPTH {
            parent = parent.join(format!("lvl-{i}"));
        }
        let folder = parent.join("too-deep-transformer");
        fs::create_dir_all(&folder).expect("create over-cap transformer folder");
        fs::write(folder.join("config.json"), b"{}").expect("write config");
        fs::write(folder.join("tokenizer.json"), b"{}").expect("write tokenizer");
        fs::write(folder.join("model.safetensors"), b"wgts").expect("write weights");

        let models = scan_model_dir(td.path());

        assert!(
            models.is_empty(),
            "transformer folder past the depth cap must not surface: {models:?}"
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

    // ─── D36: verified MLX detection ────────────────────────────────────────

    /// MLX-LM's quantization shape: top-level `quantization` object
    /// with `group_size` + `bits`. A folder carrying it gets the
    /// stricter `MlxFolder` classification.
    #[test]
    fn detects_mlx_folder_via_config_json_quantization() {
        let td = TempDir::new("mlx-config");
        let folder = td.path().join("qwen-4bit-mlx");
        fs::create_dir_all(&folder).expect("create folder");
        fs::write(
            folder.join("config.json"),
            br#"{"model_type":"qwen2","quantization":{"group_size":64,"bits":4}}"#,
        )
        .expect("write mlx config");
        fs::write(folder.join("tokenizer.json"), b"{}").expect("write tokenizer");
        fs::write(folder.join("model.safetensors"), b"wgts").expect("write weights");

        let models = scan_model_dir(td.path());
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].kind, LocalModelKind::MlxFolder);
    }

    /// Legacy MLX shape: a top-level `weights.npz` shard. Even when
    /// `config.json` is empty / vanilla, the .npz file is sufficient
    /// evidence to upgrade the classification.
    #[test]
    fn detects_mlx_folder_via_weights_npz_file() {
        let td = TempDir::new("mlx-npz");
        let folder = td.path().join("legacy-mlx");
        fs::create_dir_all(&folder).expect("create folder");
        fs::write(folder.join("config.json"), b"{}").expect("write config");
        fs::write(folder.join("tokenizer.json"), b"{}").expect("write tokenizer");
        fs::write(folder.join("weights.npz"), b"npz").expect("write weights.npz");

        let models = scan_model_dir(td.path());
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].kind, LocalModelKind::MlxFolder);
    }

    /// Vanilla HuggingFace safetensors checkpoint — no quantization,
    /// no .npz. Must stay classified as `TransformerFolder`. The
    /// product rule forbids labeling this MLX; pinning that here.
    #[test]
    fn vanilla_hf_safetensors_folder_stays_transformer_folder() {
        let td = TempDir::new("hf-vanilla");
        let folder = td.path().join("qwen-fp16");
        fs::create_dir_all(&folder).expect("create folder");
        fs::write(
            folder.join("config.json"),
            br#"{"model_type":"qwen2","architectures":["Qwen2ForCausalLM"]}"#,
        )
        .expect("write hf config");
        fs::write(folder.join("tokenizer.json"), b"{}").expect("write tokenizer");
        fs::write(folder.join("model.safetensors"), b"wgts").expect("write weights");

        let models = scan_model_dir(td.path());
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].kind, LocalModelKind::TransformerFolder);
    }

    /// Bitsandbytes / HF quantization uses the key `quantization_config`,
    /// NOT `quantization`. The MLX upgrade must not trigger on this —
    /// it's the high-value false-positive case.
    #[test]
    fn hf_quantization_config_key_does_not_trigger_mlx() {
        let td = TempDir::new("hf-bnb");
        let folder = td.path().join("qwen-bnb-4bit");
        fs::create_dir_all(&folder).expect("create folder");
        fs::write(
            folder.join("config.json"),
            br#"{"model_type":"qwen2","quantization_config":{"load_in_4bit":true,"bnb_4bit_quant_type":"nf4"}}"#,
        )
        .expect("write bnb config");
        fs::write(folder.join("tokenizer.json"), b"{}").expect("write tokenizer");
        fs::write(folder.join("model.safetensors"), b"wgts").expect("write weights");

        let models = scan_model_dir(td.path());
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].kind, LocalModelKind::TransformerFolder);
    }

    /// Partial MLX-style quantization shape (only `bits`, missing
    /// `group_size`, or vice versa) does NOT satisfy the check. The
    /// detector requires BOTH keys with positive integer values.
    #[test]
    fn partial_quantization_shape_does_not_trigger_mlx() {
        let td = TempDir::new("partial-quant");
        let folder = td.path().join("partial");
        fs::create_dir_all(&folder).expect("create folder");
        fs::write(
            folder.join("config.json"),
            br#"{"quantization":{"bits":4}}"#,
        )
        .expect("write partial config");
        fs::write(folder.join("tokenizer.json"), b"{}").expect("write tokenizer");
        fs::write(folder.join("model.safetensors"), b"wgts").expect("write weights");

        let models = scan_model_dir(td.path());
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].kind, LocalModelKind::TransformerFolder);
    }

    /// Malformed `config.json` — not valid JSON. The folder still
    /// has the transformer shape (`config.json` + tokenizer + a
    /// weight), so it surfaces as `TransformerFolder`; the MLX
    /// check returns `false` without panicking.
    #[test]
    fn malformed_config_json_falls_back_to_transformer_folder() {
        let td = TempDir::new("bad-json");
        let folder = td.path().join("broken");
        fs::create_dir_all(&folder).expect("create folder");
        fs::write(folder.join("config.json"), b"this is not json {{{ ::: \xff")
            .expect("write malformed config");
        fs::write(folder.join("tokenizer.json"), b"{}").expect("write tokenizer");
        fs::write(folder.join("model.safetensors"), b"wgts").expect("write weights");

        let models = scan_model_dir(td.path());
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].kind, LocalModelKind::TransformerFolder);
    }

    /// A `config.json` larger than `CONFIG_JSON_BYTE_CAP` is not
    /// parsed even if it WOULD have the MLX shape. The folder
    /// surfaces as `TransformerFolder`. Pin this so a hostile file
    /// can't make the scan stall on a multi-megabyte JSON parse.
    #[test]
    fn oversize_config_json_skips_mlx_check() {
        let td = TempDir::new("huge-config");
        let folder = td.path().join("huge");
        fs::create_dir_all(&folder).expect("create folder");
        // 384 KiB of valid JSON (over the 256 KiB cap), with the
        // MLX shape buried inside. The cap kicks in BEFORE the
        // parse, so the shape never registers.
        let mut padded =
            String::from("{\"quantization\":{\"group_size\":64,\"bits\":4},\"padding\":\"");
        padded.push_str(&"x".repeat(384 * 1024));
        padded.push_str("\"}");
        fs::write(folder.join("config.json"), padded.as_bytes()).expect("write huge config");
        fs::write(folder.join("tokenizer.json"), b"{}").expect("write tokenizer");
        fs::write(folder.join("model.safetensors"), b"wgts").expect("write weights");

        let models = scan_model_dir(td.path());
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].kind, LocalModelKind::TransformerFolder);
    }

    /// The serde-kebab rename pins the wire format. `MlxFolder`
    /// becomes `"mlx-folder"` on the JSON side — that's what
    /// `docs/IPC_CONTRACT.md` documents and what the TS layer
    /// branches on.
    #[test]
    fn mlx_folder_kind_serializes_kebab_case() {
        let json = serde_json::to_string(&LocalModelKind::MlxFolder).expect("serialize");
        assert_eq!(json, "\"mlx-folder\"");
    }
}
