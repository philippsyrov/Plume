//! D41: read on-disk details for a single local-model entry.
//!
//! `providers/local_models.rs` owns the scan + classification path —
//! "is this folder a transformer / mlx-folder / single GGUF". D41
//! sits on top: given an inventory id, parse `config.json` (bounded)
//! and surface honest model details for the panel's expand-row view.
//!
//! Sits in its own module so `local_models.rs` stays under the
//! decomposition cap. Both files share `CONFIG_JSON_BYTE_CAP` so a
//! tightening on the scanner's side automatically applies here.
//!
//! Read-only: no writes, no spawns, no daemon HTTP. Symlink defense
//! mirrors the scanner's posture — `symlink_metadata` at every
//! segment, `read_dir` over one folder level only, refuse any path
//! whose components leave the model directory.

use serde::Serialize;
use std::fs;
use std::io::Read;
use std::path::Path;

use crate::providers::local_models::CONFIG_JSON_BYTE_CAP;

/// D41: honest details for a single local-model entry. Every field
/// is `Option<...>` because real-world checkpoints vary: a quantized
/// MLX folder reports all of it; a vanilla HF safetensors folder
/// drops `quantization_bits` / `quantization_group_size`; a single
/// `.gguf` file drops everything except weight counts.
///
/// The two `quantization_*` fields ONLY report the MLX-LM
/// `{"quantization": {"bits": _, "group_size": _}}` shape on
/// purpose. HuggingFace's `quantization_config` (bitsandbytes,
/// AWQ, etc.) is a different protocol and is deliberately NOT
/// surfaced here — see `docs/LOCAL_AGENT_NORTH_STAR.md § MLX-first`.
///
/// Counts apply to weight files only. Tokenizer, config, and other
/// metadata files are inspected but not counted here.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LocalModelDetails {
    /// Model class as declared by `config.json`. HF puts these in
    /// `architectures: [...]` and we take the first; some MLX-LM
    /// configs only have `model_type`. `None` when neither key is
    /// present (or there's no `config.json`).
    pub architecture: Option<String>,
    /// `config.json` `model_type` ("llama", "gemma2", "qwen2", …).
    /// Reported separately from `architecture` because a single
    /// model_type can back several architectures.
    pub model_type: Option<String>,
    /// Context-window length declared by `config.json`. Reads
    /// `max_position_embeddings` first, then `max_seq_len`
    /// (some MLX-LM configs use the latter). `None` when neither
    /// is present.
    pub max_context: Option<u64>,
    /// MLX-LM quantization bits. Only the `{"quantization":
    /// {"bits": _}}` shape; HF's `quantization_config` does NOT
    /// populate this.
    pub quantization_bits: Option<u64>,
    /// MLX-LM quantization group size, same source as
    /// `quantization_bits`.
    pub quantization_group_size: Option<u64>,
    /// `true` when any of `tokenizer.json`, `tokenizer.model`,
    /// or `tokenizer_config.json` exists as a regular file at the
    /// folder root. Symlinks DO NOT count — same defense as the
    /// folder classifier.
    pub tokenizer_present: bool,
    /// Number of weight-bearing files at the folder root. Counts
    /// `.safetensors`, `.gguf`, `.npz`, and `.bin` (the last because
    /// older HF uploads still ship `pytorch_model.bin`). Each file
    /// must be a regular file by `symlink_metadata`.
    pub weight_file_count: u32,
    /// Sum of weight-file byte sizes. Doubles as a sanity check on
    /// the inventory's `sizeBytes` for folder kinds, which counts
    /// every file (tokenizers, config, etc.).
    pub weight_bytes_total: u64,
}

/// Why a `read_local_model_details` call failed. The IPC handler
/// maps these to typed `IpcError` so the frontend can switch on a
/// stable shape rather than parsing message strings.
#[derive(Debug)]
pub enum LocalModelDetailsError {
    /// `model_id` (project-relative path) doesn't resolve to a real
    /// entry under the model directory, or the entry was filtered
    /// out by the scanner. The frontend should call
    /// `providers.localModels` again — the underlying inventory may
    /// have changed since the row was rendered.
    NotFound,
    /// `model_id` resolved to a path outside the model directory, or
    /// to a path that traverses a symlink at any segment. Treat as
    /// a misconfiguration (a corrupt inventory row, an externally
    /// edited model dir) rather than a transient failure.
    PathEscape,
    /// IO error reading the entry (permissions, unreadable dir, etc).
    Io(std::io::Error),
}

impl std::fmt::Display for LocalModelDetailsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LocalModelDetailsError::NotFound => f.write_str("local model not found in inventory"),
            LocalModelDetailsError::PathEscape => {
                f.write_str("local model id resolved outside the model directory")
            }
            LocalModelDetailsError::Io(err) => write!(f, "io error reading local model: {err}"),
        }
    }
}

impl std::error::Error for LocalModelDetailsError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            LocalModelDetailsError::Io(err) => Some(err),
            _ => None,
        }
    }
}

/// File extensions counted as model weights for D41 details. Anchored
/// to lowercase comparisons in the caller so the case of the suffix
/// on disk doesn't matter.
const WEIGHT_EXTENSIONS: &[&str] = &[".safetensors", ".gguf", ".npz", ".bin"];
/// Tokenizer filenames recognised at folder root. Same case-insensitive
/// match. Listed in the order most-likely-to-be-present so an early
/// `true` short-circuits the scan.
const TOKENIZER_FILES: &[&str] = &["tokenizer.json", "tokenizer.model", "tokenizer_config.json"];

/// D41: resolve `model_id` against `model_dir` and read on-disk
/// details for the matched entry. Same path-safety posture as the
/// scanner: every segment is checked with `symlink_metadata` and the
/// resolved path must live under `model_dir`. The config-read budget
/// matches `config_json_has_mlx_quantization` so the same `256 KiB`
/// cap defends both code paths.
///
/// Read-only: no writes, no spawns. Returns `Err(NotFound)` when the
/// inventory match misses (the canonical path doesn't resolve to a
/// known entry shape) rather than fabricating an empty details
/// record — the frontend should refresh `providers.localModels` and
/// retry.
pub fn read_local_model_details(
    model_dir: &Path,
    model_id: &str,
) -> Result<LocalModelDetails, LocalModelDetailsError> {
    // Reject obvious traversal up-front. A real inventory id is a
    // relative path of safe segments; anything weirder is a bug or
    // an attack, not a refresh case.
    if model_id.is_empty()
        || model_id.starts_with('/')
        || model_id.contains("..")
        || model_id.contains('\0')
    {
        return Err(LocalModelDetailsError::PathEscape);
    }

    let candidate = model_dir.join(model_id);

    // Walk from `model_dir` to `candidate` and refuse a symlink at
    // any segment. We can't use `canonicalize` because that would
    // resolve a planted symlink rather than reject it.
    let rel = candidate
        .strip_prefix(model_dir)
        .map_err(|_| LocalModelDetailsError::PathEscape)?;
    let mut walk = model_dir.to_path_buf();
    for segment in rel.components() {
        use std::path::Component;
        let name = match segment {
            Component::Normal(n) => n,
            // Parent / current-dir / root components were ruled out by
            // the `starts_with('/')` / `..` checks above; anything
            // surviving here is a guard against future API changes.
            _ => return Err(LocalModelDetailsError::PathEscape),
        };
        walk.push(name);
        match fs::symlink_metadata(&walk) {
            Ok(meta) if meta.file_type().is_symlink() => {
                return Err(LocalModelDetailsError::PathEscape);
            }
            Ok(_) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                return Err(LocalModelDetailsError::NotFound);
            }
            Err(err) => return Err(LocalModelDetailsError::Io(err)),
        }
    }

    let meta = fs::symlink_metadata(&candidate).map_err(|err| {
        if err.kind() == std::io::ErrorKind::NotFound {
            LocalModelDetailsError::NotFound
        } else {
            LocalModelDetailsError::Io(err)
        }
    })?;

    if meta.file_type().is_file() {
        // Single-file kinds (.gguf, .safetensors): the only honest
        // detail is the file's byte size. Everything else stays None.
        return Ok(LocalModelDetails {
            architecture: None,
            model_type: None,
            max_context: None,
            quantization_bits: None,
            quantization_group_size: None,
            tokenizer_present: false,
            weight_file_count: 1,
            weight_bytes_total: meta.len(),
        });
    }

    if !meta.file_type().is_dir() {
        return Err(LocalModelDetailsError::NotFound);
    }

    // Folder kind: walk one level (no recursion — only weight files
    // at the folder root count, matching the classifier's contract),
    // pick up tokenizer + weight files, then parse config.json.
    let mut tokenizer_present = false;
    let mut weight_file_count: u32 = 0;
    let mut weight_bytes_total: u64 = 0;

    for entry in fs::read_dir(&candidate).map_err(LocalModelDetailsError::Io)? {
        let entry = entry.map_err(LocalModelDetailsError::Io)?;
        let entry_path = entry.path();
        let Ok(entry_meta) = fs::symlink_metadata(&entry_path) else {
            continue;
        };
        if !entry_meta.file_type().is_file() {
            continue;
        }
        let Some(name) = entry_path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let lower = name.to_ascii_lowercase();
        if TOKENIZER_FILES.iter().any(|t| *t == lower) {
            tokenizer_present = true;
        }
        if WEIGHT_EXTENSIONS.iter().any(|ext| lower.ends_with(ext)) {
            weight_file_count = weight_file_count.saturating_add(1);
            weight_bytes_total = weight_bytes_total.saturating_add(entry_meta.len());
        }
    }

    let cfg = read_config_json(&candidate);

    Ok(LocalModelDetails {
        architecture: cfg.architecture,
        model_type: cfg.model_type,
        max_context: cfg.max_context,
        quantization_bits: cfg.quantization_bits,
        quantization_group_size: cfg.quantization_group_size,
        tokenizer_present,
        weight_file_count,
        weight_bytes_total,
    })
}

/// Bounded `config.json` parse: returns whatever D41-relevant fields
/// the file carries, with all-`None` on every failure mode (missing,
/// oversize, malformed, symlinked). Same byte cap and symlink-refusal
/// posture as `config_json_has_mlx_quantization` — a planted
/// `config.json` -> elsewhere link cannot influence details either.
fn read_config_json(folder: &Path) -> ConfigJsonFields {
    let empty = ConfigJsonFields::default();
    let path = folder.join("config.json");
    let Ok(meta) = fs::symlink_metadata(&path) else {
        return empty;
    };
    if !meta.file_type().is_file() {
        return empty;
    }
    if meta.len() > CONFIG_JSON_BYTE_CAP {
        return empty;
    }
    let Ok(mut file) = fs::File::open(&path) else {
        return empty;
    };
    let mut buf = String::new();
    if file
        .by_ref()
        .take(CONFIG_JSON_BYTE_CAP)
        .read_to_string(&mut buf)
        .is_err()
    {
        return empty;
    }
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&buf) else {
        return empty;
    };

    // `architectures: [..]` is the HF convention; take the first.
    let architecture = value
        .get("architectures")
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first())
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let model_type = value
        .get("model_type")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    // Two context-length keys are common; prefer the standard HF one
    // when both exist. `max_seq_len` shows up in some MLX-LM configs.
    let max_context = value
        .get("max_position_embeddings")
        .and_then(|v| v.as_u64())
        .or_else(|| value.get("max_seq_len").and_then(|v| v.as_u64()));

    let quant_obj = value.get("quantization").and_then(|v| v.as_object());
    let quantization_bits = quant_obj
        .and_then(|q| q.get("bits"))
        .and_then(|v| v.as_u64());
    let quantization_group_size = quant_obj
        .and_then(|q| q.get("group_size"))
        .and_then(|v| v.as_u64());

    ConfigJsonFields {
        architecture,
        model_type,
        max_context,
        quantization_bits,
        quantization_group_size,
    }
}

#[derive(Default, Debug, PartialEq, Eq)]
struct ConfigJsonFields {
    architecture: Option<String>,
    model_type: Option<String>,
    max_context: Option<u64>,
    quantization_bits: Option<u64>,
    quantization_group_size: Option<u64>,
}

#[cfg(test)]
#[path = "local_model_details_tests.rs"]
mod tests;
