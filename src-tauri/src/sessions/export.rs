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

use std::collections::HashMap;

use super::{EntryRole, SessionRecord, TranscriptEntry};

/// Research-note bodies, keyed by artifact id and version.
///
/// The transcript stores only a reference; the note itself lives in the
/// artifact store, and deleting the conversation deletes it too. An export
/// taken *before* deleting a full store would therefore lose the substantive
/// note unless its body travels with the transcript, so the command layer
/// resolves the bodies and hands them in here.
pub type ResearchNotes = HashMap<(String, u32), String>;

/// Markdown for one conversation, including the entries Plume would rather the
/// user did not lose track of: cancellations and errors appear as themselves
/// rather than being silently dropped, because an export that quietly omits
/// them misrepresents what happened in the conversation.
pub fn to_markdown(record: &SessionRecord, notes: &ResearchNotes) -> String {
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
                out.push_str(&format!("## {who}\n\n{}\n\n", body(&message.content)));
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
                let trimmed = body(partial);
                if !trimmed.is_empty() {
                    out.push_str(&format!("{trimmed}\n\n"));
                }
                out.push_str("_Stopped by you._\n\n");
            }
            TranscriptEntry::Error { message, .. } => {
                // Emphasis wrapping is the export's own framing, so the
                // message inside it must not be able to close that emphasis
                // early and misstate the failure.
                out.push_str(&format!("## Plume\n\n_Failed: {}_\n\n", inline(message)));
            }
            // Research entries reference a note stored outside the transcript,
            // so the export names which one rather than flattening every note
            // in a thread to the same placeholder.
            TranscriptEntry::ResearchArtifact {
                artifact_id,
                version,
                ..
            } => {
                out.push_str("## Plume\n\n");
                out.push_str(&format!(
                    "_Research note {} (version {version})._\n\n",
                    inline(artifact_id),
                ));
                match notes.get(&(artifact_id.clone(), *version)) {
                    Some(markdown) => out.push_str(&format!("{}\n\n", body(markdown))),
                    // Said plainly rather than omitted: a silent gap would make
                    // this read like the note never had content.
                    None => out.push_str("_This note could not be read and is not included._\n\n"),
                }
            }
            TranscriptEntry::ResearchExport {
                artifact_id,
                version,
                file_name,
                ..
            } => {
                out.push_str(&format!(
                    "## Plume\n\n_Research note {} (version {version}) exported as {}._\n\n",
                    inline(artifact_id),
                    inline(file_name),
                ));
            }
        }
    }

    out
}

/// A transcript body is prose the user wrote or the model produced, not
/// Markdown this file authored. Indenting it by a blockquote would change how
/// it reads, so instead the two constructs that could restructure the document
/// from column zero — ATX headings and thematic breaks — are neutralised, and
/// everything else is left as the user saw it.
fn body(content: &str) -> String {
    let mut fence: Option<(char, usize)> = None;
    let mut lines = Vec::new();

    for line in content.trim_end_matches(['\n', '\r']).lines() {
        let marker_and_text = line.trim_start_matches(' ');
        let leading_spaces = line.len() - marker_and_text.len();

        if let Some((marker, opening_len)) = fence {
            lines.push(line.to_string());
            if leading_spaces <= 3 && closes_fence(marker_and_text, marker, opening_len) {
                fence = None;
            }
            continue;
        }

        if leading_spaces <= 3 {
            if let Some(opening) = opens_fence(marker_and_text) {
                fence = Some(opening);
                lines.push(line.to_string());
                continue;
            }
        }

        let starts_thematic_marker = marker_and_text.starts_with('-')
            || marker_and_text.starts_with('*')
            || marker_and_text.starts_with('_');
        if leading_spaces <= 3
            && (marker_and_text.starts_with('#')
                || (starts_thematic_marker && is_thematic_break(marker_and_text)))
        {
            let (indent, marker_and_text) = line.split_at(leading_spaces);
            lines.push(format!("{indent}\\{marker_and_text}"));
        } else {
            lines.push(line.to_string());
        }
    }

    if let Some((marker, opening_len)) = fence {
        lines.push(marker.to_string().repeat(opening_len));
    }

    lines.join("\n")
}

fn opens_fence(line: &str) -> Option<(char, usize)> {
    let marker = line.chars().next()?;
    if marker != '`' && marker != '~' {
        return None;
    }
    let run_len = line
        .chars()
        .take_while(|candidate| *candidate == marker)
        .count();
    if run_len < 3 {
        return None;
    }
    let remainder = &line[run_len..];
    if marker == '`' && remainder.contains('`') {
        return None;
    }
    Some((marker, run_len))
}

fn closes_fence(line: &str, marker: char, opening_len: usize) -> bool {
    let run_len = line
        .chars()
        .take_while(|candidate| *candidate == marker)
        .count();
    run_len >= opening_len
        && line[run_len..]
            .chars()
            .all(|candidate| candidate == ' ' || candidate == '\t')
}

fn is_thematic_break(line: &str) -> bool {
    let stripped: String = line.chars().filter(|c| !c.is_whitespace()).collect();
    stripped.len() >= 3
        && (stripped.chars().all(|c| c == '-')
            || stripped.chars().all(|c| c == '*')
            || stripped.chars().all(|c| c == '_'))
}

/// Text placed inside the export's own emphasis or heading markers, where a
/// stray `_` or `*` would close the construct early.
fn inline(text: &str) -> String {
    text.replace(['\n', '\r'], " ")
        .replace('_', "\\_")
        .replace('*', "\\*")
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
