use super::*;
use crate::chat::{ChatMessage, ChatRole};
use crate::safety::path::canonicalize_root;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

struct TempDir {
    path: PathBuf,
}
impl TempDir {
    fn new(label: &str) -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "plume-assemble-{}-{}-{}",
            label,
            std::process::id(),
            nanos
        ));
        fs::create_dir_all(&path).unwrap();
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

fn user_msg(s: &str) -> ChatMessage {
    ChatMessage {
        role: ChatRole::User,
        content: s.to_string(),
    }
}
fn assistant_msg(s: &str) -> ChatMessage {
    ChatMessage {
        role: ChatRole::Assistant,
        content: s.to_string(),
    }
}

/// D15: test-only shim around `assemble_chat(...)` that defaults
/// the new `mode` argument to `ChatMode::Chat`. Lets the
/// existing D7.1 / D8 / D10 / D11 tests stay short — they
/// don't care about the mode field, they're exercising the
/// attachment + instructions plumbing. The propose-diff tests
/// below call `assemble_chat(...)` directly with an explicit mode.
fn assemble_chat(
    project_root: Option<&Path>,
    messages: &[ChatMessage],
    attachment: Option<AttachmentRequest>,
) -> Result<AssembledPrompt, IpcError> {
    assemble(project_root, messages, attachment, ChatMode::Chat)
}

#[test]
fn passes_through_when_no_attachment() {
    let td = TempDir::new("noattach");
    let root = canonicalize_root(td.path()).unwrap();
    let msgs = vec![user_msg("hi"), assistant_msg("hello"), user_msg("again")];
    let out = assemble_chat(Some(&root), &msgs, None).expect("ok");
    assert!(out.attachment.is_none());
    assert_eq!(out.messages.len(), msgs.len());
    assert_eq!(out.messages[2].content, "again");
}

fn whole_file(rel: &str) -> AttachmentRequest {
    AttachmentRequest::ProjectFile {
        rel_path: rel.into(),
        line_range: None,
    }
}

fn range(rel: &str, start: u32, end: u32) -> AttachmentRequest {
    AttachmentRequest::ProjectFile {
        rel_path: rel.into(),
        line_range: Some(LineRange { start, end }),
    }
}

#[test]
fn wraps_only_last_user_message() {
    let td = TempDir::new("wraplast");
    let root = canonicalize_root(td.path()).unwrap();
    fs::write(td.path().join("hello.txt"), "world").unwrap();

    let msgs = vec![
        user_msg("first turn"),
        assistant_msg("first reply"),
        user_msg("explain"),
    ];
    let out = assemble_chat(Some(&root), &msgs, Some(whole_file("hello.txt"))).expect("ok");

    // History unchanged.
    assert_eq!(out.messages[0].content, "first turn");
    assert_eq!(out.messages[1].content, "first reply");
    // Last user message gets the wrapper + the original input.
    let last = &out.messages[2].content;
    assert!(last.contains("Attached file (read-only context): hello.txt"));
    assert!(last.contains("----- FILE BEGIN -----"));
    assert!(last.contains("world"));
    assert!(last.contains("----- FILE END -----"));
    assert!(last.ends_with("explain"));
    let summary = out.attachment.as_ref().expect("attached");
    assert_eq!(summary.rel_path, "hello.txt");
    // Whole-file attach surfaces line_range == None in the
    // summary so logs can distinguish "user wanted the whole
    // thing" from "user wanted lines 1..N where N == file end".
    assert_eq!(summary.line_range, None);
}

#[test]
fn surfaces_redaction_count_in_summary() {
    let td = TempDir::new("redsum");
    let root = canonicalize_root(td.path()).unwrap();
    // Deliberate fake — the literal is the test input we expect
    // the redactor to catch. `gitleaks:allow` marks both the
    // write-side and the negative assertion below.
    fs::write(
        td.path().join("secrets.txt"),
        "OPENAI_API_KEY=sk-1234567890abcdef1234567890abcdef\n", // gitleaks:allow
    )
    .unwrap();

    let msgs = vec![user_msg("what's in this file?")];
    let out = assemble_chat(Some(&root), &msgs, Some(whole_file("secrets.txt"))).expect("ok");
    let sum = out.attachment.as_ref().expect("attached");
    assert_eq!(sum.redaction_count, 1);
    // The wrapped message must NOT contain the secret literal.
    let last = &out.messages[0].content;
    assert!(!last.contains("sk-1234567890abcdef1234567890abcdef")); // gitleaks:allow
    assert!(last.contains("[REDACTED:api-key]"));
}

#[test]
fn rejects_secret_filename_attachment() {
    let td = TempDir::new("envattach");
    let root = canonicalize_root(td.path()).unwrap();
    fs::write(td.path().join(".env"), "X=1").unwrap();

    let msgs = vec![user_msg("read .env")];
    let err = assemble_chat(Some(&root), &msgs, Some(whole_file(".env"))).unwrap_err();
    assert!(matches!(err, IpcError::Blocked(_)), "got {err:?}");
}

#[test]
fn rejects_path_escape_attachment() {
    let td = TempDir::new("escape");
    let root = canonicalize_root(td.path()).unwrap();
    // `../<sibling>` resolves outside the project root.
    let msgs = vec![user_msg("read")];
    let err = assemble_chat(Some(&root), &msgs, Some(whole_file("../oops.txt"))).unwrap_err();
    // PathEscape from ensure_inside, or NotFound if the parent
    // doesn't exist — both are correct rejections for an escape
    // attempt and both surface as typed IpcError. Treat either
    // as pass.
    match err {
        IpcError::PathEscape(_) | IpcError::NotFound(_) => {}
        other => panic!("expected PathEscape or NotFound, got {other:?}"),
    }
}

#[test]
fn rejects_when_last_message_is_assistant() {
    // The chat handler's payload validation already rejects
    // this shape, but if a refactor moves that check we want
    // assemble to fail safe rather than silently wrap an
    // assistant turn.
    let td = TempDir::new("trailassist");
    let root = canonicalize_root(td.path()).unwrap();
    fs::write(td.path().join("a.txt"), "x").unwrap();

    let msgs = vec![user_msg("first"), assistant_msg("answer")];
    let err = assemble_chat(Some(&root), &msgs, Some(whole_file("a.txt"))).unwrap_err();
    assert!(matches!(err, IpcError::BadArgument(_)), "got {err:?}");
}

#[test]
fn wrapped_content_appends_newline_when_file_missing_trailing_newline() {
    // A file like "no-newline-eof" shouldn't run into the
    // ----- FILE END ----- marker.
    let td = TempDir::new("nonl");
    let root = canonicalize_root(td.path()).unwrap();
    fs::write(td.path().join("nl.txt"), "no newline").unwrap();

    let msgs = vec![user_msg("look")];
    let out = assemble_chat(Some(&root), &msgs, Some(whole_file("nl.txt"))).expect("ok");
    let last = &out.messages[0].content;
    assert!(last.contains("no newline\n----- FILE END -----"));
}

// ---- D10 line-range slicing ----

#[test]
fn line_range_keeps_only_requested_lines_and_labels_header() {
    // 6-line file; ask for lines 2–4. The wrapped content must
    // contain those lines verbatim and NOT contain the lines
    // outside the range. The header line in the wrapper picks
    // up the "(lines 2–4)" label.
    let td = TempDir::new("range");
    let root = canonicalize_root(td.path()).unwrap();
    fs::write(
        td.path().join("six.txt"),
        "alpha\nbeta\ngamma\ndelta\nepsilon\nzeta\n",
    )
    .unwrap();

    let msgs = vec![user_msg("look at the middle")];
    let out = assemble_chat(Some(&root), &msgs, Some(range("six.txt", 2, 4))).expect("ok");
    let body = &out.messages[0].content;
    assert!(body.contains("(lines 2\u{2013}4)"), "body was: {body}");
    assert!(body.contains("beta\ngamma\ndelta\n"));
    // Lines outside the range must not leak.
    assert!(!body.contains("alpha"));
    assert!(!body.contains("epsilon"));
    assert!(!body.contains("zeta"));
    let summary = out.attachment.as_ref().expect("attached");
    assert_eq!(summary.line_range, Some(LineRange { start: 2, end: 4 }));
}

#[test]
fn line_range_single_line_uses_singular_label() {
    let td = TempDir::new("singleline");
    let root = canonicalize_root(td.path()).unwrap();
    fs::write(td.path().join("three.txt"), "first\nsecond\nthird\n").unwrap();

    let msgs = vec![user_msg("focus")];
    let out = assemble_chat(Some(&root), &msgs, Some(range("three.txt", 2, 2))).expect("ok");
    let body = &out.messages[0].content;
    assert!(body.contains("(line 2)"), "body was: {body}");
    assert!(body.contains("second"));
    assert!(!body.contains("first"));
    assert!(!body.contains("third"));
}

#[test]
fn line_range_redacts_before_slicing() {
    // The redactor runs over the full file; slicing then picks
    // the requested lines from the already-redacted text. A
    // secret on a line outside the range never appears regardless
    // — and a secret on a line INSIDE the range shows the marker,
    // not the original.
    let td = TempDir::new("redactslice");
    let root = canonicalize_root(td.path()).unwrap();
    fs::write(
        td.path().join("mixed.txt"),
        // Line 1: plain
        // Line 2: contains a key
        // Line 3: plain
        "untouched\nOPENAI_API_KEY=sk-1234567890abcdef1234567890abcdef\nepilogue\n", // gitleaks:allow
    )
    .unwrap();

    let msgs = vec![user_msg("look at the middle")];
    let out = assemble_chat(Some(&root), &msgs, Some(range("mixed.txt", 2, 2))).expect("ok");
    let body = &out.messages[0].content;
    assert!(body.contains("[REDACTED:api-key]"));
    assert!(!body.contains("sk-1234567890abcdef1234567890abcdef")); // gitleaks:allow
                                                                    // The other lines must not leak through the slice.
    assert!(!body.contains("untouched"));
    assert!(!body.contains("epilogue"));
}

#[test]
fn line_range_end_past_eof_rejects_with_bad_argument() {
    let td = TempDir::new("rangepast");
    let root = canonicalize_root(td.path()).unwrap();
    fs::write(td.path().join("two.txt"), "a\nb\n").unwrap();

    let msgs = vec![user_msg("?")];
    let err = assemble_chat(Some(&root), &msgs, Some(range("two.txt", 1, 99))).unwrap_err();
    match err {
        IpcError::BadArgument(msg) => {
            assert!(msg.contains("endLine"), "msg was: {msg}");
            assert!(msg.contains("past"), "msg was: {msg}");
        }
        other => panic!("expected BadArgument, got {other:?}"),
    }
}

#[test]
fn line_range_start_past_eof_rejects_with_bad_argument() {
    let td = TempDir::new("rangestart");
    let root = canonicalize_root(td.path()).unwrap();
    fs::write(td.path().join("two.txt"), "a\nb\n").unwrap();

    let msgs = vec![user_msg("?")];
    let err = assemble_chat(Some(&root), &msgs, Some(range("two.txt", 5, 6))).unwrap_err();
    match err {
        IpcError::BadArgument(msg) => assert!(msg.contains("startLine"), "msg was: {msg}"),
        other => panic!("expected BadArgument, got {other:?}"),
    }
}

// ---- D11 project instructions ----

#[test]
fn prepends_system_message_when_agents_md_present() {
    let td = TempDir::new("instr-present");
    fs::write(
        td.path().join("AGENTS.md"),
        "# Plume rules\n\nNo writes without approval.\n",
    )
    .unwrap();
    let root = canonicalize_root(td.path()).unwrap();
    let msgs = vec![user_msg("hello")];

    let out = assemble_chat(Some(&root), &msgs, None).expect("ok");
    // System message prepended, original message preserved.
    assert_eq!(out.messages.len(), 2);
    assert!(matches!(out.messages[0].role, ChatRole::System));
    assert!(matches!(out.messages[1].role, ChatRole::User));
    let sys = &out.messages[0].content;
    assert!(sys.starts_with("Project instructions (read-only, from AGENTS.md"));
    assert!(sys.contains("No writes without approval."));
    assert_eq!(out.messages[1].content, "hello");
    let summary = out.instructions.expect("instructions summary");
    assert_eq!(summary.source, "AGENTS.md");
    assert_eq!(summary.redaction_count, 0);
    assert!(summary.original_bytes > 0);
}

#[test]
fn no_system_message_when_agents_md_absent() {
    let td = TempDir::new("instr-absent");
    let root = canonicalize_root(td.path()).unwrap();
    let msgs = vec![user_msg("hello")];
    let out = assemble_chat(Some(&root), &msgs, None).expect("ok");
    assert_eq!(out.messages.len(), 1);
    assert!(matches!(out.messages[0].role, ChatRole::User));
    assert!(out.instructions.is_none());
}

#[test]
fn no_system_message_when_project_root_is_none() {
    // Plain chat without a trusted project — the D7.1 path —
    // must not try to read AGENTS.md from anywhere.
    let msgs = vec![user_msg("hello")];
    let out = assemble_chat(None, &msgs, None).expect("ok");
    assert_eq!(out.messages.len(), 1);
    assert!(out.instructions.is_none());
    assert!(out.attachment.is_none());
}

#[test]
fn assemble_rejects_attachment_without_project_root() {
    // The chat handler is responsible for rejecting this shape
    // with `NeedsApproval` before reaching the assembler.
    // Earlier drafts used `debug_assert!` + `expect()` which
    // would panic in release builds if the caller's contract
    // ever slipped; the typed `Internal` error keeps the
    // failure mode honest in any build profile.
    let msgs = vec![user_msg("hi")];
    let err = assemble_chat(None, &msgs, Some(whole_file("anything.rs")))
        .expect_err("attachment without project_root must surface a typed error");
    match err {
        IpcError::Internal(msg) => {
            assert!(msg.contains("project_root"), "msg was: {msg}");
        }
        other => panic!("expected Internal, got {other:?}"),
    }
}

#[test]
fn instructions_and_attachment_compose() {
    // Both features active: instructions ride as a leading
    // system message AND the last user message gets the file
    // attachment wrap.
    let td = TempDir::new("instr+attach");
    fs::write(td.path().join("AGENTS.md"), "Be careful.\n").unwrap();
    fs::write(td.path().join("hello.txt"), "hi from file\n").unwrap();
    let root = canonicalize_root(td.path()).unwrap();
    let msgs = vec![user_msg("look at the file")];

    let out = assemble_chat(Some(&root), &msgs, Some(whole_file("hello.txt"))).expect("ok");
    assert_eq!(out.messages.len(), 2);
    assert!(matches!(out.messages[0].role, ChatRole::System));
    assert!(out.messages[0].content.contains("Be careful."));
    let last = &out.messages[1].content;
    assert!(last.contains("Attached file (read-only context): hello.txt"));
    assert!(last.contains("hi from file"));
    assert!(last.ends_with("look at the file"));
    assert!(out.attachment.is_some());
    assert!(out.instructions.is_some());
}

#[test]
fn instructions_redactor_runs_over_agents_md_content() {
    // Belt-and-suspenders: a stray API-key shape in AGENTS.md
    // gets the same [REDACTED:api-key] treatment as a file
    // attachment.
    let td = TempDir::new("instr-redact");
    // Deliberate fake — `gitleaks:allow` must sit on the same
    // line as the literal for the pre-commit scanner.
    let raw = "Test key: sk-1234567890abcdef1234567890abcdef\n"; // gitleaks:allow
    fs::write(td.path().join("AGENTS.md"), raw).unwrap();
    let root = canonicalize_root(td.path()).unwrap();
    let msgs = vec![user_msg("hi")];

    let out = assemble_chat(Some(&root), &msgs, None).expect("ok");
    let sys = &out.messages[0].content;
    assert!(!sys.contains("sk-1234567890abcdef1234567890abcdef")); // gitleaks:allow
    assert!(sys.contains("[REDACTED:api-key]"));
    let summary = out.instructions.expect("attached");
    assert_eq!(summary.redaction_count, 1);
}

#[test]
fn instructions_re_read_picks_up_edits_between_calls() {
    // Ollama is stateless across /api/chat, so the assembler
    // re-reads AGENTS.md on every call. Verifies that two
    // consecutive assemble calls reflect a mid-session edit
    // to AGENTS.md.
    let td = TempDir::new("instr-reread");
    let agents = td.path().join("AGENTS.md");
    fs::write(&agents, "v1: original rule\n").unwrap();
    let root = canonicalize_root(td.path()).unwrap();
    let msgs = vec![user_msg("?")];

    let first = assemble_chat(Some(&root), &msgs, None).expect("ok 1");
    assert!(first.messages[0].content.contains("v1: original rule"));

    fs::write(&agents, "v2: replacement rule\n").unwrap();
    let second = assemble_chat(Some(&root), &msgs, None).expect("ok 2");
    assert!(second.messages[0].content.contains("v2: replacement rule"));
    assert!(!second.messages[0].content.contains("v1: original rule"));
}

#[test]
fn line_range_handles_file_without_trailing_newline() {
    // "a\nb\nc" (no trailing '\n') is a legal 3-line file; the
    // splitter mustn't count the missing '\n' as a 4th line.
    let td = TempDir::new("notrail");
    let root = canonicalize_root(td.path()).unwrap();
    fs::write(td.path().join("notrail.txt"), "a\nb\nc").unwrap();

    let msgs = vec![user_msg("?")];
    // Lines 1..=3 should work; 4 should be rejected.
    assemble_chat(Some(&root), &msgs, Some(range("notrail.txt", 1, 3))).expect("1..=3 ok");
    let err = assemble_chat(Some(&root), &msgs, Some(range("notrail.txt", 1, 4))).unwrap_err();
    assert!(matches!(err, IpcError::BadArgument(_)), "got {err:?}");
}

// ---- D15: propose-diff mode ----

#[test]
fn propose_diff_mode_prepends_system_message_first() {
    // Mode is the response-shape constraint; AGENTS.md is
    // project context. Mode must land FIRST in the transcript
    // so the model sees "respond as a diff" before "and here
    // are the project rules." If a future refactor reorders
    // this, this test catches it.
    let td = TempDir::new("mode-order");
    fs::write(td.path().join("AGENTS.md"), "be careful.\n").unwrap();
    let root = canonicalize_root(td.path()).unwrap();
    let msgs = vec![user_msg("rename foo to bar")];

    let out = assemble(Some(&root), &msgs, None, ChatMode::ProposeDiff).expect("ok");
    // Final transcript: [mode system, instructions system, user]
    assert_eq!(out.messages.len(), 3);
    assert!(matches!(out.messages[0].role, ChatRole::System));
    assert!(out.messages[0].content.contains("propose-diff"));
    assert!(out.messages[0].content.contains("UNIFIED DIFF"));
    assert!(matches!(out.messages[1].role, ChatRole::System));
    assert!(out.messages[1].content.contains("Project instructions"));
    assert!(matches!(out.messages[2].role, ChatRole::User));
    assert_eq!(out.messages[2].content, "rename foo to bar");
}

#[test]
fn propose_diff_mode_prepends_without_agents_md() {
    // No project / no AGENTS.md → still inject the mode system
    // message. The mode pin applies regardless of project state.
    let msgs = vec![user_msg("rename foo to bar")];
    let out = assemble(None, &msgs, None, ChatMode::ProposeDiff).expect("ok");
    assert_eq!(out.messages.len(), 2);
    assert!(matches!(out.messages[0].role, ChatRole::System));
    assert!(out.messages[0].content.contains("propose-diff"));
    assert!(matches!(out.messages[1].role, ChatRole::User));
}

#[test]
fn chat_mode_does_not_inject_system_message() {
    // The D7.1 default behaviour must survive the new
    // `mode` parameter. With `ChatMode::Chat` no extra
    // system message is prepended — the transcript matches
    // what `assemble_chat` produces.
    let msgs = vec![user_msg("hi")];
    let out = assemble(None, &msgs, None, ChatMode::Chat).expect("ok");
    assert_eq!(out.messages.len(), 1);
    assert!(matches!(out.messages[0].role, ChatRole::User));
}

#[test]
fn propose_diff_mode_composes_with_attachment() {
    // All three folding paths active at once: mode pin (D15)
    // first, AGENTS.md (D11) second, attachment wrapped into
    // the last user message (D8). The combined shape is the
    // most complex thing the assembler builds today; pinning
    // it catches off-by-one ordering bugs.
    let td = TempDir::new("mode+instr+attach");
    fs::write(td.path().join("AGENTS.md"), "rules\n").unwrap();
    fs::write(td.path().join("foo.txt"), "hello\n").unwrap();
    let root = canonicalize_root(td.path()).unwrap();
    let msgs = vec![user_msg("change this file")];

    let out = assemble(
        Some(&root),
        &msgs,
        Some(whole_file("foo.txt")),
        ChatMode::ProposeDiff,
    )
    .expect("ok");
    assert_eq!(out.messages.len(), 3);
    assert!(out.messages[0].content.contains("propose-diff"));
    assert!(out.messages[1].content.contains("Project instructions"));
    let last = &out.messages[2].content;
    assert!(last.contains("Attached file"));
    assert!(last.contains("hello"));
    assert!(last.ends_with("change this file"));
    assert!(out.attachment.is_some());
    assert!(out.instructions.is_some());
}

// ---- D12: chat.context preview path ----

#[test]
fn preview_returns_empty_when_no_project_and_no_attachment() {
    // Plain chat without a project open — the D7.1 shape. The
    // preview must answer "nothing would ride along" rather
    // than erroring; the UI uses that to suppress the context
    // area entirely.
    let preview = preview_context(None, None);
    assert!(preview.instructions.is_none());
    assert!(preview.attachment.is_none());
}

#[test]
fn preview_surfaces_agents_md_summary_for_trusted_project() {
    let td = TempDir::new("preview-instr");
    fs::write(
        td.path().join("AGENTS.md"),
        "# Plume rules\n\nBe careful.\n",
    )
    .unwrap();
    let root = canonicalize_root(td.path()).unwrap();

    let preview = preview_context(Some(&root), None);
    let instr = preview.instructions.expect("instructions summary");
    assert_eq!(instr.source, "AGENTS.md");
    assert!(instr.original_bytes > 0);
    assert_eq!(instr.redaction_count, 0);
    assert!(preview.attachment.is_none());
}

#[test]
fn preview_omits_instructions_when_agents_md_absent() {
    let td = TempDir::new("preview-no-instr");
    let root = canonicalize_root(td.path()).unwrap();
    let preview = preview_context(Some(&root), None);
    assert!(preview.instructions.is_none());
    assert!(preview.attachment.is_none());
}

#[test]
fn preview_reports_ready_attachment_with_summary() {
    let td = TempDir::new("preview-ready");
    let root = canonicalize_root(td.path()).unwrap();
    fs::write(td.path().join("hello.txt"), "world\n").unwrap();

    let preview = preview_context(Some(&root), Some(whole_file("hello.txt")));
    let outcome = preview.attachment.expect("attachment outcome");
    match outcome {
        AttachmentPreviewOutcome::Ready(summary) => {
            assert_eq!(summary.rel_path, "hello.txt");
            assert_eq!(summary.line_range, None);
            assert!(summary.original_bytes > 0);
            assert_eq!(summary.redaction_count, 0);
        }
        other => panic!("expected Ready, got {other:?}"),
    }
}

#[test]
fn preview_redaction_count_matches_send_path() {
    // The preview's redaction_count must equal what the actual
    // `assemble` call would surface; the redactor runs over the
    // same redacted string for both paths.
    let td = TempDir::new("preview-redact-eq");
    let root = canonicalize_root(td.path()).unwrap();
    // Deliberate fake — gitleaks:allow on the same line.
    let raw = "OPENAI_API_KEY=sk-1234567890abcdef1234567890abcdef\n"; // gitleaks:allow
    fs::write(td.path().join("secrets.txt"), raw).unwrap();

    let preview = preview_context(Some(&root), Some(whole_file("secrets.txt")));
    let preview_count = match preview.attachment.expect("attachment outcome") {
        AttachmentPreviewOutcome::Ready(s) => s.redaction_count,
        other => panic!("expected Ready, got {other:?}"),
    };
    let assembled = assemble_chat(
        Some(&root),
        &[user_msg("?")],
        Some(whole_file("secrets.txt")),
    )
    .expect("assemble ok");
    let send_count = assembled.attachment.expect("send summary").redaction_count;
    assert_eq!(preview_count, send_count, "preview must match send");
}

#[test]
fn preview_blocks_secret_filename_attachment() {
    // Secret-filename policy must surface in the preview as
    // Blocked, not Ready — and it must NOT raise an Err out of
    // preview_context (in-band reporting is the whole point).
    let td = TempDir::new("preview-block-secret");
    let root = canonicalize_root(td.path()).unwrap();
    fs::write(td.path().join(".env"), "X=1\n").unwrap();

    let preview = preview_context(Some(&root), Some(whole_file(".env")));
    match preview.attachment.expect("attachment outcome") {
        AttachmentPreviewOutcome::Blocked { rel_path, error } => {
            assert_eq!(rel_path, ".env");
            assert!(matches!(error, IpcError::Blocked(_)), "got {error:?}");
        }
        other => panic!("expected Blocked, got {other:?}"),
    }
}

#[test]
fn preview_blocks_path_escape() {
    let td = TempDir::new("preview-block-escape");
    let root = canonicalize_root(td.path()).unwrap();
    let preview = preview_context(Some(&root), Some(whole_file("../oops.txt")));
    match preview.attachment.expect("attachment outcome") {
        AttachmentPreviewOutcome::Blocked { rel_path, error } => {
            assert_eq!(rel_path, "../oops.txt");
            assert!(
                matches!(error, IpcError::PathEscape(_) | IpcError::NotFound(_)),
                "got {error:?}"
            );
        }
        other => panic!("expected Blocked, got {other:?}"),
    }
}

#[test]
fn preview_blocks_attachment_when_no_trusted_project() {
    // No project_root → attachment surfaces as NeedsApproval,
    // mirroring what `chat.send`'s trust gate would reject with.
    let preview = preview_context(None, Some(whole_file("src/main.rs")));
    match preview.attachment.expect("attachment outcome") {
        AttachmentPreviewOutcome::Blocked { rel_path, error } => {
            assert_eq!(rel_path, "src/main.rs");
            assert!(matches!(error, IpcError::NeedsApproval), "got {error:?}");
        }
        other => panic!("expected Blocked, got {other:?}"),
    }
    // And no instructions read happens — there's no trusted
    // project to read AGENTS.md from.
    assert!(preview.instructions.is_none());
}

#[test]
fn preview_blocks_invalid_line_range() {
    // endLine past EOF must surface as Blocked(BadArgument),
    // matching what `chat.send` would reject the actual send
    // with.
    let td = TempDir::new("preview-bad-range");
    let root = canonicalize_root(td.path()).unwrap();
    fs::write(td.path().join("short.txt"), "only\ntwo\n").unwrap();

    let preview = preview_context(Some(&root), Some(range("short.txt", 1, 99)));
    match preview.attachment.expect("attachment outcome") {
        AttachmentPreviewOutcome::Blocked { rel_path, error } => {
            assert_eq!(rel_path, "short.txt");
            match error {
                IpcError::BadArgument(msg) => {
                    assert!(msg.contains("endLine"), "msg was: {msg}");
                }
                other => panic!("expected BadArgument, got {other:?}"),
            }
        }
        other => panic!("expected Blocked, got {other:?}"),
    }
}

#[test]
fn preview_returns_both_instructions_and_attachment_together() {
    // When both AGENTS.md and a ready attachment are present,
    // the preview surfaces both — the UI is the only consumer
    // that wants the combined picture in a single answer.
    let td = TempDir::new("preview-combo");
    let root = canonicalize_root(td.path()).unwrap();
    fs::write(td.path().join("AGENTS.md"), "rule 1: be careful\n").unwrap();
    fs::write(td.path().join("note.txt"), "hello world\n").unwrap();

    let preview = preview_context(Some(&root), Some(whole_file("note.txt")));
    assert!(preview.instructions.is_some(), "instructions must ride");
    match preview.attachment.expect("attachment outcome") {
        AttachmentPreviewOutcome::Ready(summary) => {
            assert_eq!(summary.rel_path, "note.txt");
        }
        other => panic!("expected Ready, got {other:?}"),
    }
}

#[test]
fn preview_reflects_line_range_in_summary() {
    // The slice itself doesn't ride on the wire (the preview's
    // job is to report a summary), but the requested range
    // shows up in the Ready outcome so the UI can render
    // `note.txt:2–3` rather than `note.txt`.
    let td = TempDir::new("preview-range");
    let root = canonicalize_root(td.path()).unwrap();
    fs::write(td.path().join("note.txt"), "a\nb\nc\nd\n").unwrap();

    let preview = preview_context(Some(&root), Some(range("note.txt", 2, 3)));
    match preview.attachment.expect("attachment outcome") {
        AttachmentPreviewOutcome::Ready(summary) => {
            assert_eq!(summary.line_range, Some(LineRange { start: 2, end: 3 }));
        }
        other => panic!("expected Ready, got {other:?}"),
    }
}
