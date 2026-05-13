//! Prompt assembly: turn a chat transcript into the final
//! `Vec<ChatMessage>` the model adapter sends.
//!
//! Two folding paths land here:
//!
//! 1. **File attachment (D8 + D10).** At most one attachment per
//!    send, folded into the LAST user message only. Earlier turns
//!    in the transcript flow through unchanged — they're already
//!    history, and re-wrapping them every turn would duplicate the
//!    file content in the context window. With D10, an optional
//!    line range narrows the wrapped slice to a 1-based inclusive
//!    `[start, end]` range.
//!
//! 2. **Project instructions (D11).** When a trusted project has a
//!    root `AGENTS.md`, it's prepended as a `system` message on
//!    every send. Re-read on every call because Ollama's
//!    `/api/chat` is stateless and caching would let a stale
//!    version linger past a user edit. See
//!    `prompts::instructions` for the read + skip-on-error policy.
//!
//! The frontend's visible transcript only stores user/assistant
//! messages; both the system instructions message and the
//! attachment-wrapping live on the wire and are invisible in the
//! UI.
//!
//! Attachment wrapping format:
//!
//! ```text
//! Attached file (read-only context): src/foo.rs
//!
//! ----- FILE BEGIN -----
//! <redacted content>
//! ----- FILE END -----
//!
//! <user's instruction>
//! ```
//!
//! Project-instructions message format:
//!
//! ```text
//! Project instructions (read-only, from AGENTS.md at the project root):
//!
//! <redacted content>
//! ```
//!
//! Models handle these reliably without needing tool-call syntax,
//! and the delimiters don't collide with markdown fences in the
//! file. The same shape will keep working when richer prompt modes
//! (`propose-diff`, `scoped-edit`) land — they'll layer their own
//! system message on top, not replace these.

use std::path::Path;

use crate::chat::{ChatMessage, ChatRole};
use crate::error::IpcError;
use crate::prompts::instructions::{read_project_instructions, INSTRUCTIONS_FILENAME};
use crate::prompts::read::{read_for_prompt, RedactedContent};
use crate::safety::path::ensure_inside;

/// What the chat handler passes in when the user attaches a file
/// from the file inspector. The Tauri command's wire shape
/// (`AttachmentRef`) maps onto this after validating that a project
/// is open and trusted.
#[derive(Debug, Clone)]
pub enum AttachmentRequest {
    /// A file inside the currently-open project root. The path is
    /// already validated to be non-empty and within a length cap by
    /// the handler; resolution and the prompt-read happen here.
    ///
    /// `line_range` is the optional D10 narrowing: when set, the
    /// assembler trims the redacted content to lines
    /// `[start, end]` (1-based, inclusive) before wrapping. Range
    /// shape is validated by the chat handler before calling in —
    /// here we only need to enforce "the requested end line exists
    /// in the file" after the read.
    ProjectFile {
        /// Project-relative form as quoted in the prompt.
        rel_path: String,
        /// Optional 1-based inclusive line range. `None` means
        /// "the whole file", same as D8.
        line_range: Option<LineRange>,
    },
}

/// 1-based inclusive line range. Both ends are kept as `u32` —
/// `usize` would be tempting but 4 billion lines is well past any
/// useful prompt-read scope, and `u32` matches typical editor line
/// counter precision plus serialises cleanly without target
/// platform surprises.
///
/// Invariants enforced upstream (the chat handler's
/// `validate_attachment`): `start >= 1`, `end >= start`. Tests in
/// this module rely on those invariants rather than re-checking,
/// so callers that bypass the handler must establish them
/// themselves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineRange {
    pub start: u32,
    pub end: u32,
}

/// What `assemble` returns to the chat handler.
#[derive(Debug, Clone)]
pub struct AssembledPrompt {
    /// Final messages array. Same length as the input transcript
    /// when no instructions and no attachment apply; one longer
    /// when D11 instructions were prepended; the last user
    /// message is wrapped when a D8/D10 attachment applied.
    pub messages: Vec<ChatMessage>,
    /// Summary of the file attachment, when one was folded in.
    /// Forwarded by the handler in tracing logs.
    pub attachment: Option<AttachmentSummary>,
    /// D11 summary of the project-instructions read, when
    /// `AGENTS.md` was successfully folded in. The chat handler
    /// surfaces an honest "instructionsIncluded" boolean to the
    /// frontend based on whether this is `Some`.
    pub instructions: Option<InstructionsSummary>,
}

/// Diagnostics about a successful project-instructions fold. Same
/// shape philosophy as `AttachmentSummary` — no content
/// fingerprint, nothing that could leak through tracing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstructionsSummary {
    /// Filename relative to the project root. Today this is always
    /// `AGENTS.md`; the field exists so a future "also read
    /// `CLAUDE.md` for backward compat" or per-directory overlay
    /// can populate it without a struct rename.
    pub source: String,
    /// Bytes on disk before the redactor ran.
    pub original_bytes: u64,
    /// Number of secret-pattern matches the redactor masked. The
    /// chat handler logs this so a user pasting a key into
    /// `AGENTS.md` is visible to the audit log.
    pub redaction_count: usize,
}

/// Diagnostics about a successful attachment. The visible chip in
/// the chat panel already knows the path; this is for logs / future
/// telemetry. The summary is intentionally small — no content
/// fingerprint, nothing that could leak through tracing.
///
/// `line_range` echoes the requested (and verified) range when the
/// caller asked for one. `None` means "whole file" so the log can
/// distinguish "user attached the whole file" from "user attached
/// lines 1–N where N happened to be the full file" without a
/// separate flag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttachmentSummary {
    pub rel_path: String,
    pub original_bytes: u64,
    pub redaction_count: usize,
    pub line_range: Option<LineRange>,
}

/// Build the final messages array for an Ollama `/api/chat` call.
///
/// `project_root` is `Some(canonical_root)` when the caller has a
/// trusted open project; `None` for plain chat without a project
/// open. When `Some`, the assembler probes for `AGENTS.md` at
/// the root and prepends a `system` message on every send (D11).
///
/// `attachment` is the optional D8/D10 file-context request.
/// Attachment without a `project_root` is a caller-contract
/// violation — the chat handler is supposed to reject that with
/// `NeedsApproval` before reaching the assembler. If we see it
/// here anyway we surface `IpcError::Internal` rather than
/// panicking, so a release-mode caller bug never crashes the
/// IPC thread.
///
/// On error the chat handler surfaces the typed `IpcError`
/// synchronously, BEFORE registering a stream id, so the frontend
/// sees a `Blocked` / `NotFound` / `PathEscape` reject the same
/// way it would for a malformed text-only send. No `chat.done`
/// event fires in that path. Project-instructions read errors do
/// NOT propagate: a missing / oversize / binary `AGENTS.md`
/// returns a summary of `None` and the chat continues without
/// instructions.
pub fn assemble(
    project_root: Option<&Path>,
    messages: &[ChatMessage],
    attachment: Option<AttachmentRequest>,
) -> Result<AssembledPrompt, IpcError> {
    // Step 1: wrap the attachment into the last user message, if
    // one was provided. The output `attachment_summary` is `None`
    // when there was no attachment to fold. An attachment with no
    // project_root is a caller-contract violation that surfaces as
    // a typed `Internal` rather than a panic.
    let (mut out_messages, attachment_summary) = match (attachment, project_root) {
        (None, _) => (messages.to_vec(), None),
        (Some(req), Some(root)) => apply_attachment(root, messages, req)?,
        (Some(_), None) => {
            return Err(IpcError::Internal(
                "assemble: attachment requires a project_root; the chat handler should have rejected this with NeedsApproval before calling in".into(),
            ));
        }
    };

    // Step 2: probe the project root for `AGENTS.md` and prepend
    // it as a `system` message if available. Re-read on every
    // send so a user edit to `AGENTS.md` between sends is picked
    // up without an extra "reload" verb.
    let instructions_summary =
        project_root
            .and_then(read_project_instructions)
            .and_then(|content| {
                // Defensive — `read_project_instructions` already
                // skips empty / whitespace-only files, but if a
                // future refactor changes that we still want
                // assemble to refuse to inject a useless system
                // message.
                if content.content.trim().is_empty() {
                    return None;
                }
                let system_msg = make_instructions_message(&content.content);
                out_messages.insert(0, system_msg);
                Some(InstructionsSummary {
                    source: INSTRUCTIONS_FILENAME.to_string(),
                    original_bytes: content.original_bytes,
                    redaction_count: content.redactions.len(),
                })
            });

    Ok(AssembledPrompt {
        messages: out_messages,
        attachment: attachment_summary,
        instructions: instructions_summary,
    })
}

/// Take a `Vec<ChatMessage>` and a validated `AttachmentRequest`,
/// return the wrapped messages plus a summary. Extracted from the
/// body of `assemble` so the function reads as two ordered steps
/// (wrap, then prepend) rather than one nested block.
fn apply_attachment(
    root: &Path,
    messages: &[ChatMessage],
    req: AttachmentRequest,
) -> Result<(Vec<ChatMessage>, Option<AttachmentSummary>), IpcError> {
    let AttachmentRequest::ProjectFile {
        rel_path,
        line_range,
    } = req;

    if messages.is_empty() {
        // Defensive — the handler already rejects empty messages
        // before calling us. Surface a typed error if a future
        // refactor moves the check.
        return Err(IpcError::BadArgument(
            "cannot attach a file to an empty transcript".into(),
        ));
    }

    let red = resolve_and_read(root, &rel_path)?;
    let mut out: Vec<ChatMessage> = messages.to_vec();
    let last = out
        .last_mut()
        .expect("non-empty checked above; len() > 0 guarantees last_mut() is Some");
    if !matches!(last.role, ChatRole::User) {
        return Err(IpcError::BadArgument(
            "attachment can only attach to a final user message".into(),
        ));
    }

    // D10: if the caller asked for a line range, slice the redacted
    // content here. Slicing AFTER the redactor matters — secrets on
    // lines outside the range still get redacted from any
    // overlapping fragment, and the range fields can't be used as a
    // boundary to dodge redaction on a line that crosses it (we
    // operate on the redacted string).
    let (sliced_content, applied_range) = match line_range {
        None => (red.content.clone(), None),
        Some(range) => {
            let sliced = slice_lines(&red.content, range).map_err(|reason| {
                IpcError::BadArgument(format!("attachment.relPath '{}': {reason}", red.rel_path))
            })?;
            (sliced, Some(range))
        }
    };

    last.content =
        wrap_with_attachment(&red.rel_path, &sliced_content, applied_range, &last.content);
    let summary = AttachmentSummary {
        rel_path: red.rel_path,
        original_bytes: red.original_bytes,
        redaction_count: red.redactions.len(),
        line_range: applied_range,
    };
    Ok((out, Some(summary)))
}

/// Build the D11 `system`-role message that carries the project's
/// AGENTS.md content. Pulled out so tests can assert on the
/// preamble shape without spinning up a full assemble call.
fn make_instructions_message(redacted_content: &str) -> ChatMessage {
    let mut text = String::with_capacity(redacted_content.len() + 96);
    text.push_str("Project instructions (read-only, from AGENTS.md at the project root):\n\n");
    text.push_str(redacted_content);
    // The redactor preserves the file's trailing newline behavior;
    // we add one if it was missing so the next message (if any
    // future system layer prepends another) doesn't run together.
    if !redacted_content.ends_with('\n') {
        text.push('\n');
    }
    ChatMessage {
        role: ChatRole::System,
        content: text,
    }
}

/// Slice `content` to lines `[range.start, range.end]` (1-based,
/// inclusive). Returns `Err(reason)` when the range's end is past
/// the file's last line — that's a typed `BadArgument` upstream.
///
/// Newline handling: we split on `'\n'` only, so a file with `\r\n`
/// line endings (rare in Plume's target source trees but legal)
/// keeps trailing `\r` characters on each line. That's fine for
/// model context — the model sees what the file has. We don't
/// rewrite line endings.
///
/// We always append a trailing newline to the sliced result so the
/// closing `----- FILE END -----` marker in the wrapper sits on its
/// own line. Without the trailing newline a one-line slice would
/// run into the marker.
fn slice_lines(content: &str, range: LineRange) -> Result<String, String> {
    debug_assert!(range.start >= 1, "start must be 1-based");
    debug_assert!(range.end >= range.start, "end must be >= start");

    // `split('\n')` produces N+1 segments for a string with N
    // newlines; the trailing one is empty when the file ends with
    // '\n'. We count the actual lines as the number of segments
    // that aren't an empty trailing artefact.
    let parts: Vec<&str> = content.split('\n').collect();
    let line_count = if parts.last().is_some_and(|s| s.is_empty()) && parts.len() > 1 {
        parts.len() - 1
    } else {
        parts.len()
    };

    let start = range.start as usize;
    let end = range.end as usize;
    if start > line_count {
        return Err(format!(
            "startLine {start} is past the file's last line ({line_count})"
        ));
    }
    let end_clamped = end.min(line_count);
    if end > line_count {
        // The user (or a buggy frontend) asked for more lines than
        // exist. Reject rather than silently clamp — the frontend
        // claims to know which range it wants, so being honest
        // about the mismatch surfaces the real problem.
        return Err(format!(
            "endLine {end} is past the file's last line ({line_count})"
        ));
    }
    let _ = end_clamped; // explicit no-op to document the no-clamp choice
    let mut out = parts[(start - 1)..end].join("\n");
    if !out.ends_with('\n') {
        out.push('\n');
    }
    Ok(out)
}

fn resolve_and_read(root: &Path, rel_path: &str) -> Result<RedactedContent, IpcError> {
    // The relative-path checks (non-empty, no leading slash, no
    // `..` segments) happen in the chat handler so the error
    // messages reference `attachment.relPath` directly. By the time
    // we get here we trust the shape; `ensure_inside` is still the
    // belt-and-suspenders catch for an absolute path that slipped
    // through, a symlink that escapes, or a path that canonicalizes
    // elsewhere.
    let candidate = root.join(rel_path);
    let canon = ensure_inside(root, &candidate).map_err(IpcError::from)?;
    read_for_prompt(root, &canon, rel_path)
}

/// Compose the wrapped user message. `content` is what gets quoted
/// inside the delimiter block — already redacted, already sliced
/// to the requested range when one was supplied. `applied_range`
/// is echoed in the header line so the model sees "(lines 12–18)"
/// inline and won't hallucinate adjacent lines.
fn wrap_with_attachment(
    rel_path: &str,
    content: &str,
    applied_range: Option<LineRange>,
    user_instruction: &str,
) -> String {
    let mut out = String::with_capacity(content.len() + user_instruction.len() + 200);
    out.push_str("Attached file (read-only context): ");
    out.push_str(rel_path);
    if let Some(range) = applied_range {
        // Format "lines N–M" for a multi-line range, "line N" for
        // a single-line one. Using an en-dash here (instead of a
        // hyphen) keeps the label visually distinct from the
        // path; the model handles either fine.
        if range.start == range.end {
            out.push_str(&format!(" (line {})", range.start));
        } else {
            out.push_str(&format!(" (lines {}\u{2013}{})", range.start, range.end));
        }
    }
    out.push_str("\n\n----- FILE BEGIN -----\n");
    out.push_str(content);
    if !content.ends_with('\n') {
        out.push('\n');
    }
    out.push_str("----- FILE END -----\n\n");
    out.push_str(user_instruction);
    out
}

#[cfg(test)]
mod tests {
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

    #[test]
    fn passes_through_when_no_attachment() {
        let td = TempDir::new("noattach");
        let root = canonicalize_root(td.path()).unwrap();
        let msgs = vec![user_msg("hi"), assistant_msg("hello"), user_msg("again")];
        let out = assemble(Some(&root), &msgs, None).expect("ok");
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
        let out = assemble(Some(&root), &msgs, Some(whole_file("hello.txt"))).expect("ok");

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
        let out = assemble(Some(&root), &msgs, Some(whole_file("secrets.txt"))).expect("ok");
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
        let err = assemble(Some(&root), &msgs, Some(whole_file(".env"))).unwrap_err();
        assert!(matches!(err, IpcError::Blocked(_)), "got {err:?}");
    }

    #[test]
    fn rejects_path_escape_attachment() {
        let td = TempDir::new("escape");
        let root = canonicalize_root(td.path()).unwrap();
        // `../<sibling>` resolves outside the project root.
        let msgs = vec![user_msg("read")];
        let err = assemble(Some(&root), &msgs, Some(whole_file("../oops.txt"))).unwrap_err();
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
        let err = assemble(Some(&root), &msgs, Some(whole_file("a.txt"))).unwrap_err();
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
        let out = assemble(Some(&root), &msgs, Some(whole_file("nl.txt"))).expect("ok");
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
        let out = assemble(Some(&root), &msgs, Some(range("six.txt", 2, 4))).expect("ok");
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
        let out = assemble(Some(&root), &msgs, Some(range("three.txt", 2, 2))).expect("ok");
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
        let out = assemble(Some(&root), &msgs, Some(range("mixed.txt", 2, 2))).expect("ok");
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
        let err = assemble(Some(&root), &msgs, Some(range("two.txt", 1, 99))).unwrap_err();
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
        let err = assemble(Some(&root), &msgs, Some(range("two.txt", 5, 6))).unwrap_err();
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

        let out = assemble(Some(&root), &msgs, None).expect("ok");
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
        let out = assemble(Some(&root), &msgs, None).expect("ok");
        assert_eq!(out.messages.len(), 1);
        assert!(matches!(out.messages[0].role, ChatRole::User));
        assert!(out.instructions.is_none());
    }

    #[test]
    fn no_system_message_when_project_root_is_none() {
        // Plain chat without a trusted project — the D7.1 path —
        // must not try to read AGENTS.md from anywhere.
        let msgs = vec![user_msg("hello")];
        let out = assemble(None, &msgs, None).expect("ok");
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
        let err = assemble(None, &msgs, Some(whole_file("anything.rs")))
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

        let out = assemble(Some(&root), &msgs, Some(whole_file("hello.txt"))).expect("ok");
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

        let out = assemble(Some(&root), &msgs, None).expect("ok");
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

        let first = assemble(Some(&root), &msgs, None).expect("ok 1");
        assert!(first.messages[0].content.contains("v1: original rule"));

        fs::write(&agents, "v2: replacement rule\n").unwrap();
        let second = assemble(Some(&root), &msgs, None).expect("ok 2");
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
        assemble(Some(&root), &msgs, Some(range("notrail.txt", 1, 3))).expect("1..=3 ok");
        let err = assemble(Some(&root), &msgs, Some(range("notrail.txt", 1, 4))).unwrap_err();
        assert!(matches!(err, IpcError::BadArgument(_)), "got {err:?}");
    }
}
