//! Prompt assembly: fold an optional read-only file attachment into
//! a chat transcript before it reaches the model adapter.
//!
//! D8 scope: at most one attachment per send, and the attachment is
//! folded into the LAST user message only. Earlier turns in the
//! transcript flow through unchanged — they're already history, and
//! re-wrapping them every turn would duplicate the file content in
//! the context window for no benefit. The frontend's visible
//! transcript stores the user's bare instruction; the wrapping
//! lives on the wire and is invisible in the UI.
//!
//! The wrapping format is deliberately simple — a labeled delimiter
//! block followed by the user's instruction:
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
//! Models handle this reliably without needing tool-call syntax,
//! and the delimiters don't collide with markdown fences in the
//! file. The same shape will keep working when richer prompt modes
//! (`propose-diff`, `scoped-edit`) land — they'll layer their own
//! system message on top, not replace this one.

use std::path::Path;

use crate::chat::{ChatMessage, ChatRole};
use crate::error::IpcError;
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
    ProjectFile {
        /// Project-relative form as quoted in the prompt.
        rel_path: String,
    },
}

/// What `assemble` returns to the chat handler.
#[derive(Debug, Clone)]
pub struct AssembledPrompt {
    /// Final messages array. Same length as the input transcript;
    /// only the last user message has potentially been wrapped.
    pub messages: Vec<ChatMessage>,
    /// Summary of what was attached. `None` when the caller passed
    /// no attachment. Forwarded by the handler in tracing logs and
    /// can later ride in the terminal `chat.done` event for the UI.
    pub attachment: Option<AttachmentSummary>,
}

/// Diagnostics about a successful attachment. The visible chip in
/// the chat panel already knows the path; this is for logs / future
/// telemetry. The summary is intentionally small — no content
/// fingerprint, nothing that could leak through tracing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttachmentSummary {
    pub rel_path: String,
    pub original_bytes: u64,
    pub redaction_count: usize,
}

/// Build the final messages array for an Ollama `/api/chat` call.
///
/// `root` is the canonical project root (caller already validated
/// the project is open and trusted). When `attachment` is `None`
/// the messages array is returned untouched — D7.1 behavior is
/// preserved exactly.
///
/// On error the chat handler surfaces the typed `IpcError`
/// synchronously, BEFORE registering a stream id, so the frontend
/// sees a `Blocked` / `NotFound` / `PathEscape` reject the same
/// way it would for a malformed text-only send. No `chat.done`
/// event fires in that path.
pub fn assemble(
    root: &Path,
    messages: &[ChatMessage],
    attachment: Option<AttachmentRequest>,
) -> Result<AssembledPrompt, IpcError> {
    let Some(req) = attachment else {
        return Ok(AssembledPrompt {
            messages: messages.to_vec(),
            attachment: None,
        });
    };
    let AttachmentRequest::ProjectFile { rel_path } = req;

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
    last.content = wrap_with_attachment(&red, &last.content);
    let summary = AttachmentSummary {
        rel_path: red.rel_path,
        original_bytes: red.original_bytes,
        redaction_count: red.redactions.len(),
    };
    Ok(AssembledPrompt {
        messages: out,
        attachment: Some(summary),
    })
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

fn wrap_with_attachment(red: &RedactedContent, user_instruction: &str) -> String {
    // Reserve a small amount of headroom for the boilerplate; the
    // file dominates the size.
    let mut out = String::with_capacity(red.content.len() + user_instruction.len() + 200);
    out.push_str("Attached file (read-only context): ");
    out.push_str(&red.rel_path);
    out.push_str("\n\n----- FILE BEGIN -----\n");
    out.push_str(&red.content);
    if !red.content.ends_with('\n') {
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
        let out = assemble(&root, &msgs, None).expect("ok");
        assert!(out.attachment.is_none());
        assert_eq!(out.messages.len(), msgs.len());
        assert_eq!(out.messages[2].content, "again");
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
        let out = assemble(
            &root,
            &msgs,
            Some(AttachmentRequest::ProjectFile {
                rel_path: "hello.txt".into(),
            }),
        )
        .expect("ok");

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
        assert_eq!(
            out.attachment.as_ref().expect("attached").rel_path,
            "hello.txt"
        );
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
        let out = assemble(
            &root,
            &msgs,
            Some(AttachmentRequest::ProjectFile {
                rel_path: "secrets.txt".into(),
            }),
        )
        .expect("ok");
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
        let err = assemble(
            &root,
            &msgs,
            Some(AttachmentRequest::ProjectFile {
                rel_path: ".env".into(),
            }),
        )
        .unwrap_err();
        assert!(matches!(err, IpcError::Blocked(_)), "got {err:?}");
    }

    #[test]
    fn rejects_path_escape_attachment() {
        let td = TempDir::new("escape");
        let root = canonicalize_root(td.path()).unwrap();
        // `../<sibling>` resolves outside the project root.
        let msgs = vec![user_msg("read")];
        let err = assemble(
            &root,
            &msgs,
            Some(AttachmentRequest::ProjectFile {
                rel_path: "../oops.txt".into(),
            }),
        )
        .unwrap_err();
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
        let err = assemble(
            &root,
            &msgs,
            Some(AttachmentRequest::ProjectFile {
                rel_path: "a.txt".into(),
            }),
        )
        .unwrap_err();
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
        let out = assemble(
            &root,
            &msgs,
            Some(AttachmentRequest::ProjectFile {
                rel_path: "nl.txt".into(),
            }),
        )
        .expect("ok");
        let last = &out.messages[0].content;
        assert!(last.contains("no newline\n----- FILE END -----"));
    }
}
