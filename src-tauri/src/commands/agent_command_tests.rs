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

    // The single-step base prompt is [system, user]; apply_attachment folds
    // the file into the trailing user message — same path chat.send uses.
    let base = build_propose_diff_messages("summarize the notes");
    let req = attachment_to_request(&AttachmentPayload::ProjectFile {
        rel_path: "notes.txt".to_string(),
        start_line: None,
        end_line: None,
    });
    let (folded, summary) = apply_attachment(&root, &base, req).expect("fold ok");

    assert_eq!(folded.len(), 2, "still system + user");
    assert_eq!(folded[0].role, ChatRole::System);
    assert_eq!(folded[1].role, ChatRole::User);
    // The instruction AND the file content ride in the final user message.
    assert!(folded[1].content.contains("summarize the notes"));
    assert!(folded[1].content.contains("notes.txt"));
    assert!(folded[1].content.contains("beta"));
    assert_eq!(summary.expect("summary").rel_path, "notes.txt");
}

#[test]
fn single_step_attachment_applies_the_secret_redaction_gate() {
    // The same secret-filename block chat.send enforces must reject on the
    // single-step path too — folding context can't smuggle a `.env`.
    let td = TempDir::new("secret");
    let root = fs::canonicalize(&td.path).expect("canonicalize");
    fs::write(root.join(".env"), "TOKEN=shh\n").unwrap();

    let base = build_propose_diff_messages("read the env");
    let req = attachment_to_request(&AttachmentPayload::ProjectFile {
        rel_path: ".env".to_string(),
        start_line: None,
        end_line: None,
    });
    let res = apply_attachment(&root, &base, req);
    assert!(
        matches!(res, Err(IpcError::Blocked(_))),
        "a secret-filename attachment must be Blocked, got {res:?}"
    );
}
