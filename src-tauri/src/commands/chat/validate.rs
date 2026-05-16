//! `chat.send` / `chat.context` payload-shape validation.
//!
//! Pure functions over the wire payload types defined in the
//! orchestrator (`super::ChatSendPayload`, `super::AttachmentPayload`).
//! No `AppState`, no filesystem, no network — every reject here is
//! about a malformed request, not a runtime condition.

use crate::chat::ChatRole;
use crate::error::IpcError;

use super::send::ChatSendPayload;
use super::{AttachmentPayload, MAX_ATTACHMENT_REL_PATH_LEN, MAX_STREAM_ID_LEN};

/// Reject obviously malformed payloads with `BadArgument` before
/// any network call. Each branch is its own clause so the error
/// string names the failing field.
pub(super) fn validate_payload(payload: &ChatSendPayload) -> Result<(), IpcError> {
    if payload.stream_id.trim().is_empty() {
        return Err(IpcError::BadArgument(
            "chat.send: streamId is empty".to_string(),
        ));
    }
    if payload.stream_id.len() > MAX_STREAM_ID_LEN {
        return Err(IpcError::BadArgument(format!(
            "chat.send: streamId exceeds {MAX_STREAM_ID_LEN} chars"
        )));
    }
    if payload.provider_id.trim().is_empty() {
        return Err(IpcError::BadArgument(
            "chat.send: providerId is empty".to_string(),
        ));
    }
    if payload.model_id.trim().is_empty() {
        return Err(IpcError::BadArgument(
            "chat.send: modelId is empty — pick a model in the provider panel first".to_string(),
        ));
    }
    if payload.messages.is_empty() {
        return Err(IpcError::BadArgument(
            "chat.send: messages array is empty".to_string(),
        ));
    }
    for (i, m) in payload.messages.iter().enumerate() {
        if m.content.is_empty() {
            return Err(IpcError::BadArgument(format!(
                "chat.send: messages[{i}] has empty content"
            )));
        }
        if matches!(m.role, ChatRole::Tool) {
            return Err(IpcError::BadArgument(format!(
                "chat.send: messages[{i}] uses the 'tool' role, which is not supported yet"
            )));
        }
    }
    let last = payload.messages.last().expect("non-empty checked above");
    if !matches!(last.role, ChatRole::User) {
        return Err(IpcError::BadArgument(
            "chat.send: last message must have role='user'".to_string(),
        ));
    }
    if let Some(att) = payload.attachment.as_ref() {
        validate_attachment(att)?;
    }
    Ok(())
}

/// Reject obviously bad attachment payloads before the handler
/// reaches for the project session. The full path-safety check
/// (canonicalize-then-ensure-inside) runs later in `assemble`; this
/// catches shapes that would never be a legitimate relative path.
pub(super) fn validate_attachment(att: &AttachmentPayload) -> Result<(), IpcError> {
    match att {
        AttachmentPayload::ProjectFile {
            rel_path,
            start_line,
            end_line,
        } => {
            let trimmed = rel_path.trim();
            if trimmed.is_empty() {
                return Err(IpcError::BadArgument(
                    "chat.send: attachment.relPath is empty".into(),
                ));
            }
            if rel_path.len() > MAX_ATTACHMENT_REL_PATH_LEN {
                return Err(IpcError::BadArgument(format!(
                    "chat.send: attachment.relPath exceeds {MAX_ATTACHMENT_REL_PATH_LEN} chars"
                )));
            }
            // Absolute paths and bare `..` traversal are never legal
            // for a project-relative attachment. `assemble`'s
            // canonicalize-then-ensure-inside would catch escapes
            // too, but rejecting up front gives a clearer error
            // message and avoids reaching for the filesystem at all.
            if rel_path.starts_with('/') || rel_path.starts_with('\\') {
                return Err(IpcError::BadArgument(
                    "chat.send: attachment.relPath must be project-relative, not absolute".into(),
                ));
            }
            for segment in rel_path.split(['/', '\\']) {
                if segment == ".." {
                    return Err(IpcError::BadArgument(
                        "chat.send: attachment.relPath must not contain '..' segments".into(),
                    ));
                }
            }
            // NUL bytes in a path string are a hard reject — they'd
            // either fail filesystem syscalls or be silently
            // truncated on some platforms.
            if rel_path.contains('\0') {
                return Err(IpcError::BadArgument(
                    "chat.send: attachment.relPath contains NUL byte".into(),
                ));
            }
            // D10: line range is all-or-nothing. Half a range
            // (just startLine, or just endLine) is almost certainly
            // a frontend bug; reject so the caller fixes the
            // payload instead of silently treating it as
            // whole-file.
            validate_line_range(*start_line, *end_line)?;
        }
    }
    Ok(())
}

fn validate_line_range(start: Option<u32>, end: Option<u32>) -> Result<(), IpcError> {
    match (start, end) {
        (None, None) => Ok(()),
        (Some(_), None) => Err(IpcError::BadArgument(
            "chat.send: attachment.startLine set without endLine".into(),
        )),
        (None, Some(_)) => Err(IpcError::BadArgument(
            "chat.send: attachment.endLine set without startLine".into(),
        )),
        (Some(s), Some(e)) => {
            if s == 0 {
                return Err(IpcError::BadArgument(
                    "chat.send: attachment.startLine must be >= 1 (lines are 1-based)".into(),
                ));
            }
            if e < s {
                return Err(IpcError::BadArgument(format!(
                    "chat.send: attachment.endLine ({e}) must be >= startLine ({s})"
                )));
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::AttachmentPayload;
    use super::*;
    use crate::chat::{ChatMessage, ChatRole};
    use crate::prompts::ChatMode;

    fn user_msg(content: &str) -> ChatMessage {
        ChatMessage {
            role: ChatRole::User,
            content: content.to_string(),
        }
    }

    fn assistant_msg(content: &str) -> ChatMessage {
        ChatMessage {
            role: ChatRole::Assistant,
            content: content.to_string(),
        }
    }

    fn ok_payload(messages: Vec<ChatMessage>) -> ChatSendPayload {
        ChatSendPayload {
            stream_id: "stream-test-0001".into(),
            provider_id: "ollama".into(),
            model_id: "llama3".into(),
            messages,
            handle_id: None,
            attachment: None,
            mode: ChatMode::Chat,
        }
    }

    fn payload_with_attachment(
        messages: Vec<ChatMessage>,
        attachment: AttachmentPayload,
    ) -> ChatSendPayload {
        ChatSendPayload {
            stream_id: "stream-test-attach".into(),
            provider_id: "ollama".into(),
            model_id: "llama3".into(),
            messages,
            handle_id: None,
            attachment: Some(attachment),
            mode: ChatMode::Chat,
        }
    }

    fn project_file_attachment(
        rel_path: &str,
        start_line: Option<u32>,
        end_line: Option<u32>,
    ) -> AttachmentPayload {
        AttachmentPayload::ProjectFile {
            rel_path: rel_path.into(),
            start_line,
            end_line,
        }
    }

    #[test]
    fn rejects_empty_stream_id() {
        let mut p = ok_payload(vec![user_msg("hi")]);
        p.stream_id = "   ".into();
        let err = validate_payload(&p).expect_err("blank stream id rejected");
        match err {
            IpcError::BadArgument(s) => assert!(s.contains("streamId")),
            other => panic!("expected BadArgument, got {other:?}"),
        }
    }

    #[test]
    fn rejects_overlong_stream_id() {
        let mut p = ok_payload(vec![user_msg("hi")]);
        p.stream_id = "x".repeat(MAX_STREAM_ID_LEN + 1);
        let err = validate_payload(&p).expect_err("overlong stream id rejected");
        match err {
            IpcError::BadArgument(s) => assert!(s.contains("streamId")),
            other => panic!("expected BadArgument, got {other:?}"),
        }
    }

    #[test]
    fn rejects_empty_model_id() {
        let mut p = ok_payload(vec![user_msg("hi")]);
        p.model_id = "   ".into();
        let err = validate_payload(&p).expect_err("blank model rejected");
        match err {
            IpcError::BadArgument(s) => assert!(s.contains("modelId")),
            other => panic!("expected BadArgument, got {other:?}"),
        }
    }

    #[test]
    fn rejects_empty_messages() {
        let p = ok_payload(vec![]);
        let err = validate_payload(&p).expect_err("empty messages rejected");
        match err {
            IpcError::BadArgument(s) => assert!(s.contains("messages")),
            other => panic!("expected BadArgument, got {other:?}"),
        }
    }

    #[test]
    fn rejects_tool_role_in_v1() {
        let p = ok_payload(vec![ChatMessage {
            role: ChatRole::Tool,
            content: "tool result".into(),
        }]);
        let err = validate_payload(&p).expect_err("tool rejected");
        match err {
            IpcError::BadArgument(s) => assert!(s.contains("tool")),
            other => panic!("expected BadArgument, got {other:?}"),
        }
    }

    #[test]
    fn rejects_when_last_message_is_assistant() {
        let p = ok_payload(vec![user_msg("hi"), assistant_msg("hey")]);
        let err = validate_payload(&p).expect_err("trailing assistant rejected");
        match err {
            IpcError::BadArgument(s) => assert!(s.contains("user")),
            other => panic!("expected BadArgument, got {other:?}"),
        }
    }

    #[test]
    fn accepts_well_formed_history() {
        let p = ok_payload(vec![user_msg("hi"), assistant_msg("hey"), user_msg("more")]);
        validate_payload(&p).expect("should pass");
    }

    #[test]
    fn rejects_empty_content() {
        let p = ok_payload(vec![user_msg("")]);
        let err = validate_payload(&p).expect_err("empty content rejected");
        match err {
            IpcError::BadArgument(s) => assert!(s.contains("content")),
            other => panic!("expected BadArgument, got {other:?}"),
        }
    }

    // ---- D8 attachment validation ----

    #[test]
    fn accepts_payload_without_attachment() {
        // Sanity: the new field is optional and the D7.1 shape still
        // passes validation untouched.
        let p = ok_payload(vec![user_msg("hi")]);
        validate_payload(&p).expect("D7.1 payload must still validate");
    }

    #[test]
    fn accepts_well_formed_project_file_attachment() {
        let p = payload_with_attachment(
            vec![user_msg("explain this file")],
            project_file_attachment("src/main.rs", None, None),
        );
        validate_payload(&p).expect("normal attachment must validate");
    }

    #[test]
    fn rejects_empty_attachment_rel_path() {
        let p = payload_with_attachment(
            vec![user_msg("hi")],
            project_file_attachment("   ", None, None),
        );
        let err = validate_payload(&p).expect_err("blank relPath rejected");
        match err {
            IpcError::BadArgument(s) => assert!(s.contains("relPath")),
            other => panic!("expected BadArgument, got {other:?}"),
        }
    }

    #[test]
    fn rejects_overlong_attachment_rel_path() {
        let p = payload_with_attachment(
            vec![user_msg("hi")],
            project_file_attachment(&"a".repeat(MAX_ATTACHMENT_REL_PATH_LEN + 1), None, None),
        );
        let err = validate_payload(&p).expect_err("overlong relPath rejected");
        match err {
            IpcError::BadArgument(s) => assert!(s.contains("relPath")),
            other => panic!("expected BadArgument, got {other:?}"),
        }
    }

    #[test]
    fn rejects_absolute_attachment_rel_path() {
        let p = payload_with_attachment(
            vec![user_msg("hi")],
            project_file_attachment("/etc/passwd", None, None),
        );
        let err = validate_payload(&p).expect_err("absolute path rejected");
        match err {
            IpcError::BadArgument(s) => assert!(s.contains("project-relative")),
            other => panic!("expected BadArgument, got {other:?}"),
        }
    }

    #[test]
    fn rejects_dotdot_traversal_in_attachment_rel_path() {
        // Even with a junk parent the `..` segment is a hard reject.
        let p = payload_with_attachment(
            vec![user_msg("hi")],
            project_file_attachment("src/../../etc/passwd", None, None),
        );
        let err = validate_payload(&p).expect_err("`..` segment rejected");
        match err {
            IpcError::BadArgument(s) => assert!(s.contains("'..'")),
            other => panic!("expected BadArgument, got {other:?}"),
        }
    }

    // ---- D10 line-range payload validation ----

    #[test]
    fn accepts_well_formed_line_range_attachment() {
        let p = payload_with_attachment(
            vec![user_msg("look at lines 12-18")],
            project_file_attachment("src/main.rs", Some(12), Some(18)),
        );
        validate_payload(&p).expect("normal line range must validate");
    }

    #[test]
    fn rejects_partial_line_range_start_only() {
        // A startLine without endLine is almost certainly a
        // frontend bug; reject so the caller has to be explicit.
        let p = payload_with_attachment(
            vec![user_msg("?")],
            project_file_attachment("src/main.rs", Some(10), None),
        );
        let err = validate_payload(&p).expect_err("partial range rejected");
        match err {
            IpcError::BadArgument(s) => assert!(s.contains("endLine"), "msg was: {s}"),
            other => panic!("expected BadArgument, got {other:?}"),
        }
    }

    #[test]
    fn rejects_partial_line_range_end_only() {
        let p = payload_with_attachment(
            vec![user_msg("?")],
            project_file_attachment("src/main.rs", None, Some(10)),
        );
        let err = validate_payload(&p).expect_err("partial range rejected");
        match err {
            IpcError::BadArgument(s) => assert!(s.contains("startLine"), "msg was: {s}"),
            other => panic!("expected BadArgument, got {other:?}"),
        }
    }

    #[test]
    fn rejects_zero_start_line() {
        // Lines are 1-based on every code surface (editor gutter,
        // grep, the model's own conventions). `0` is wrong.
        let p = payload_with_attachment(
            vec![user_msg("?")],
            project_file_attachment("src/main.rs", Some(0), Some(10)),
        );
        let err = validate_payload(&p).expect_err("zero startLine rejected");
        match err {
            IpcError::BadArgument(s) => assert!(s.contains("1-based"), "msg was: {s}"),
            other => panic!("expected BadArgument, got {other:?}"),
        }
    }

    #[test]
    fn rejects_end_line_before_start_line() {
        let p = payload_with_attachment(
            vec![user_msg("?")],
            project_file_attachment("src/main.rs", Some(20), Some(10)),
        );
        let err = validate_payload(&p).expect_err("inverted range rejected");
        match err {
            IpcError::BadArgument(s) => assert!(s.contains("endLine"), "msg was: {s}"),
            other => panic!("expected BadArgument, got {other:?}"),
        }
    }

    #[test]
    fn accepts_single_line_range_where_start_equals_end() {
        // start == end is a one-line range — common when the user
        // clicks on a single line and hits Attach.
        let p = payload_with_attachment(
            vec![user_msg("focus")],
            project_file_attachment("src/main.rs", Some(42), Some(42)),
        );
        validate_payload(&p).expect("single-line range must validate");
    }

    #[test]
    fn rejects_nul_byte_in_attachment_rel_path() {
        let p = payload_with_attachment(
            vec![user_msg("hi")],
            project_file_attachment("src/main\0.rs", None, None),
        );
        let err = validate_payload(&p).expect_err("NUL in relPath rejected");
        match err {
            IpcError::BadArgument(s) => assert!(s.contains("NUL")),
            other => panic!("expected BadArgument, got {other:?}"),
        }
    }
}
