//! Read-only inventory for model artifacts Plume can import later.
//!
//! D50 expands the inventory beyond `PLUME_MODEL_DIR`. Plume now reads
//! a small set of read-only "known sources" so models the user has
//! already downloaded through other local apps surface in the panel.
//! Each entry's `source` field names which root it came from; ids are
//! source-prefixed (`<source-tag>:<relative-path>`) so two sources
//! with an identically named subfolder do not collide on the wire.
//!
//! Today's sources:
//!
//! * `plume-model-dir` — `$PLUME_MODEL_DIR` (default
//!   `<cwd>/plume-models`). Primary; always scanned. Plume's own
//!   download dir when the download verb lands.
//! * `locally-ai-cache` — Locally AI's sandboxed HuggingFace cache at
//!   `~/Library/Containers/app.locallyai.Locally/Data/Library/
//!   app.locallyai.Locally/huggingface/models`. Read-only.
//! * `lm-studio-cache` — LM Studio's flat models dir at
//!   `~/.lmstudio/models`. Read-only.
//!
//! Ollama's blob store (`~/.ollama/models/blobs`) is intentionally NOT
//! a source. Ollama keeps weights as content-addressed blobs with the
//! human-readable model id only in its own SQLite manifest; Plume
//! cannot point `mlx_lm.server` at a `sha256:...` blob and there is
//! no honest way to surface a model name from the on-disk layout
//! without parsing Ollama's manifest. Ollama remains a provider via
//! `/api/tags` (D2+) — chat works, but the model files themselves
//! stay opaque to the local-model importer. See
//! `docs/MODEL_PROVIDERS.md § Ollama` for the runtime path.
//!
//! All existing scan defenses apply unchanged per source: dotfile
//! skip, depth cap (`MAX_SCAN_DEPTH = 8`), and symlink-as-noise. The
//! per-source root is the safety boundary the scan walks from; a
//! symlink whose target lives outside its own source root is rejected
//! exactly like in `PLUME_MODEL_DIR`.

use serde::Serialize;
use std::env;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LocalModel {
    /// Source-prefixed id of the form `<source-tag>:<relative-path>`.
    /// `<source-tag>` is the kebab-case `LocalModelSource` discriminant
    /// (`plume-model-dir`, `locally-ai-cache`, `lm-studio-cache`).
    /// The prefix is what makes the id unique across multi-source
    /// scans — two roots may both have a `qwen-mlx/` folder without
    /// colliding on the wire. Backend resolvers split on the FIRST
    /// `:` to recover (source, relative) and look the entry up.
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

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "kebab-case")]
pub enum LocalModelSource {
    /// `$PLUME_MODEL_DIR` (default `<cwd>/plume-models`). Primary
    /// source; the only one Plume itself writes to.
    PlumeModelDir,
    /// Locally AI's sandboxed HuggingFace cache. Read-only; Plume
    /// scans the snapshot folders the same way it scans `plume-models`
    /// subdirs, using the existing transformer-folder / MLX classifier.
    LocallyAiCache,
    /// LM Studio's flat models tree at `~/.lmstudio/models`. Read-only.
    LmStudioCache,
}

impl LocalModelSource {
    /// Kebab-case discriminant matching the `serde(rename_all)` rule
    /// — kept in code rather than re-deriving via `serde_json` so the
    /// id formatter doesn't allocate a JSON intermediate every scan.
    /// MUST stay in sync with the Serialize impl above; the unit test
    /// `local_model_source_tag_matches_serde_rename` pins that.
    pub fn tag(self) -> &'static str {
        match self {
            LocalModelSource::PlumeModelDir => "plume-model-dir",
            LocalModelSource::LocallyAiCache => "locally-ai-cache",
            LocalModelSource::LmStudioCache => "lm-studio-cache",
        }
    }

    /// Parse a kebab-case source tag back to the enum. Returns `None`
    /// for any unknown tag — the resolver maps that to `NotFound`
    /// rather than panicking.
    pub fn from_tag(tag: &str) -> Option<Self> {
        match tag {
            "plume-model-dir" => Some(LocalModelSource::PlumeModelDir),
            "locally-ai-cache" => Some(LocalModelSource::LocallyAiCache),
            "lm-studio-cache" => Some(LocalModelSource::LmStudioCache),
            _ => None,
        }
    }
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

/// D50: resolve a source kind to the directory we'd scan for it on
/// the current host. Returns `None` for an external source whose
/// dependency app isn't installed (the directory doesn't exist) — the
/// caller skips it silently rather than surfacing a "couldn't read"
/// failure for an app the user hasn't installed.
///
/// `PlumeModelDir` always returns `Some(...)` because the scanner
/// gracefully handles a missing directory as an empty inventory; we
/// preserve that "missing means empty" posture so the panel stays
/// readable on a fresh checkout. The two external sources are
/// existence-gated: `LocallyAiCache` returns `None` when neither the
/// `HOME`-derived nor the env-overridden path exists on disk, same
/// for `LmStudioCache`. Tests override the env vars to point at a
/// tempdir.
///
/// **Env overrides (test-only).** `PLUME_LOCALLY_AI_MODEL_DIR` and
/// `PLUME_LM_STUDIO_MODEL_DIR` let the test suite redirect each
/// external source at a controlled tempdir without monkey-patching
/// `HOME`. Production paths win when the env is unset.
pub fn source_root_for(source: LocalModelSource) -> Option<PathBuf> {
    match source {
        LocalModelSource::PlumeModelDir => Some(default_model_dir()),
        LocalModelSource::LocallyAiCache => {
            let path = locally_ai_cache_path()?;
            existing_dir(path)
        }
        LocalModelSource::LmStudioCache => {
            let path = lm_studio_cache_path()?;
            existing_dir(path)
        }
    }
}

fn locally_ai_cache_path() -> Option<PathBuf> {
    if let Some(env_dir) = env::var_os("PLUME_LOCALLY_AI_MODEL_DIR") {
        return Some(PathBuf::from(env_dir));
    }
    let home = env::var_os("HOME")?;
    Some(
        PathBuf::from(home)
            .join("Library")
            .join("Containers")
            .join("app.locallyai.Locally")
            .join("Data")
            .join("Library")
            .join("app.locallyai.Locally")
            .join("huggingface")
            .join("models"),
    )
}

fn lm_studio_cache_path() -> Option<PathBuf> {
    if let Some(env_dir) = env::var_os("PLUME_LM_STUDIO_MODEL_DIR") {
        return Some(PathBuf::from(env_dir));
    }
    let home = env::var_os("HOME")?;
    Some(PathBuf::from(home).join(".lmstudio").join("models"))
}

fn existing_dir(path: PathBuf) -> Option<PathBuf> {
    let meta = fs::symlink_metadata(&path).ok()?;
    if meta.file_type().is_dir() {
        Some(path)
    } else {
        None
    }
}

/// D50: order in which sources are visited and the result is reported.
/// Plume's own dir is first so the user's curated downloads appear at
/// the top of the panel; external caches follow in alphabetical-by-tag
/// order (deterministic for tests, no implicit preference between
/// third-party apps).
pub const SOURCE_SCAN_ORDER: &[LocalModelSource] = &[
    LocalModelSource::PlumeModelDir,
    LocalModelSource::LocallyAiCache,
    LocalModelSource::LmStudioCache,
];

/// D50: walk every configured source and return a single merged
/// inventory. Each source contributes its own entries with the
/// matching `source` field and a source-prefixed id; ordering is
/// `SOURCE_SCAN_ORDER` then per-source sort-by-path. External sources
/// whose root directory doesn't exist (the app isn't installed)
/// silently contribute zero entries — same posture as a missing
/// `plume-models/`.
pub fn scan_all_sources() -> Vec<LocalModel> {
    let mut all = Vec::new();
    for source in SOURCE_SCAN_ORDER {
        let Some(root) = source_root_for(*source) else {
            continue;
        };
        let mut entries = scan_source(&root, *source);
        all.append(&mut entries);
    }
    all
}

/// D50: scan a single source. Public so tests can drive it without
/// going through the env / source-root resolution. The id of every
/// returned entry carries the matching source tag prefix.
pub fn scan_source(root: &Path, source: LocalModelSource) -> Vec<LocalModel> {
    let mut models = Vec::new();
    // Immediate children of `root` live at depth 1; the root itself
    // is depth 0 (never surfaced).
    scan_dir_into(root, root, &mut models, 1, source);
    models.sort_by(|a, b| a.path.cmp(&b.path));
    models
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

/// D50: test-only single-source convenience. Pre-D50 the public scan
/// entry point took just a root directory and implicitly assigned
/// `LocalModelSource::PlumeModelDir` to every entry; the existing
/// test suite from D27 onward calls it that way. Production code now
/// goes through `scan_all_sources` (the multi-source merge) or
/// `scan_source` (single-source) directly so the test-shape wrapper
/// is `#[cfg(test)]` — the production binary doesn't carry it.
#[cfg(test)]
pub fn scan_model_dir(model_dir: &Path) -> Vec<LocalModel> {
    scan_source(model_dir, LocalModelSource::PlumeModelDir)
}

/// `depth` is the root-relative depth of the entries being iterated
/// by this call (NOT the depth of `dir`). The early-return is the
/// single, symmetric cap: it gates files, plain folders, and
/// transformer-folder detection together, eliminating the "folders
/// past the cap can still be detected" hole the previous shape had.
fn scan_dir_into(
    root: &Path,
    dir: &Path,
    models: &mut Vec<LocalModel>,
    depth: usize,
    source: LocalModelSource,
) {
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
            if let Some(model) = transformer_folder(root, &path, source) {
                models.push(model);
            } else {
                // Recursion is unconditional; the early-return at
                // the top of the recursed call handles the cap so
                // every kind of entry is gated by the same rule.
                scan_dir_into(root, &path, models, depth + 1, source);
            }
            continue;
        }
        if !metadata.is_file() {
            continue;
        }
        let Some(kind) = file_kind(&path) else {
            continue;
        };
        models.push(local_model(root, &path, kind, metadata.len(), source));
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

fn transformer_folder(root: &Path, folder: &Path, source: LocalModelSource) -> Option<LocalModel> {
    // D36 Codex fix: `is_file()` follows symlinks. The scanner's
    // contract is "symlinks inside the model dir never participate
    // in classification" (see `is_noise_path` + the symlink check in
    // `scan_dir`). A `config.json` symlinked to a path OUTSIDE the
    // model dir would otherwise drive folder detection AND feed the
    // MLX-quantization parse below with bytes from outside. Use
    // `symlink_metadata` and require a regular file.
    let config_path = folder.join("config.json");
    match fs::symlink_metadata(&config_path) {
        Ok(meta) if meta.file_type().is_file() => {}
        _ => return None,
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

    Some(local_model(root, folder, kind, size_bytes, source))
}

/// Max bytes of `config.json` we'll try to parse for MLX evidence.
/// Real configs are well under 50 KiB; the 256 KiB cap is generous
/// while still bounding the worst case (a hostile or corrupt file
/// pointing at a huge inline schema).
pub(crate) const CONFIG_JSON_BYTE_CAP: u64 = 256 * 1024;

/// Returns `true` iff `<folder>/config.json` parses as JSON and
/// contains a top-level `quantization` object with both `group_size`
/// (a positive integer) and `bits` (a positive integer) keys. All
/// other shapes — missing file, oversize file, malformed JSON, key
/// present but the wrong type, key named `quantization_config`
/// instead — return `false` without surfacing an error.
///
/// Safety: read is bounded by `CONFIG_JSON_BYTE_CAP` and we never
/// panic on parser output. We `symlink_metadata` (NOT `metadata`)
/// the config path and reject anything that isn't a regular file,
/// so a `config.json` symlinked outside the model dir cannot
/// influence classification even if `transformer_folder`'s leaf
/// check were ever weakened. Belt and braces with the caller, which
/// applies the same rejection at the entry point.
fn config_json_has_mlx_quantization(folder: &Path) -> bool {
    let path = folder.join("config.json");
    let Ok(meta) = fs::symlink_metadata(&path) else {
        return false;
    };
    if !meta.file_type().is_file() {
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

fn local_model(
    root: &Path,
    path: &Path,
    kind: LocalModelKind,
    size_bytes: u64,
    source: LocalModelSource,
) -> LocalModel {
    let relative = path.strip_prefix(root).unwrap_or(path);
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("local-model")
        .to_string();
    // D50: source-prefixed id. The format is `<source-tag>:<relative>`;
    // resolvers split on the first `:` to recover (source, relative).
    // Relative paths in our scan roots do not contain `:` in practice,
    // but the split-on-first-colon rule keeps the parse honest even
    // for filenames that do.
    let id = format!("{}:{}", source.tag(), relative.to_string_lossy());
    LocalModel {
        id,
        name,
        path: path.to_string_lossy().to_string(),
        kind,
        size_bytes,
        source,
    }
}

/// D50: split a source-prefixed inventory id back into (source,
/// relative-path). Returns `None` for ids that don't carry a known
/// source tag — the resolver maps that to `NotFound` (a corrupt or
/// stale id, never a transient failure). Split on the FIRST `:` so
/// a relative path that legally contains `:` (rare but possible on
/// macOS) survives the round-trip.
pub fn parse_inventory_id(id: &str) -> Option<(LocalModelSource, &str)> {
    let (tag, rest) = id.split_once(':')?;
    let source = LocalModelSource::from_tag(tag)?;
    Some((source, rest))
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

    /// D36 Codex fix: a `config.json` symlinked to an MLX-shaped
    /// file OUTSIDE the model dir must NOT drive classification.
    /// The scanner's contract is "symlinks never participate";
    /// pre-D36 leaf checks (`is_file()`, `fs::metadata`) followed
    /// symlinks, which would have let an attacker plant a
    /// configless `transformer-folder` and upgrade it to
    /// `mlx-folder` via a symlinked config. After the fix, the
    /// folder must surface as nothing at all (the rejected
    /// `config.json` means it doesn't even register as a
    /// transformer-folder), and the planted outside file is
    /// untouched.
    #[cfg(unix)]
    #[test]
    fn config_json_symlink_is_refused() {
        use std::os::unix::fs::symlink;

        let td = TempDir::new("config-symlink");
        let folder = td.path().join("model");
        fs::create_dir_all(&folder).expect("create folder");
        fs::write(folder.join("tokenizer.json"), b"{}").expect("write tokenizer");
        fs::write(folder.join("model.safetensors"), b"wgts").expect("write weights");

        // Real MLX-shape config OUTSIDE the model dir.
        let outside = TempDir::new("config-symlink-outside");
        let outside_config = outside.path().join("evil-config.json");
        fs::write(
            &outside_config,
            br#"{"quantization":{"group_size":64,"bits":4}}"#,
        )
        .expect("write outside config");

        // Symlink inside the model folder pointing at the outside
        // config. Pre-fix this would have classified the folder as
        // `MlxFolder`.
        symlink(&outside_config, folder.join("config.json")).expect("create symlink");

        let models = scan_model_dir(td.path());
        // The folder is invisible — `config.json` is rejected as a
        // symlink, so the transformer-folder check fails and the
        // scan recurses into the folder instead. That recursion
        // surfaces only the non-symlinked weight / tokenizer files
        // (neither has a recognised extension at the top level).
        // The key property: no `MlxFolder` and no `TransformerFolder`
        // result.
        for m in &models {
            assert_ne!(
                m.kind,
                LocalModelKind::MlxFolder,
                "outside MLX config must not classify the folder as MLX: {models:?}"
            );
            assert_ne!(
                m.kind,
                LocalModelKind::TransformerFolder,
                "outside symlinked config must not classify the folder as transformer: {models:?}"
            );
        }
        // The outside config must still exist intact.
        let outside_bytes = fs::read(&outside_config).expect("outside config readable");
        assert!(
            String::from_utf8_lossy(&outside_bytes).contains("\"group_size\":64"),
            "outside config must not have been mutated"
        );
    }

    // ─── D50: external local model sources ──────────────────────────────────

    /// Cargo runs tests in parallel by default; tests that mutate
    /// `PLUME_LOCALLY_AI_MODEL_DIR` / `PLUME_LM_STUDIO_MODEL_DIR` MUST
    /// serialize on this mutex so their set / scan / remove window is
    /// not interleaved with another test reading the same vars. Same
    /// pattern as `memory_mutex` — local to the test module so production
    /// code is unaffected.
    fn d50_env_mutex() -> &'static std::sync::Mutex<()> {
        static MUTEX: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
        MUTEX.get_or_init(|| std::sync::Mutex::new(()))
    }

    /// `LocalModelSource::tag()` must agree with what serde emits for
    /// the same variant. If serde's rename rule ever changes, this test
    /// is the canary — the IPC wire (Serialize) and the id formatter
    /// (`tag()`) MUST stay byte-identical or sources stop round-tripping.
    #[test]
    fn local_model_source_tag_matches_serde_rename() {
        for source in [
            LocalModelSource::PlumeModelDir,
            LocalModelSource::LocallyAiCache,
            LocalModelSource::LmStudioCache,
        ] {
            let json = serde_json::to_string(&source).expect("serialize source");
            let expected = format!("\"{}\"", source.tag());
            assert_eq!(json, expected, "tag/serde drift for {source:?}");
            // Round-trip through from_tag — the resolver relies on it.
            assert_eq!(LocalModelSource::from_tag(source.tag()), Some(source));
        }
        // Unknown tags don't resolve — resolver maps to NotFound.
        assert_eq!(LocalModelSource::from_tag("does-not-exist"), None);
        assert_eq!(LocalModelSource::from_tag(""), None);
    }

    /// Entries from `scan_source` carry the requested source AND a
    /// source-prefixed id. The Plume-managed dir uses the existing
    /// `plume-model-dir:` prefix; this is the floor of multi-source
    /// safety because resolvers split on it.
    #[test]
    fn scan_source_emits_source_prefixed_ids() {
        let td = TempDir::new("d50-ids");
        fs::write(td.path().join("a.gguf"), b"g").expect("write a");
        fs::write(td.path().join("b.safetensors"), b"s").expect("write b");

        let plume = scan_source(td.path(), LocalModelSource::PlumeModelDir);
        for m in &plume {
            assert_eq!(m.source, LocalModelSource::PlumeModelDir);
            assert!(
                m.id.starts_with("plume-model-dir:"),
                "id missing source prefix: {}",
                m.id
            );
            // The id round-trips through parse_inventory_id.
            let (s, rel) = parse_inventory_id(&m.id).expect("parse id");
            assert_eq!(s, LocalModelSource::PlumeModelDir);
            assert!(!rel.is_empty(), "relative part should not be empty");
        }

        // Same scan with a different source tag relabels the entries.
        let locally = scan_source(td.path(), LocalModelSource::LocallyAiCache);
        for m in &locally {
            assert_eq!(m.source, LocalModelSource::LocallyAiCache);
            assert!(
                m.id.starts_with("locally-ai-cache:"),
                "id missing locally-ai-cache prefix: {}",
                m.id
            );
        }
    }

    /// `parse_inventory_id` survives a relative path that itself
    /// contains `:`. The split-on-FIRST-colon rule is the property
    /// that keeps the parse honest. On Unix, `:` is a perfectly legal
    /// filename character (HFS used `/` and Finder shows `:`; on the
    /// raw filesystem it just passes through).
    #[test]
    fn parse_inventory_id_splits_on_first_colon() {
        let id = "plume-model-dir:weird:name.gguf";
        let (s, rel) = parse_inventory_id(id).expect("parse");
        assert_eq!(s, LocalModelSource::PlumeModelDir);
        assert_eq!(rel, "weird:name.gguf");
    }

    /// An id without a known source tag is a stale id; `parse_` returns
    /// `None` and the resolver maps it to `NotFound` (frontend refresh
    /// recovers). Same for an id without a `:` at all.
    #[test]
    fn parse_inventory_id_rejects_unknown_or_missing_prefix() {
        assert!(parse_inventory_id("just-a-relative-path.gguf").is_none());
        assert!(parse_inventory_id("invented-source:foo.gguf").is_none());
        assert!(parse_inventory_id("").is_none());
        // A bare prefix with empty body still parses (the resolver
        // handles the empty path safely via the path-safety walk).
        let (s, rel) = parse_inventory_id("lm-studio-cache:").expect("parse");
        assert_eq!(s, LocalModelSource::LmStudioCache);
        assert_eq!(rel, "");
    }

    /// `source_root_for(PlumeModelDir)` always returns `Some(...)` —
    /// the scanner gracefully handles a missing primary dir as empty
    /// inventory, so the panel never goes blank on a fresh checkout.
    /// External sources return `None` for missing roots; the multi-
    /// source scan silently skips them.
    #[test]
    fn source_root_external_sources_return_none_for_missing_dir() {
        let _guard = d50_env_mutex().lock().unwrap_or_else(|e| e.into_inner());
        let td = TempDir::new("d50-missing-roots");
        let missing = td.path().join("does-not-exist");
        std::env::set_var("PLUME_LOCALLY_AI_MODEL_DIR", &missing);
        std::env::set_var("PLUME_LM_STUDIO_MODEL_DIR", &missing);

        let locally = source_root_for(LocalModelSource::LocallyAiCache);
        let lmstudio = source_root_for(LocalModelSource::LmStudioCache);

        std::env::remove_var("PLUME_LOCALLY_AI_MODEL_DIR");
        std::env::remove_var("PLUME_LM_STUDIO_MODEL_DIR");

        assert!(
            locally.is_none(),
            "expected None for missing Locally AI root"
        );
        assert!(
            lmstudio.is_none(),
            "expected None for missing LM Studio root"
        );
    }

    /// `scan_all_sources` merges entries across sources in
    /// `SOURCE_SCAN_ORDER`. Each entry's `source` field matches the
    /// scan root it came from, and each id carries the matching prefix.
    #[test]
    fn scan_all_sources_merges_multi_source_inventory() {
        let _guard = d50_env_mutex().lock().unwrap_or_else(|e| e.into_inner());
        // Two tempdirs, one per external source. Plume dir we leave
        // env-default (it may or may not exist; the test doesn't
        // assert on the primary count).
        let locally_td = TempDir::new("d50-locally");
        let lmstudio_td = TempDir::new("d50-lmstudio");

        // One model per external source.
        let locally_model = locally_td.path().join("gemma-2b");
        fs::create_dir_all(&locally_model).expect("create locally model");
        fs::write(locally_model.join("config.json"), b"{}").expect("locally config");
        fs::write(locally_model.join("tokenizer.json"), b"{}").expect("locally tokenizer");
        fs::write(locally_model.join("model.safetensors"), b"w").expect("locally weights");

        fs::write(lmstudio_td.path().join("orphan.gguf"), b"w").expect("lmstudio model");

        std::env::set_var("PLUME_LOCALLY_AI_MODEL_DIR", locally_td.path());
        std::env::set_var("PLUME_LM_STUDIO_MODEL_DIR", lmstudio_td.path());

        let all = scan_all_sources();

        std::env::remove_var("PLUME_LOCALLY_AI_MODEL_DIR");
        std::env::remove_var("PLUME_LM_STUDIO_MODEL_DIR");

        // The two we created must be present, regardless of what the
        // ambient Plume dir contains.
        let locally_hit = all
            .iter()
            .find(|m| m.source == LocalModelSource::LocallyAiCache);
        let lmstudio_hit = all
            .iter()
            .find(|m| m.source == LocalModelSource::LmStudioCache);
        assert!(locally_hit.is_some(), "Locally AI entry missing: {all:?}");
        assert!(lmstudio_hit.is_some(), "LM Studio entry missing: {all:?}");

        let locally_hit = locally_hit.unwrap();
        assert!(locally_hit.id.starts_with("locally-ai-cache:"));
        assert_eq!(locally_hit.kind, LocalModelKind::TransformerFolder);
        assert_eq!(locally_hit.name, "gemma-2b");

        let lmstudio_hit = lmstudio_hit.unwrap();
        assert!(lmstudio_hit.id.starts_with("lm-studio-cache:"));
        assert_eq!(lmstudio_hit.kind, LocalModelKind::Gguf);
        assert_eq!(lmstudio_hit.name, "orphan.gguf");
    }

    /// The symlink-skip defense applies per-source: a symlink inside
    /// an external source dir whose target lives elsewhere must NOT
    /// surface as an inventory entry. Exercises `scan_source` directly
    /// rather than going through `scan_all_sources` so we don't have
    /// to serialize on the env mutex — the source enum's tagging is
    /// what makes the test honest about which source the entry would
    /// have been attributed to.
    #[cfg(unix)]
    #[test]
    fn external_source_symlinks_are_skipped() {
        use std::os::unix::fs::symlink;

        let cache_td = TempDir::new("d50-cache-symlinks");
        let outside_td = TempDir::new("d50-outside");
        let outside_target = outside_td.path().join("foreign.gguf");
        fs::write(&outside_target, b"w").expect("write outside target");
        symlink(&outside_target, cache_td.path().join("link.gguf"))
            .expect("create symlink in cache");

        let inventory = scan_source(cache_td.path(), LocalModelSource::LocallyAiCache);

        assert!(
            inventory.is_empty(),
            "symlink in external source dir must not surface: {inventory:?}"
        );
    }

    /// Dotfile noise (`.git`, `.DS_Store`, `.cache`) is skipped on
    /// external sources the same way as Plume-managed dir. macOS
    /// sprinkles `.DS_Store` everywhere; we don't want it in the
    /// panel.
    #[test]
    fn external_source_dotfile_skip() {
        let td = TempDir::new("d50-dotfiles");
        fs::write(td.path().join(".DS_Store"), b"junk").expect("write .DS_Store");
        let dot_cache = td.path().join(".cache");
        fs::create_dir_all(&dot_cache).expect("create .cache");
        fs::write(dot_cache.join("ignored.gguf"), b"w").expect("write inside .cache");
        // Real model alongside to confirm the scan still finds positive entries.
        fs::write(td.path().join("real.gguf"), b"w").expect("write real");

        let inventory = scan_source(td.path(), LocalModelSource::LmStudioCache);
        assert_eq!(inventory.len(), 1, "got {inventory:?}");
        assert_eq!(inventory[0].name, "real.gguf");
        assert_eq!(inventory[0].source, LocalModelSource::LmStudioCache);
    }
}
