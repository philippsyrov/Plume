//! Tests for `local_model_details`. Sibling file via `#[path]` so
//! the production module stays under the decomposition cap.

use super::*;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(label: &str) -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "plume-local-model-details-{}-{}-{}",
            label,
            std::process::id(),
            nanos
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

fn make_folder_model(
    root: &Path,
    name: &str,
    config_json: &str,
    weight_files: &[(&str, usize)],
    tokenizer_files: &[&str],
) -> PathBuf {
    let folder = root.join(name);
    fs::create_dir_all(&folder).unwrap();
    if !config_json.is_empty() {
        fs::write(folder.join("config.json"), config_json).unwrap();
    }
    for (weight_name, size) in weight_files {
        fs::write(folder.join(weight_name), vec![0u8; *size]).unwrap();
    }
    for tokenizer_name in tokenizer_files {
        fs::write(folder.join(tokenizer_name), b"{}").unwrap();
    }
    folder
}

// --- path-safety paths ---------------------------------------------------

#[test]
fn rejects_empty_id() {
    let td = TempDir::new("empty");
    let err = read_local_model_details(td.path(), "").expect_err("empty must reject");
    assert!(matches!(err, LocalModelDetailsError::PathEscape));
}

#[test]
fn rejects_absolute_id() {
    let td = TempDir::new("abs");
    let err = read_local_model_details(td.path(), "/etc/passwd").expect_err("absolute must reject");
    assert!(matches!(err, LocalModelDetailsError::PathEscape));
}

#[test]
fn rejects_dot_dot_traversal() {
    let td = TempDir::new("dotdot");
    let err = read_local_model_details(td.path(), "../outside").expect_err("dot-dot must reject");
    assert!(matches!(err, LocalModelDetailsError::PathEscape));
}

#[test]
fn rejects_embedded_nul_byte() {
    let td = TempDir::new("nul");
    let err = read_local_model_details(td.path(), "model\0name").expect_err("NUL byte must reject");
    assert!(matches!(err, LocalModelDetailsError::PathEscape));
}

#[test]
fn rejects_symlinked_segment_in_id() {
    #[cfg(unix)]
    {
        let td = TempDir::new("symlink-seg");
        let outside = td.path().join("outside");
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("config.json"), "{}").unwrap();
        // Plant a symlink INSIDE the model dir that points outside.
        let link = td.path().join("models").join("planted");
        fs::create_dir_all(link.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(&outside, &link).unwrap();

        let err = read_local_model_details(td.path(), "models/planted")
            .expect_err("symlinked segment must reject");
        assert!(
            matches!(err, LocalModelDetailsError::PathEscape),
            "expected PathEscape, got {err:?}"
        );
    }
}

#[test]
fn missing_id_reports_not_found() {
    let td = TempDir::new("missing");
    let err =
        read_local_model_details(td.path(), "no-such-folder").expect_err("missing must reject");
    assert!(
        matches!(err, LocalModelDetailsError::NotFound),
        "expected NotFound, got {err:?}"
    );
}

// --- single-file kinds ---------------------------------------------------

#[test]
fn single_gguf_file_reports_only_size() {
    let td = TempDir::new("single-gguf");
    let gguf_path = td.path().join("tiny.gguf");
    fs::write(&gguf_path, vec![0u8; 12345]).unwrap();

    let details = read_local_model_details(td.path(), "tiny.gguf").expect("ok");
    assert_eq!(details.architecture, None);
    assert_eq!(details.model_type, None);
    assert_eq!(details.max_context, None);
    assert_eq!(details.quantization_bits, None);
    assert_eq!(details.quantization_group_size, None);
    assert!(!details.tokenizer_present);
    assert_eq!(details.weight_file_count, 1);
    assert_eq!(details.weight_bytes_total, 12345);
}

// --- transformer-folder details -----------------------------------------

#[test]
fn folder_with_hf_config_surfaces_architecture_and_max_context() {
    let td = TempDir::new("hf-folder");
    make_folder_model(
        td.path(),
        "llama-1b",
        r#"{
            "architectures": ["LlamaForCausalLM"],
            "model_type": "llama",
            "max_position_embeddings": 4096,
            "hidden_size": 2048
        }"#,
        &[("model.safetensors", 2048)],
        &["tokenizer.json"],
    );

    let details = read_local_model_details(td.path(), "llama-1b").expect("ok");
    assert_eq!(details.architecture.as_deref(), Some("LlamaForCausalLM"));
    assert_eq!(details.model_type.as_deref(), Some("llama"));
    assert_eq!(details.max_context, Some(4096));
    assert_eq!(details.quantization_bits, None);
    assert_eq!(details.quantization_group_size, None);
    assert!(details.tokenizer_present);
    assert_eq!(details.weight_file_count, 1);
    assert_eq!(details.weight_bytes_total, 2048);
}

#[test]
fn folder_falls_back_to_max_seq_len_when_max_position_missing() {
    let td = TempDir::new("max-seq-len");
    make_folder_model(
        td.path(),
        "qwen-1b",
        r#"{
            "model_type": "qwen2",
            "max_seq_len": 8192
        }"#,
        &[("model.safetensors", 100)],
        &["tokenizer.json"],
    );

    let details = read_local_model_details(td.path(), "qwen-1b").expect("ok");
    assert_eq!(details.max_context, Some(8192));
}

#[test]
fn folder_prefers_max_position_embeddings_over_max_seq_len() {
    let td = TempDir::new("both-context-keys");
    make_folder_model(
        td.path(),
        "both",
        r#"{
            "model_type": "llama",
            "max_position_embeddings": 4096,
            "max_seq_len": 99999
        }"#,
        &[("model.safetensors", 100)],
        &["tokenizer.json"],
    );

    let details = read_local_model_details(td.path(), "both").expect("ok");
    assert_eq!(details.max_context, Some(4096));
}

// --- quantization paths --------------------------------------------------

#[test]
fn surfaces_mlx_quantization_when_present() {
    let td = TempDir::new("mlx-quant");
    make_folder_model(
        td.path(),
        "qwen-4bit",
        r#"{
            "architectures": ["Qwen2ForCausalLM"],
            "model_type": "qwen2",
            "quantization": {"group_size": 64, "bits": 4}
        }"#,
        &[("weights.npz", 1024), ("model.safetensors", 512)],
        &["tokenizer.json"],
    );

    let details = read_local_model_details(td.path(), "qwen-4bit").expect("ok");
    assert_eq!(details.quantization_bits, Some(4));
    assert_eq!(details.quantization_group_size, Some(64));
    assert_eq!(details.weight_file_count, 2);
    assert_eq!(details.weight_bytes_total, 1024 + 512);
}

#[test]
fn ignores_hf_quantization_config_key() {
    // HF's `quantization_config` is a different protocol than MLX's
    // top-level `quantization`. The details reader must NOT surface
    // `quantization_config.bits` as `quantization_bits` — that would
    // be a false MLX claim.
    let td = TempDir::new("hf-quant-config");
    make_folder_model(
        td.path(),
        "bnb-model",
        r#"{
            "model_type": "llama",
            "quantization_config": {
                "load_in_4bit": true,
                "bnb_4bit_compute_dtype": "float16",
                "bits": 4
            }
        }"#,
        &[("model.safetensors", 100)],
        &["tokenizer.json"],
    );

    let details = read_local_model_details(td.path(), "bnb-model").expect("ok");
    assert_eq!(details.quantization_bits, None);
    assert_eq!(details.quantization_group_size, None);
}

// --- weight + tokenizer counts -----------------------------------------

#[test]
fn counts_multiple_weight_files_and_summed_bytes() {
    let td = TempDir::new("shards");
    make_folder_model(
        td.path(),
        "sharded",
        r#"{"model_type":"llama"}"#,
        &[
            ("model-00001-of-00003.safetensors", 1000),
            ("model-00002-of-00003.safetensors", 2000),
            ("model-00003-of-00003.safetensors", 3000),
        ],
        &["tokenizer.json"],
    );

    let details = read_local_model_details(td.path(), "sharded").expect("ok");
    assert_eq!(details.weight_file_count, 3);
    assert_eq!(details.weight_bytes_total, 6000);
}

#[test]
fn pytorch_bin_files_count_as_weights() {
    // Older HF uploads ship `pytorch_model.bin` instead of safetensors.
    // We accept `.bin` to keep the count honest.
    let td = TempDir::new("pytorch-bin");
    make_folder_model(
        td.path(),
        "legacy",
        r#"{"model_type":"llama"}"#,
        &[("pytorch_model.bin", 5000)],
        &["tokenizer.json"],
    );

    let details = read_local_model_details(td.path(), "legacy").expect("ok");
    assert_eq!(details.weight_file_count, 1);
    assert_eq!(details.weight_bytes_total, 5000);
}

#[test]
fn tokenizer_model_filename_also_counts() {
    // SentencePiece tokenizers ship as `tokenizer.model`.
    let td = TempDir::new("sp-tokenizer");
    make_folder_model(
        td.path(),
        "sp",
        r#"{"model_type":"llama"}"#,
        &[("model.safetensors", 10)],
        &["tokenizer.model"],
    );

    let details = read_local_model_details(td.path(), "sp").expect("ok");
    assert!(details.tokenizer_present);
}

#[test]
fn tokenizer_config_only_still_counts_as_tokenizer_present() {
    let td = TempDir::new("config-only");
    make_folder_model(
        td.path(),
        "cfg-only",
        r#"{"model_type":"llama"}"#,
        &[("model.safetensors", 10)],
        &["tokenizer_config.json"],
    );

    let details = read_local_model_details(td.path(), "cfg-only").expect("ok");
    assert!(details.tokenizer_present);
}

#[test]
fn folder_with_no_tokenizer_reports_false() {
    let td = TempDir::new("no-tokenizer");
    make_folder_model(
        td.path(),
        "untokenized",
        r#"{"model_type":"llama"}"#,
        &[("model.safetensors", 10)],
        &[],
    );

    let details = read_local_model_details(td.path(), "untokenized").expect("ok");
    assert!(!details.tokenizer_present);
}

// --- config.json edge cases ---------------------------------------------

#[test]
fn missing_config_json_returns_all_none_for_config_fields() {
    let td = TempDir::new("no-config");
    let folder = td.path().join("orphan");
    fs::create_dir_all(&folder).unwrap();
    fs::write(folder.join("model.safetensors"), vec![0u8; 100]).unwrap();
    fs::write(folder.join("tokenizer.json"), b"{}").unwrap();

    let details = read_local_model_details(td.path(), "orphan").expect("ok");
    assert_eq!(details.architecture, None);
    assert_eq!(details.model_type, None);
    assert_eq!(details.max_context, None);
    // Weights and tokenizer still counted; config-derived fields all None.
    assert!(details.tokenizer_present);
    assert_eq!(details.weight_file_count, 1);
    assert_eq!(details.weight_bytes_total, 100);
}

#[test]
fn malformed_config_json_returns_all_none_silently() {
    let td = TempDir::new("bad-config");
    make_folder_model(
        td.path(),
        "broken",
        r#"{ not valid json"#,
        &[("model.safetensors", 100)],
        &["tokenizer.json"],
    );

    let details = read_local_model_details(td.path(), "broken").expect("ok");
    assert_eq!(details.architecture, None);
    assert_eq!(details.model_type, None);
    assert_eq!(details.max_context, None);
    // Reading the folder didn't fail — the config just gave us nothing.
    assert!(details.tokenizer_present);
    assert_eq!(details.weight_file_count, 1);
}

#[test]
fn oversize_config_json_is_skipped() {
    let td = TempDir::new("huge-config");
    // CONFIG_JSON_BYTE_CAP is 256 KiB; write 300 KiB of valid JSON.
    let folder = td.path().join("huge");
    fs::create_dir_all(&folder).unwrap();
    let padding = "x".repeat(300 * 1024);
    let config = format!(r#"{{"model_type":"llama","padding":"{}"}}"#, padding);
    fs::write(folder.join("config.json"), config).unwrap();
    fs::write(folder.join("model.safetensors"), vec![0u8; 100]).unwrap();
    fs::write(folder.join("tokenizer.json"), b"{}").unwrap();

    let details = read_local_model_details(td.path(), "huge").expect("ok");
    // Config skipped → no model_type surfaces, but tokenizer + weight
    // counts still work.
    assert_eq!(details.model_type, None);
    assert!(details.tokenizer_present);
    assert_eq!(details.weight_file_count, 1);
}

#[test]
fn symlinked_config_json_is_ignored() {
    #[cfg(unix)]
    {
        let td = TempDir::new("symlink-config");
        let outside = td.path().join("outside-config.json");
        fs::write(
            &outside,
            r#"{"model_type":"llama","max_position_embeddings":99999}"#,
        )
        .unwrap();

        let folder = td.path().join("trap");
        fs::create_dir_all(&folder).unwrap();
        std::os::unix::fs::symlink(&outside, folder.join("config.json")).unwrap();
        fs::write(folder.join("model.safetensors"), vec![0u8; 100]).unwrap();
        fs::write(folder.join("tokenizer.json"), b"{}").unwrap();

        let details = read_local_model_details(td.path(), "trap").expect("ok");
        // The symlinked config must NOT have driven any classification.
        assert_eq!(details.model_type, None);
        assert_eq!(details.max_context, None);
        assert_eq!(details.architecture, None);
        // Outside config must still exist intact (defense doesn't mutate).
        let raw = fs::read_to_string(&outside).unwrap();
        assert!(raw.contains("99999"));
    }
}

// --- serialization shape ------------------------------------------------

#[test]
fn serializes_to_camelcase_wire_shape() {
    let value = LocalModelDetails {
        architecture: Some("LlamaForCausalLM".into()),
        model_type: Some("llama".into()),
        max_context: Some(4096),
        quantization_bits: Some(4),
        quantization_group_size: Some(64),
        tokenizer_present: true,
        weight_file_count: 3,
        weight_bytes_total: 1234,
    };
    let json = serde_json::to_string(&value).unwrap();
    for key in [
        "\"architecture\"",
        "\"modelType\"",
        "\"maxContext\"",
        "\"quantizationBits\"",
        "\"quantizationGroupSize\"",
        "\"tokenizerPresent\"",
        "\"weightFileCount\"",
        "\"weightBytesTotal\"",
    ] {
        assert!(json.contains(key), "missing key {key} in {json}");
    }
    // Snake-case must NOT leak.
    for leaked in [
        "\"model_type\"",
        "\"max_context\"",
        "\"quantization_bits\"",
        "\"quantization_group_size\"",
        "\"tokenizer_present\"",
        "\"weight_file_count\"",
        "\"weight_bytes_total\"",
    ] {
        assert!(
            !json.contains(leaked),
            "snake_case {leaked} leaked in {json}"
        );
    }
}
