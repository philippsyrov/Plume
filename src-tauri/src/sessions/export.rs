//! Render a conversation as Markdown the user can keep.
//!
//! Export is the leg that makes the storage cap survivable. A store at its cap
//! refuses new saves, and the honest answer to "then what?" is *take your
//! history somewhere else and delete what you no longer need* — which requires
//! being able to take it. Deleting a conversation is otherwise the only way to
//! reclaim space, and that is a bad trade to force without an exit.
//!
//! This module is pure. It turns a loaded record into a string; writing that
//! string to disk goes through the existing atomic export port, which owns
//! overwrite consent and path refusal.

use super::{EntryRole, SessionRecord, TranscriptEntry};

/// Markdown for one conversation, including the entries Plume would rather the
/// user did not lose track of: cancellations and errors appear as themselves
/// rather than being silently dropped, because an export that quietly omits
/// them misrepresents what happened in the conversation.
pub fn to_markdown(record: &SessionRecord) -> String {
    let mut out = String::new();
    out.push_str(&format!("# {}\n\n", escape_heading(&record.title)));

    for entry in &record.entries {
        match entry {
            TranscriptEntry::Message {
                message,
                model_used,
                ..
            } => {
                let who = match message.role {
                    EntryRole::User => "You".to_string(),
                    EntryRole::Assistant => match model_used {
                        Some(model) => format!("Plume ({model})"),
                        None => "Plume".to_string(),
                    },
                };
                out.push_str(&format!("## {who}\n\n{}\n\n", message.content.trim_end()));
            }
            TranscriptEntry::Cancelled {
                partial,
                model_used,
                ..
            } => {
                // The partial answer is real content the user saw on screen.
                // Exporting only "stopped" would lose it and make the export
                // disagree with the transcript it came from.
                let who = match model_used {
                    Some(model) => format!("Plume ({model})"),
                    None => "Plume".to_string(),
                };
                out.push_str(&format!("## {who}\n\n"));
                let trimmed = partial.trim_end();
                if !trimmed.is_empty() {
                    out.push_str(&format!("{trimmed}\n\n"));
                }
                out.push_str("_Stopped by you._\n\n");
            }
            TranscriptEntry::Error { message, .. } => {
                out.push_str(&format!("## Plume\n\n_Failed: {message}_\n\n"));
            }
            TranscriptEntry::ResearchArtifact { .. } => {
                out.push_str("## Plume\n\n_Research note._\n\n");
            }
            TranscriptEntry::ResearchExport { .. } => {
                out.push_str("## Plume\n\n_Research note exported._\n\n");
            }
        }
    }

    out
}

/// A title containing newlines would otherwise break out of its heading and
/// restructure the document. Titles are user-supplied, so this is a contract
/// with the output format, not a defensive nicety.
fn escape_heading(title: &str) -> String {
    title.replace(['\n', '\r'], " ").trim().to_string()
}

/// Default file name offered by the save dialog.
pub fn default_file_name(record: &SessionRecord) -> String {
    let stem: String = record
        .title
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect();
    let trimmed = stem.trim_matches('-');
    if trimmed.is_empty() {
        "conversation.md".to_string()
    } else {
        let short: String = trimmed.chars().take(60).collect();
        format!("{}.md", short.trim_matches('-').to_lowercase())
    }
}
