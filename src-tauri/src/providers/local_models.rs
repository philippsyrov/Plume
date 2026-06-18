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
#[path = "local_models_tests.rs"]
mod tests;
