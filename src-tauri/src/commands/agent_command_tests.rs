//! Tests for the agent IPC commands (D93 + D96). The event ordering /
//! terminal state live in `agent::dry_run` and `agent::single_step` tests;
//! here we pin the commands' wire shapes — the events array the frontend
//! renders, the single-step payload contract, and the `validate_patch` →
//! `ValidateSummary` bridge. The full single-step command needs a live MLX
//! model, so its end-to-end path is covered by the Qwen smoke scripts, not
//! the in-container suite.

use super::*;
use crate::chat::ChatRole;
use crate::error::IPC_VERSION;
use std::fs;
use std::path::PathBuf;

#[test]
fn response_serializes_events_as_flattened_envelopes() {
    let resp = AgentDryRunResponse {
        events: scripted_dry_run(1_700_000_000_000),
    };
    let v = serde_json::to_value(&resp).unwrap();
    let events = v["events"].as_array().expect("events array");
    assert!(!events.is_empty());

    // First frame: seq + tsMs + a flattened event (kind + payload).
    let first = &events[0];
    assert_eq!(first["seq"], 0);
    assert!(first["tsMs"].is_number());
    assert!(first["kind"].is_string());

    // Last frame is the terminal done.
    let last = events.last().unwrap();
    assert_eq!(last["kind"], "done");
}

#[test]
fn response_is_camel_case_and_carries_call_ids() {
    let resp = AgentDryRunResponse {
        events: scripted_dry_run(1),
    };
    let v = serde_json::to_value(&resp).unwrap();
    // A toolProposed frame uses camelCase callId on the wire.
    let proposed = v["events"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["kind"] == "toolProposed")
        .expect("a toolProposed frame");
    assert!(proposed["callId"].is_string());
}

// ─── D96: agent.singleStep ───────────────────────────────────────────────

#[test]
fn single_step_payload_deserialises_camel_case() {
    let raw = serde_json::json!({
        "prompt": "make greet return an f-string",
        "providerId": "mlx-lm",
        "modelId": "qwen2.5-coder-3b",
        "handleId": "srv_0000000000000001",
    });
    let p: AgentSingleStepPayload = serde_json::from_value(raw).unwrap();
    assert_eq!(p.provider_id, "mlx-lm");
    assert_eq!(p.handle_id, "srv_0000000000000001");
}

#[test]
fn single_step_payload_rejects_unknown_field() {
    let raw = serde_json::json!({
        "prompt": "x",
        "providerId": "mlx-lm",
        "modelId": "m",
        "handleId": "h",
        "rogue": true,
    });
    let res = serde_json::from_value::<AgentSingleStepPayload>(raw);
    assert!(res.is_err(), "unknown field must be rejected: {res:?}");
}

#[test]
fn single_step_payload_round_trips_through_the_envelope() {
    let raw = serde_json::json!({
        "ipcVersion": IPC_VERSION,
        "payload": {
            "prompt": "x",
            "providerId": "mlx-lm",
            "modelId": "m",
            "handleId": "h",
        },
    });
    let req: IpcRequest<AgentSingleStepPayload> = serde_json::from_value(raw).unwrap();
    req.check_version().unwrap();
    assert_eq!(req.payload.prompt, "x");
}

#[test]
fn propose_diff_messages_are_system_then_user() {
    let msgs = build_propose_diff_messages("rename foo to bar");
    assert_eq!(msgs.len(), 2);
    assert_eq!(msgs[0].role, ChatRole::System);
    assert!(msgs[0].content.contains("unified diff"));
    assert!(msgs[0].content.contains("TOOL_REQUEST"));
    assert_eq!(msgs[1].role, ChatRole::User);
    assert_eq!(msgs[1].content, "rename foo to bar");
}

// ─── summarize_validate: the validate_patch → ValidateSummary bridge ─────

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
            "plume-single-step-{}-{}-{}",
            label,
            std::process::id(),
            nanos
        ));
        fs::create_dir_all(&path).expect("create tempdir");
        Self { path }
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

#[test]
fn summarize_validate_reports_a_valid_create_diff() {
    let td = TempDir::new("valid");
    let root = fs::canonicalize(&td.path).expect("canonicalize");
    // A create diff: no pre-existing file needed, just a real root.
    let diff = "--- /dev/null\n\
        +++ b/new.txt\n\
        @@ -0,0 +1,1 @@\n\
        +hello\n";
    let summary = summarize_validate(validate_patch(&root, diff));
    assert!(summary.valid, "create diff should validate: {summary:?}");
    assert_eq!(summary.paths, vec!["new.txt".to_string()]);
    assert!(summary.detail.contains("1 file"));
    assert!(summary.detail.contains("1 hunk"));
}

#[test]
fn summarize_validate_reports_an_invalid_diff() {
    let td = TempDir::new("invalid");
    let root = fs::canonicalize(&td.path).expect("canonicalize");
    // Path escape — rejected before any write.
    let diff = "--- a/../../etc/passwd\n\
        +++ b/../../etc/passwd\n\
        @@ -1,1 +1,1 @@\n\
        -x\n\
        +y\n";
    let summary = summarize_validate(validate_patch(&root, diff));
    assert!(!summary.valid, "path escape must be invalid");
    assert!(summary.paths.is_empty());
    assert!(!summary.detail.is_empty());
}

// ─── D99: optional read-only file attachment on the single step ──────────

#[test]
fn single_step_payload_accepts_an_attachment_with_a_line_range() {
    let raw = serde_json::json!({
        "prompt": "tidy greet",
        "providerId": "mlx-lm",
        "modelId": "m",
        "handleId": "h",
        "attachment": {
            "kind": "projectFile",
            "relPath": "src/greet.rs",
            "startLine": 2,
            "endLine": 5,
        },
    });
    let p: AgentSingleStepPayload = serde_json::from_value(raw).unwrap();
    let att = p.attachment.expect("attachment present");
    let crate::prompts::AttachmentRequest::ProjectFile {
        rel_path,
        line_range,
    } = attachment_to_request(&att);
    assert_eq!(rel_path, "src/greet.rs");
    let r = line_range.expect("line range");
    assert_eq!((r.start, r.end), (2, 5));
}

#[test]
fn single_step_payload_attachment_is_optional() {
    // The D96 wire (no attachment) must still deserialise unchanged.
    let raw = serde_json::json!({
        "prompt": "x",
        "providerId": "mlx-lm",
        "modelId": "m",
        "handleId": "h",
    });
    let p: AgentSingleStepPayload = serde_json::from_value(raw).unwrap();
    assert!(p.attachment.is_none());
}

#[test]
fn single_step_folds_an_attachment_into_the_user_message() {
    let td = TempDir::new("attach");
    let root = fs::canonicalize(&td.path).expect("canonicalize");
    fs::write(root.join("notes.txt"), "alpha\nbeta\ngamma\n").unwrap();

    // The real command path: build [system, user], then fold the file in
    // (validate → resolve → redact → wrap) via `fold_attachment`.
    let base = build_propose_diff_messages("summarize the notes");
    let att = AttachmentPayload::ProjectFile {
        rel_path: "notes.txt".to_string(),
        start_line: None,
        end_line: None,
    };
    let folded = fold_attachment(&root, base, Some(&att)).expect("fold ok");

    assert_eq!(folded.len(), 2, "still system + user");
    assert_eq!(folded[0].role, ChatRole::System);
    assert_eq!(folded[1].role, ChatRole::User);
    // The instruction AND the file content ride in the final user message.
    assert!(folded[1].content.contains("summarize the notes"));
    assert!(folded[1].content.contains("notes.txt"));
    assert!(folded[1].content.contains("beta"));
}

#[test]
fn single_step_attachment_none_is_a_no_op() {
    let td = TempDir::new("noop");
    let root = fs::canonicalize(&td.path).expect("canonicalize");
    let folded =
        fold_attachment(&root, build_propose_diff_messages("just do it"), None).expect("no-op ok");
    assert_eq!(folded.len(), 2);
    assert_eq!(
        folded[1].content, "just do it",
        "no attachment leaves the prompt untouched"
    );
}

#[test]
fn single_step_attachment_applies_the_secret_redaction_gate() {
    // The same secret-filename block chat.send enforces must reject on the
    // single-step path too — folding context can't smuggle a `.env`.
    let td = TempDir::new("secret");
    let root = fs::canonicalize(&td.path).expect("canonicalize");
    fs::write(root.join(".env"), "TOKEN=shh\n").unwrap();

    let base = build_propose_diff_messages("read the env");
    let att = AttachmentPayload::ProjectFile {
        rel_path: ".env".to_string(),
        start_line: None,
        end_line: None,
    };
    assert!(
        matches!(
            fold_attachment(&root, base, Some(&att)),
            Err(IpcError::Blocked(_))
        ),
        "a secret-filename attachment must be Blocked"
    );
}

// Codex PR #82 review (MEDIUM): the single-step fold must run the same shape
// validator chat.send does, BEFORE `attachment_to_request` — otherwise a
// half range silently becomes whole-file and a zero start underflows
// `slice_lines`' `start - 1`. These pin both rejections on the agent path.

#[test]
fn single_step_attachment_rejects_a_half_range() {
    let td = TempDir::new("half");
    let root = fs::canonicalize(&td.path).expect("canonicalize");
    fs::write(root.join("f.txt"), "a\nb\nc\n").unwrap();
    let att = AttachmentPayload::ProjectFile {
        rel_path: "f.txt".to_string(),
        start_line: Some(2),
        end_line: None,
    };
    assert!(
        matches!(
            fold_attachment(&root, build_propose_diff_messages("x"), Some(&att)),
            Err(IpcError::BadArgument(_))
        ),
        "half a line range must be rejected, not silently treated as whole-file"
    );
}

#[test]
fn single_step_attachment_rejects_a_zero_start_line() {
    let td = TempDir::new("zero");
    let root = fs::canonicalize(&td.path).expect("canonicalize");
    fs::write(root.join("f.txt"), "a\nb\nc\n").unwrap();
    let att = AttachmentPayload::ProjectFile {
        rel_path: "f.txt".to_string(),
        start_line: Some(0),
        end_line: Some(1),
    };
    // Rejected by validate_attachment BEFORE slice_lines' `start - 1` runs.
    assert!(
        matches!(
            fold_attachment(&root, build_propose_diff_messages("x"), Some(&att)),
            Err(IpcError::BadArgument(_))
        ),
        "a zero start line must be rejected before the slice underflow"
    );
}

// ─── D100: applicable-diff handoff for the explicit apply ────────────────

#[test]
fn applicable_diff_is_some_only_for_a_valid_propose_diff() {
    let diff = "--- a/x\n+++ b/x\n@@ -1 +1 @@\n-a\n+b\n";
    let propose = ProposedAction::ProposeDiff {
        diff: diff.to_string(),
    };
    // A propose-diff that validated → the diff is offered for apply.
    assert_eq!(applicable_diff(&propose, true).as_deref(), Some(diff));
    // A propose-diff that did NOT validate → nothing to apply.
    assert!(applicable_diff(&propose, false).is_none());
    // Non-diff actions never offer an apply, even with `valid = true`.
    assert!(applicable_diff(
        &ProposedAction::UnsupportedTool {
            name: "shell".to_string()
        },
        true
    )
    .is_none());
    assert!(applicable_diff(&ProposedAction::NoAction, true).is_none());
}

#[test]
fn single_step_response_serializes_applicable_diff_camel_case() {
    let resp = AgentSingleStepResponse {
        events: vec![],
        applicable_diff: Some("the diff".to_string()),
    };
    let v = serde_json::to_value(&resp).unwrap();
    assert_eq!(v["applicableDiff"], "the diff");

    // None serialises to JSON null (the frontend reads it as "no apply").
    let none = AgentSingleStepResponse {
        events: vec![],
        applicable_diff: None,
    };
    assert!(serde_json::to_value(&none).unwrap()["applicableDiff"].is_null());
}
