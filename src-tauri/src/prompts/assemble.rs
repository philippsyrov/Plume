//! Prompt assembly: turn a chat transcript into the final
//! `Vec<ChatMessage>` the model adapter sends — and tell the UI,
//! without invoking a model, what that final transcript WOULD pick
//! up (D12 preview).
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
//! The D12 preview (`preview_context`) shares the same helpers as
//! the send path. By construction the two can't drift: every
//! resolve + read + slice-validate step that the actual send runs
//! also runs in the preview, so a chat.context response that says
//! "ready, 1280 bytes, 0 redactions" reflects the same numbers
//! `chat.send` would produce on the next turn. The preview never
//! invokes a model and never registers a stream id.
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
use crate::memory;
use crate::prompts::instructions::{read_project_instructions, INSTRUCTIONS_FILENAME};
use crate::prompts::mode::{propose_diff_system_message, ChatMode};
use crate::prompts::read::{read_for_prompt, RedactedContent};
use crate::safety::path::ensure_inside;

#[path = "assemble_messages.rs"]
mod messages;
use messages::{make_instructions_message, make_memory_message, make_topics_message};

/// D42: byte budget for the project-memory system message folded
/// into each chat send. Deliberately tighter than the on-disk total
/// cap (`memory::MAX_BYTES_TOTAL` = 64 KiB): a chat prompt also has
/// to fit AGENTS.md, an optional file attachment (up to 256 KiB),
/// and the user's instruction. Letting memory consume up to 64 KiB
/// here would crowd out everything else on small-context local
/// models. 4 KiB fits a few dozen short remembered facts and is
/// easy to budget against on a 4k / 8k context window.
///
/// Sized in bytes (not characters) because that's the unit the
/// upstream redactor and the disk cap use. The cap applies to the
/// concatenated text content only; the JSON envelope and the
/// per-entry bullet/newline the assembler adds do not count.
pub const MEMORY_CONTEXT_BYTE_CAP: usize = 4 * 1024;

/// D72: byte budget for the always-loaded core topic files
/// (INDEX/USER/SOUL) folded into each chat send. Separate from
/// `MEMORY_CONTEXT_BYTE_CAP` so the curated trio and the remembered
/// entries have independent budgets and one can't starve the other.
/// 6 KiB fits all three core files at their 2 KiB per-file read cap
/// (`memory::topics::MAX_CORE_FILE_BYTES`) while staying small enough
/// to leave room for AGENTS.md, an attachment, and the user prompt on
/// a 4k / 8k context window.
pub const TOPICS_CONTEXT_BYTE_CAP: usize = 6 * 1024;

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
    /// Final messages array. Length grows by one for each system
    /// message that was prepended (project instructions, project
    /// memory, mode pin) and the last user message is wrapped when
    /// a D8/D10 attachment applied.
    pub messages: Vec<ChatMessage>,
    /// Summary of the file attachment, when one was folded in.
    /// Forwarded by the handler in tracing logs.
    pub attachment: Option<AttachmentSummary>,
    /// D11 summary of the project-instructions read, when
    /// `AGENTS.md` was successfully folded in. The chat handler
    /// surfaces an honest "instructionsIncluded" boolean to the
    /// frontend based on whether this is `Some`.
    pub instructions: Option<InstructionsSummary>,
    /// D42 summary of the project-memory injection. `Some` when at
    /// least one entry was folded into a system message. `None`
    /// when no project is open, the store is empty, or the store
    /// is unreadable (a planted symlink, for instance) — memory
    /// failures DO NOT propagate as errors; chat continues without
    /// memory, same posture as a broken `AGENTS.md`.
    pub memory: Option<MemorySummary>,
    /// D72 summary of the curated topic-file injection (INDEX/USER/
    /// SOUL). `Some` when at least one core file was folded into a
    /// system message; `None` on the same honest skips as `memory`
    /// (no project, none created, unreadable). Failures DO NOT
    /// propagate — chat continues without them.
    pub topics: Option<TopicsSummary>,
}

/// Diagnostics about a successful project-memory fold. The chat
/// handler echoes these on `chat.send`'s response so the panel can
/// render a "Memory · N entries · K bytes" badge that matches what
/// the model actually saw. No entry text, no ids — just the
/// summary numbers. `truncated` is `true` when at least one stored
/// entry was dropped to stay within `byte_cap`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemorySummary {
    pub entry_count: usize,
    pub used_bytes: usize,
    pub byte_cap: usize,
    pub truncated: bool,
}

/// D72 diagnostics about a successful curated topic-file fold. Same
/// shape philosophy as `MemorySummary` — counts only, no content.
/// `file_count` is how many of the core trio were folded in;
/// `truncated` is `true` when a core file was skipped to fit the
/// budget or trimmed at its per-file cap.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TopicsSummary {
    pub file_count: usize,
    pub used_bytes: usize,
    pub byte_cap: usize,
    pub truncated: bool,
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

/// D12 preview: what `assemble` would fold in, without actually
/// building messages or invoking a model. Shape mirrors
/// `AssembledPrompt`'s summary fields — same numbers, no message
/// content. The chat handler uses this to populate `chat.context`
/// for the UI's "what will ride along" preview area.
///
/// Attachment errors surface IN-BAND as
/// `AttachmentPreviewOutcome::Blocked` rather than `Err(...)` so a
/// blocked attachment doesn't hide the AGENTS.md preview alongside
/// it. The actual send path still rejects the same conditions with
/// the typed `IpcError`; the preview is the only consumer that
/// wants both pieces of context in a single answer.
#[derive(Debug)]
pub struct ContextPreview {
    /// Same shape `assemble` returns when AGENTS.md was folded in.
    /// `None` covers every honest skip: no trusted project, no
    /// AGENTS.md, or AGENTS.md present but unreadable (oversize /
    /// binary / hardlink / etc).
    pub instructions: Option<InstructionsSummary>,
    /// Per-attachment preview. `None` when the caller asked for no
    /// attachment in the request. Present when an attachment was
    /// requested, regardless of whether it would succeed.
    pub attachment: Option<AttachmentPreviewOutcome>,
    /// D42 preview of the project-memory injection. Same shape and
    /// same semantics as `AssembledPrompt::memory`. `None` covers
    /// every honest skip: no trusted project, empty store, or store
    /// unreadable.
    pub memory: Option<MemorySummary>,
    /// D72 preview of the curated topic-file injection. Same shape and
    /// semantics as `AssembledPrompt::topics`.
    pub topics: Option<TopicsSummary>,
}

/// Per-attachment preview: either "would ride along, here's the
/// summary you'd see logged" or "would be rejected, here's the
/// typed reason the actual send would surface." The `IpcError` is
/// the same value `chat.send` would have produced; the chat handler
/// is responsible for mapping it onto the wire's `blockReason` enum.
#[derive(Debug)]
pub enum AttachmentPreviewOutcome {
    /// Attachment would succeed; the wrapped content + summary on
    /// the next `chat.send` would carry these numbers.
    Ready(AttachmentSummary),
    /// Attachment would reject. `rel_path` is the request's path
    /// (canonicalised to the project-relative form when the read
    /// got that far; otherwise echoed verbatim from the request).
    Blocked { rel_path: String, error: IpcError },
}

/// Read-only preview matching what `assemble` would fold into the
/// next `chat.send`. Same gates run as the real path — secret
/// filenames, oversized files, binary content, path escapes,
/// hardlink aliases, `..` traversal, and line-range validation.
///
/// `project_root` is `Some(canonical_root)` for a trusted open
/// project, `None` otherwise. When `None`:
///   * AGENTS.md is not read (the path isn't trusted).
///   * If an attachment was requested, it surfaces as `Blocked`
///     with `IpcError::NeedsApproval` — matching what `chat.send`
///     would reject with at its trust gate.
///
/// The function NEVER returns `Err`. Every failure mode for an
/// attachment surfaces inside `AttachmentPreviewOutcome::Blocked`,
/// every failure mode for instructions surfaces as `None`. The
/// preview is, by design, a question the UI can ask and always get
/// an answer to.
pub fn preview_context(
    project_root: Option<&Path>,
    attachment: Option<AttachmentRequest>,
) -> ContextPreview {
    // Step 1: probe AGENTS.md the same way `assemble` does. The
    // assembler's defensive "trim().is_empty()" check is mirrored
    // here so the preview never claims an empty instructions file
    // would ride along.
    let instructions = project_root
        .and_then(read_project_instructions)
        .and_then(|content| {
            if content.content.trim().is_empty() {
                return None;
            }
            Some(InstructionsSummary {
                source: INSTRUCTIONS_FILENAME.to_string(),
                original_bytes: content.original_bytes,
                redaction_count: content.redactions.len(),
            })
        });

    // Step 2: per-attachment preview. Three branches mirror the
    // chat handler's structure: no attachment → no preview;
    // attachment + trusted project → run the read and validate;
    // attachment without a trusted project → surface NeedsApproval
    // in-band.
    let attachment_outcome = match (attachment, project_root) {
        (None, _) => None,
        (Some(req), Some(root)) => Some(preview_attachment(root, req)),
        (Some(req), None) => {
            let AttachmentRequest::ProjectFile { rel_path, .. } = req;
            Some(AttachmentPreviewOutcome::Blocked {
                rel_path,
                error: IpcError::NeedsApproval,
            })
        }
    };

    // Step 3 (D42): project-memory preview. Same posture as
    // instructions — failures (planted `.plume` symlink, unreadable
    // store) silently surface as `None` so the preview never
    // refuses to answer.
    let memory = project_root.and_then(read_memory_summary);

    // Step 4 (D72): curated topic-file preview. Same posture as memory.
    let topics = project_root.and_then(read_topics_summary);

    ContextPreview {
        instructions,
        attachment: attachment_outcome,
        memory,
        topics,
    }
}

/// Probe `<project>/.plume/memory/entries.jsonl` for chat-context
/// fold-in metadata. Returns `None` when the store doesn't exist,
/// has no entries, or read failed (planted symlink, etc). The
/// `MemoryPromptRead.entries` field is intentionally dropped here —
/// the preview only needs the counts. The full read happens again
/// inside `assemble` when the send actually fires, so a remember
/// that lands between preview and send is reflected in the real
/// transcript even though the preview's numbers are stale.
fn read_memory_summary(root: &Path) -> Option<MemorySummary> {
    let read = memory::read_for_prompt(root, MEMORY_CONTEXT_BYTE_CAP).ok()?;
    if read.entries.is_empty() {
        return None;
    }
    Some(MemorySummary {
        entry_count: read.entries.len(),
        used_bytes: read.used_bytes,
        byte_cap: read.byte_cap,
        truncated: read.truncated,
    })
}

/// D72: probe the curated core topic files for chat-context fold-in
/// metadata. `None` when no core file exists / is non-empty, or the
/// read failed (planted symlink). Mirrors `read_memory_summary`; the
/// real fold happens again inside `assemble` at send time.
fn read_topics_summary(root: &Path) -> Option<TopicsSummary> {
    let read = memory::read_core_for_prompt(root, TOPICS_CONTEXT_BYTE_CAP).ok()?;
    if read.files.is_empty() {
        return None;
    }
    Some(TopicsSummary {
        file_count: read.files.len(),
        used_bytes: read.used_bytes,
        byte_cap: read.byte_cap,
        truncated: read.truncated,
    })
}

/// Run the same resolve + read + line-range validation an actual
/// send would perform, but stop short of wrapping the content into
/// a user message. Errors are captured as `Blocked` outcomes rather
/// than propagating; the caller (chat handler) maps them onto the
/// wire's `blockReason` enum.
fn preview_attachment(root: &Path, req: AttachmentRequest) -> AttachmentPreviewOutcome {
    let AttachmentRequest::ProjectFile {
        rel_path,
        line_range,
    } = req;
    let rel_path_for_err = rel_path.clone();

    let red = match resolve_and_read(root, &rel_path) {
        Ok(r) => r,
        Err(err) => {
            return AttachmentPreviewOutcome::Blocked {
                rel_path: rel_path_for_err,
                error: err,
            };
        }
    };

    // Run `slice_lines` purely for validation. The wasted slice is
    // cheap (it splits the already-redacted string in memory) and
    // it keeps the validation rule in one place — if a future
    // change tightens what counts as a valid range, the preview
    // picks it up automatically.
    if let Some(range) = line_range {
        if let Err(reason) = slice_lines(&red.content, range) {
            return AttachmentPreviewOutcome::Blocked {
                rel_path: red.rel_path,
                error: IpcError::BadArgument(format!(
                    "attachment.relPath '{}': {reason}",
                    rel_path_for_err
                )),
            };
        }
    }

    AttachmentPreviewOutcome::Ready(AttachmentSummary {
        rel_path: red.rel_path,
        original_bytes: red.original_bytes,
        redaction_count: red.redactions.len(),
        line_range,
    })
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
    mode: ChatMode,
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

    // Step 2 (D42): probe the project's memory store and prepend a
    // bounded system message. Memory is inserted BEFORE instructions
    // in the code path so the subsequent `insert(0, instructions)`
    // lands AGENTS.md ahead of memory in the final transcript. The
    // model sees:
    //   [system] mode pin (step 4, if set)
    //   [system] project instructions (step 3)
    //   [system] project memory (this step)
    //   [user/assistant turns...]
    // Order rationale: AGENTS.md is durable project contract;
    // memory is incremental running notes. Reading instructions
    // first matches how a human onboarding would expect to read
    // them. Failures silently skip — chat continues, the response
    // reports `memory: None` and the user can inspect the store.
    let memory_summary = project_root.and_then(|root| {
        let read = memory::read_for_prompt(root, MEMORY_CONTEXT_BYTE_CAP).ok()?;
        if read.entries.is_empty() {
            return None;
        }
        let system_msg = make_memory_message(&read);
        out_messages.insert(0, system_msg);
        Some(MemorySummary {
            entry_count: read.entries.len(),
            used_bytes: read.used_bytes,
            byte_cap: read.byte_cap,
            truncated: read.truncated,
        })
    });

    // Step 2.5 (D72): fold the always-loaded curated topic files
    // (INDEX/USER/SOUL) into a system message. Inserted at index 0
    // AFTER the memory step so it lands ABOVE memory entries in the
    // transcript: the durable curated identity/prefs/index reads before
    // the incremental running notes. The later `insert(0, instructions)`
    // and mode pin keep AGENTS.md and the mode contract above it. Final
    // order: mode, AGENTS.md, topic files, memory entries, turns.
    // Honest skip on any failure (no files, planted symlink), same as
    // memory.
    let topics_summary = project_root.and_then(|root| {
        let read = memory::read_core_for_prompt(root, TOPICS_CONTEXT_BYTE_CAP).ok()?;
        if read.files.is_empty() {
            return None;
        }
        let system_msg = make_topics_message(&read);
        out_messages.insert(0, system_msg);
        Some(TopicsSummary {
            file_count: read.files.len(),
            used_bytes: read.used_bytes,
            byte_cap: read.byte_cap,
            truncated: read.truncated,
        })
    });

    // Step 3: probe the project root for `AGENTS.md` and prepend
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

    // Step 4 (D15): prepend the propose-diff system message when
    // the caller asked for that mode. Inserted AFTER the AGENTS.md
    // prepend (above) but using `insert(0, ...)` so it lands FIRST
    // in the final transcript — the model sees:
    //   [system] Mode pin (this step)
    //   [system] Project instructions (step 2)
    //   [user/assistant turns...]
    //   [user wrapped attachment if any]
    // Mode first is intentional: the response-shape constraint
    // applies even when the user's prompt looks like a question
    // that AGENTS.md says to answer in prose. AGENTS.md is project
    // context; mode is output contract.
    if matches!(mode, ChatMode::ProposeDiff) {
        out_messages.insert(0, propose_diff_system_message());
    }

    Ok(AssembledPrompt {
        messages: out_messages,
        attachment: attachment_summary,
        instructions: instructions_summary,
        memory: memory_summary,
        topics: topics_summary,
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
#[path = "assemble_tests.rs"]
mod tests;
