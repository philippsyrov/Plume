// D129C benchmark sidecar. Two modes, both thin shells over the REAL
// product modules — nothing in here reimplements Plume behavior:
//
//   plume_bench patch-check
//     One JSON request on stdin: {"projectRoot": "...", "diff": "...",
//     "apply": bool}. Runs the real `plume::patch::validate_patch`
//     and, when `apply` is true and validation passed, the real
//     `plume::patch::apply_patch` (callers pass a DISPOSABLE copy as
//     projectRoot — apply mutates it). One JSON verdict on stdout:
//     {"ok": true, "valid": bool, "applied": bool|null,
//      "validate": <Plume's own PatchValidateResponse JSON>}.
//
//   plume_bench orchestrate --port N --model PATH [--health]
//     A benchmark session speaking the harness's stdio JSONL protocol
//     ({"type":"generate","prompt"} / {"type":"cancel"} in;
//     {"type":"token"|"done"|"cancelled"} out). Each request runs
//     Plume's normal orchestration modules in product order: real
//     `prompts::assemble` (mode pin/message shaping), then the real
//     `chat::mlx_lm::stream_chat` client (Plume's own TCP + SSE
//     machinery, the product request body with its explicit
//     `max_tokens` cap, the product connect timeout and overall
//     budget). Timings are monotonic (`Instant`) and taken at the
//     UI-FACING EMISSION BOUNDARY — the exact point the product hands
//     a token to the webview, stdout standing in for the Tauri event
//     bridge. The webview transport/render hop is therefore NOT
//     included; docs/BENCHMARK_HARNESS.md states this boundary.
//     `--health` verifies arguments and prints the product's actual
//     output cap so the TS harness can refuse a config that declares
//     anything Plume does not really send.
//
// Failure honesty: a stream that ends without `[DONE]` or errors
// mid-flight emits a deliberately non-protocol frame — the harness
// classifies it malformed, exactly like any other protocol violation.

use std::io::{BufRead, Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::time::Instant;

use plume::chat::mlx_lm::{self, StreamOutcome, MAX_OUTPUT_TOKENS};
use plume::chat::{ChatMessage, ChatRole};
use plume::patch::{apply_patch, validate_patch, PatchApplyResponse, PatchValidateResponse};
use plume::prompts::{assemble, ChatMode};
use plume::{CHAT_OVERALL_BUDGET, CONNECT_TIMEOUT};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = match args.first().map(String::as_str) {
        Some("identity") => {
            println!("{}", identity_reply());
            0
        }
        Some("patch-check") => patch_check_main(),
        Some("orchestrate") => orchestrate_main(&args[1..]),
        _ => {
            eprintln!(
                "usage: plume_bench identity | plume_bench patch-check | plume_bench orchestrate --port N --model PATH [--health]"
            );
            2
        }
    };
    std::process::exit(code);
}

// ---- build identity ------------------------------------------------------

/// The git identity this binary was BUILT from, embedded by
/// src-tauri/build.rs (which reruns on every cargo invocation, so it
/// cannot go stale relative to the compiled code). The harness
/// compares this against the Plume identity it stamps on records and
/// refuses a stale or foreign sidecar. "unknown" (git unavailable at
/// build time) is reported as null — an unverifiable identity is a
/// refusal, never a guess.
fn build_identity() -> (Option<&'static str>, Option<bool>) {
    let sha = match env!("PLUME_BUILD_GIT_SHA") {
        "unknown" => None,
        sha => Some(sha),
    };
    let dirty = match env!("PLUME_BUILD_DIRTY") {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    };
    (sha, dirty)
}

/// The identity handshake: build identity plus the product's actual
/// output cap, so one probe verifies provenance AND the
/// declared-equals-wired generation posture.
fn identity_reply() -> String {
    let (sha, dirty) = build_identity();
    serde_json::json!({
        "ok": true,
        "gitSha": sha,
        "dirty": dirty,
        "maxOutputTokens": MAX_OUTPUT_TOKENS,
    })
    .to_string()
}

// ---- patch-check -------------------------------------------------------

fn patch_check_main() -> i32 {
    let mut input = String::new();
    if std::io::stdin().read_to_string(&mut input).is_err() {
        println!(
            "{}",
            serde_json::json!({ "ok": false, "error": "could not read stdin" })
        );
        return 1;
    }
    println!("{}", run_patch_check(&input));
    0
}

/// Pure request → verdict mapping (unit-tested below). The verdict
/// embeds Plume's own serialized `PatchValidateResponse` so the
/// harness records the product's real error taxonomy, not a retelling.
fn run_patch_check(input: &str) -> String {
    let parsed: Result<serde_json::Value, _> = serde_json::from_str(input);
    let request = match parsed {
        Ok(value) => value,
        Err(err) => {
            return serde_json::json!({ "ok": false, "error": format!("bad request JSON: {err}") })
                .to_string();
        }
    };
    let (Some(project_root), Some(diff)) = (
        request.get("projectRoot").and_then(|v| v.as_str()),
        request.get("diff").and_then(|v| v.as_str()),
    ) else {
        return serde_json::json!({ "ok": false, "error": "request needs projectRoot and diff strings" }).to_string();
    };
    let apply = request
        .get("apply")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    // The product canonicalizes the project root when it is opened
    // (project_open) and every patch verb receives that canonical
    // path; `safety::path::ensure_inside` requires it. Same here.
    let root = match Path::new(project_root).canonicalize() {
        Ok(root) if root.is_dir() => root,
        Ok(_) => {
            return serde_json::json!({ "ok": false, "error": format!("projectRoot is not a directory: {project_root}") })
                .to_string();
        }
        Err(err) => {
            return serde_json::json!({ "ok": false, "error": format!("projectRoot cannot be canonicalized: {err}") })
                .to_string();
        }
    };

    let validate = validate_patch(&root, diff);
    let valid = matches!(validate, PatchValidateResponse::Ok(_));
    let applied: Option<bool> = if apply && valid {
        Some(matches!(
            apply_patch(&root, diff),
            PatchApplyResponse::Ok(_)
        ))
    } else {
        None
    };
    serde_json::json!({
        "ok": true,
        "valid": valid,
        "applied": applied,
        "validate": serde_json::to_value(&validate).unwrap_or(serde_json::Value::Null),
    })
    .to_string()
}

// ---- orchestrate -------------------------------------------------------

struct OrchestrateArgs {
    port: u16,
    model: String,
    health: bool,
}

fn parse_orchestrate_args(args: &[String]) -> Result<OrchestrateArgs, String> {
    let mut port: Option<u16> = None;
    let mut model: Option<String> = None;
    let mut health = false;
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--port" => {
                let value = it.next().ok_or("--port needs a value")?;
                port = Some(
                    value
                        .parse::<u16>()
                        .map_err(|_| format!("bad --port {value}"))?,
                );
            }
            "--model" => {
                model = Some(it.next().ok_or("--model needs a value")?.clone());
            }
            "--health" => health = true,
            other => return Err(format!("unknown argument {other}")),
        }
    }
    Ok(OrchestrateArgs {
        port: port.ok_or("--port is required")?,
        model: model.ok_or("--model is required")?,
        health,
    })
}

fn orchestrate_main(args: &[String]) -> i32 {
    let parsed = match parse_orchestrate_args(args) {
        Ok(parsed) => parsed,
        Err(message) => {
            eprintln!("plume_bench orchestrate: {message}");
            return 2;
        }
    };
    if parsed.health {
        // Same reply as `identity`: build provenance + the product's
        // real output cap, so the harness can verify both before a
        // session serves anything.
        println!("{}", identity_reply());
        return 0;
    }

    // stdin thread: generate requests flow to the session loop; a
    // cancel frame trips the SAME AtomicBool mechanism the product's
    // `chat.cancel` uses on this adapter.
    let cancel = Arc::new(AtomicBool::new(false));
    let (tx, rx) = mpsc::channel::<String>();
    let cancel_for_reader = Arc::clone(&cancel);
    std::thread::spawn(move || {
        let stdin = std::io::stdin();
        for line in stdin.lock().lines() {
            let Ok(line) = line else { break };
            let Ok(frame) = serde_json::from_str::<serde_json::Value>(&line) else {
                continue;
            };
            match frame.get("type").and_then(|v| v.as_str()) {
                Some("generate") => {
                    let prompt = frame
                        .get("prompt")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    if tx.send(prompt).is_err() {
                        break;
                    }
                }
                Some("cancel") => cancel_for_reader.store(true, Ordering::SeqCst),
                _ => {}
            }
        }
    });

    let mut request_index: u64 = 0;
    while let Ok(prompt) = rx.recv() {
        cancel.store(false, Ordering::SeqCst);
        serve_one(&parsed, &prompt, request_index, &cancel);
        request_index += 1;
    }
    0
}

fn emit(frame: serde_json::Value) {
    let mut stdout = std::io::stdout().lock();
    if writeln!(stdout, "{frame}")
        .and_then(|()| stdout.flush())
        .is_err()
    {
        // The harness hung up; nothing sensible left to do.
        std::process::exit(0);
    }
}

fn ms(from: Instant, to: Instant) -> f64 {
    to.duration_since(from).as_secs_f64() * 1000.0
}

/// One measured request through the product path. `sent` is taken when
/// the generate frame is dequeued — Plume's prompt assembly is INSIDE
/// the measured window, because assembly is exactly the overhead this
/// measurement path exists to observe.
fn serve_one(args: &OrchestrateArgs, prompt: &str, request_index: u64, cancel: &Arc<AtomicBool>) {
    let sent = Instant::now();
    let user_messages = vec![ChatMessage {
        role: ChatRole::User,
        content: prompt.to_string(),
    }];
    let assembled = match assemble(None, &user_messages, None, ChatMode::Chat) {
        Ok(assembled) => assembled,
        Err(err) => {
            emit(
                serde_json::json!({ "type": "error", "message": format!("assemble failed: {err:?}") }),
            );
            return;
        }
    };

    let deadline = sent + CHAT_OVERALL_BUDGET;
    let mut first_token: Option<Instant> = None;
    let outcome = mlx_lm::stream_chat(
        args.port,
        &args.model,
        &assembled.messages,
        Arc::clone(cancel),
        |delta| {
            // Timestamp BEFORE handing the token to the consumer: this
            // is the UI-facing emission boundary.
            if first_token.is_none() {
                first_token = Some(Instant::now());
            }
            emit(serde_json::json!({ "type": "token", "text": delta }));
        },
        CONNECT_TIMEOUT,
        deadline,
    );

    match outcome {
        Ok(StreamOutcome::Done { stats, .. }) => {
            let finished = Instant::now();
            let mut report = serde_json::json!({
                "endToEndMs": ms(sent, finished),
                "requestIndex": request_index,
            });
            if let Some(at) = first_token {
                report["ttftMs"] = serde_json::json!(ms(sent, at));
                report["generationDurationMs"] = serde_json::json!(ms(at, finished));
            }
            if let Some(prompt_tokens) = stats.prompt_tokens {
                report["promptTokens"] = serde_json::json!(prompt_tokens);
            }
            if let Some(completion_tokens) = stats.completion_tokens {
                report["outputTokens"] = serde_json::json!(completion_tokens);
            }
            emit(serde_json::json!({ "type": "done", "report": report }));
        }
        Ok(StreamOutcome::Cancelled { .. }) => {
            emit(serde_json::json!({ "type": "cancelled" }));
        }
        Ok(StreamOutcome::EofBeforeDone { .. }) => {
            // Deliberately not a protocol frame: the harness must
            // classify a stream that died without [DONE] as malformed.
            emit(serde_json::json!({ "type": "eofBeforeDone" }));
        }
        Err(err) => {
            emit(serde_json::json!({ "type": "error", "message": format!("{err}") }));
        }
    }
}

// ---- tests -------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    const GOOD_DIFF: &str = "--- a/hello.txt\n+++ b/hello.txt\n@@ -1,1 +1,1 @@\n-hello\n+goodbye\n";

    fn fixture_root() -> tempdir::TempDirLike {
        tempdir::make("plume-bench-check")
    }

    /// Minimal tempdir helper — std-only, no new dependencies.
    mod tempdir {
        use std::path::PathBuf;

        pub struct TempDirLike(pub PathBuf);
        impl TempDirLike {
            pub fn path(&self) -> &std::path::Path {
                &self.0
            }
        }
        impl Drop for TempDirLike {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
        pub fn make(prefix: &str) -> TempDirLike {
            let unique = format!(
                "{prefix}-{}-{:?}",
                std::process::id(),
                std::time::Instant::now()
            );
            let sanitized: String = unique
                .chars()
                .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
                .collect();
            let dir = std::env::temp_dir().join(sanitized);
            std::fs::create_dir_all(&dir).expect("create temp dir");
            TempDirLike(dir)
        }
    }

    fn request(root: &std::path::Path, diff: &str, apply: bool) -> String {
        serde_json::json!({ "projectRoot": root.to_string_lossy(), "diff": diff, "apply": apply })
            .to_string()
    }

    #[test]
    fn patch_check_valid_diff_reports_valid_without_apply() {
        let root = fixture_root();
        fs::write(root.path().join("hello.txt"), "hello\n").unwrap();
        let verdict: serde_json::Value =
            serde_json::from_str(&run_patch_check(&request(root.path(), GOOD_DIFF, false)))
                .unwrap();
        assert_eq!(verdict["ok"], true);
        assert_eq!(verdict["valid"], true);
        assert_eq!(verdict["applied"], serde_json::Value::Null);
        // The embedded response is Plume's own shape.
        assert_eq!(verdict["validate"]["ok"], true);
        assert_eq!(verdict["validate"]["hunks"], 1);
    }

    #[test]
    fn patch_check_applies_through_plumes_real_applier() {
        let root = fixture_root();
        fs::write(root.path().join("hello.txt"), "hello\n").unwrap();
        let verdict: serde_json::Value =
            serde_json::from_str(&run_patch_check(&request(root.path(), GOOD_DIFF, true))).unwrap();
        assert_eq!(verdict["valid"], true);
        assert_eq!(verdict["applied"], true);
        assert_eq!(
            fs::read_to_string(root.path().join("hello.txt")).unwrap(),
            "goodbye\n"
        );
    }

    #[test]
    fn patch_check_rejects_a_path_escape_with_plumes_taxonomy() {
        let root = fixture_root();
        let escape = "--- a/../evil.txt\n+++ b/../evil.txt\n@@ -1,1 +1,1 @@\n-x\n+y\n";
        let verdict: serde_json::Value =
            serde_json::from_str(&run_patch_check(&request(root.path(), escape, true))).unwrap();
        assert_eq!(verdict["ok"], true);
        assert_eq!(verdict["valid"], false);
        // Apply must never run after a failed validation.
        assert_eq!(verdict["applied"], serde_json::Value::Null);
        assert_eq!(verdict["validate"]["ok"], false);
        assert_eq!(verdict["validate"]["errors"][0]["kind"], "pathEscape");
    }

    #[test]
    fn patch_check_reports_a_pre_image_mismatch_as_apply_failure() {
        let root = fixture_root();
        // File content does NOT match the diff's pre-image: validation
        // (shape/paths) passes, Plume's applier refuses.
        fs::write(root.path().join("hello.txt"), "something else\n").unwrap();
        let verdict: serde_json::Value =
            serde_json::from_str(&run_patch_check(&request(root.path(), GOOD_DIFF, true))).unwrap();
        assert_eq!(verdict["valid"], true);
        assert_eq!(verdict["applied"], false);
    }

    #[test]
    fn patch_check_refuses_bad_requests_without_panicking() {
        let bad: serde_json::Value = serde_json::from_str(&run_patch_check("{not json")).unwrap();
        assert_eq!(bad["ok"], false);
        let missing: serde_json::Value = serde_json::from_str(&run_patch_check("{}")).unwrap();
        assert_eq!(missing["ok"], false);
    }

    #[test]
    fn identity_reply_reports_build_provenance_and_the_product_cap() {
        let reply: serde_json::Value = serde_json::from_str(&identity_reply()).unwrap();
        assert_eq!(reply["ok"], true);
        assert_eq!(reply["maxOutputTokens"], MAX_OUTPUT_TOKENS);
        // Built in a git checkout: a real 40-hex sha and a boolean
        // dirty flag. (A git-less build reports null and the harness
        // refuses — that path cannot be exercised from inside a
        // checkout, which is the point.)
        let sha = reply["gitSha"]
            .as_str()
            .expect("gitSha must be a string in a git checkout");
        assert_eq!(sha.len(), 40);
        assert!(sha.chars().all(|c| c.is_ascii_hexdigit()));
        assert!(reply["dirty"].is_boolean());
    }

    #[test]
    fn orchestrate_args_parse_and_refuse() {
        let ok = parse_orchestrate_args(&[
            "--port".into(),
            "8080".into(),
            "--model".into(),
            "/m".into(),
        ])
        .unwrap();
        assert_eq!(ok.port, 8080);
        assert_eq!(ok.model, "/m");
        assert!(!ok.health);
        assert!(parse_orchestrate_args(&["--port".into(), "8080".into()]).is_err());
        assert!(parse_orchestrate_args(&[
            "--port".into(),
            "notaport".into(),
            "--model".into(),
            "/m".into()
        ])
        .is_err());
        assert!(parse_orchestrate_args(&["--bogus".into()]).is_err());
    }
}
