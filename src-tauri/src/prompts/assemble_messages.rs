//! System-message builders for `prompts::assemble`.
//!
//! Pure functions that turn a project-context read (memory entries,
//! curated topic files, AGENTS.md) into the `system`-role `ChatMessage`
//! the assembler prepends. Split out of `assemble.rs` (D74) to keep
//! that file under the decomposition cap; the logic and the preamble
//! wording are unchanged.

use crate::chat::{ChatMessage, ChatRole};
use crate::memory;

/// Build the D42 `system`-role message that carries the project's
/// memory entries. The entries are already redacted on the write
/// path; we treat them as untrusted project data anyway — the
/// preamble explicitly tags them as "user-supplied notes" so the
/// model doesn't elevate them to instructions or commands. Newest
/// first, bullet-separated, content only (no ids, no timestamps).
///
/// Format:
///
/// ```text
/// Project memory (read-only notes the user remembered earlier;
/// N entries, K bytes used of M byte cap[, OLDEST dropped]):
///
/// - entry text 1
/// - entry text 2
/// ```
pub(super) fn make_memory_message(read: &memory::MemoryPromptRead) -> ChatMessage {
    let estimated_size: usize = read
        .entries
        .iter()
        .map(|e| e.text.len() + 4) // "- " prefix + "\n" suffix + safety
        .sum::<usize>()
        + 200;
    let mut text = String::with_capacity(estimated_size);
    text.push_str("Project memory (read-only notes the user remembered earlier; ");
    text.push_str(&read.entries.len().to_string());
    text.push_str(" entries, ");
    text.push_str(&read.used_bytes.to_string());
    text.push_str(" bytes used of ");
    text.push_str(&read.byte_cap.to_string());
    text.push_str(" byte cap");
    if read.truncated {
        text.push_str(", older entries dropped to fit");
    }
    text.push_str("):\n\n");
    for entry in &read.entries {
        text.push_str("- ");
        // Inline newlines in the entry text would break the
        // bullet structure when the model parses this back. Replace
        // them with a space so a multi-line remembered note still
        // reads as one bullet.
        for ch in entry.text.chars() {
            if ch == '\n' {
                text.push(' ');
            } else {
                text.push(ch);
            }
        }
        text.push('\n');
    }
    ChatMessage {
        role: ChatRole::System,
        content: text,
    }
}

/// D72: build the system message folding in the curated core topic
/// files (INDEX/USER/SOUL). Each file is delimited so the model can
/// tell them apart; content is the capped Markdown the user authored.
/// Unlike memory entries, multi-line content is kept verbatim — these
/// are prose documents, not one-line bullets.
pub(super) fn make_topics_message(read: &memory::TopicsPromptRead) -> ChatMessage {
    let estimated: usize = read
        .files
        .iter()
        .map(|f| f.content.len() + f.name.len() + 16)
        .sum::<usize>()
        + 160;
    let mut text = String::with_capacity(estimated);
    text.push_str("Project memory topic files (read-only curated context the user authored; ");
    text.push_str(&read.files.len().to_string());
    text.push_str(if read.files.len() == 1 {
        " file"
    } else {
        " files"
    });
    if read.truncated {
        text.push_str(", trimmed to fit");
    }
    text.push_str("):\n");
    for file in &read.files {
        text.push_str("\n----- ");
        text.push_str(&file.name);
        text.push_str(" -----\n");
        text.push_str(file.content.trim());
        text.push('\n');
    }
    ChatMessage {
        role: ChatRole::System,
        content: text,
    }
}

/// Build the D11 `system`-role message that carries the project's
/// AGENTS.md content. Pulled out so tests can assert on the
/// preamble shape without spinning up a full assemble call.
pub(super) fn make_instructions_message(redacted_content: &str) -> ChatMessage {
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
