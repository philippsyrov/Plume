//! Transcript export tests.
//!
//! Export exists so a full store has an exit that is not "delete your history",
//! which makes fidelity the thing to guard: an export that quietly omits a
//! stopped or failed turn misrepresents the conversation the user is keeping.

use super::export::{default_file_name, to_markdown};
use super::tests::{user_entry, TempDir};
use super::*;

fn record_with(entries: Vec<TranscriptEntry>, title: &str) -> SessionRecord {
    let td = TempDir::new("export");
    let created = create(td.path(), Some(title)).expect("create");
    save_transcript(td.path(), &created.id, &entries, false).expect("save");
    let loaded = load(td.path(), &created.id).expect("load");
    std::mem::forget(td);
    loaded
}

#[test]
fn a_conversation_renders_as_titled_markdown_with_both_speakers() {
    let record = record_with(
        vec![
            user_entry("how do I split a file?"),
            TranscriptEntry::Message {
                message: EntryMessage {
                    role: EntryRole::Assistant,
                    content: "Move the cohesive part out first.".into(),
                },
                model_used: Some("qwen-coder".into()),
                duration_ms: None,
                attachment_rel_path: None,
                attachment_line_range: None,
                stats: None,
                sent_in_mode: None,
                context_sources: None,
            },
        ],
        "Splitting files",
    );

    let markdown = to_markdown(&record);

    assert!(markdown.starts_with("# Splitting files\n"));
    assert!(markdown.contains("## You\n\nhow do I split a file?"));
    assert!(
        markdown.contains("## Plume (qwen-coder)"),
        "the model that answered is part of what happened",
    );
}

#[test]
fn a_stopped_turn_is_exported_rather_than_dropped() {
    // Dropping it would export a conversation that reads as though the answer
    // simply never came, which is not what the user saw.
    let record = record_with(
        vec![
            user_entry("write the migration"),
            TranscriptEntry::Cancelled {
                partial: "Here is the first ha".into(),
                model_used: None,
                duration_ms: None,
            },
        ],
        "Stopped",
    );

    let markdown = to_markdown(&record);
    assert!(markdown.contains("_Stopped by you._"));
    assert!(
        markdown.contains("Here is the first ha"),
        "the partial answer was on screen; an export that drops it disagrees \
         with the transcript it came from",
    );
}

#[test]
fn a_failed_turn_keeps_its_reason() {
    let record = record_with(
        vec![
            user_entry("summarize this"),
            TranscriptEntry::Error {
                message: "model unavailable".into(),
            },
        ],
        "Failed",
    );

    assert!(to_markdown(&record).contains("_Failed: model unavailable_"));
}

#[test]
fn a_title_cannot_restructure_the_document() {
    // Titles are user-supplied, so a newline would otherwise break out of the
    // heading and invent sections in the exported file.
    let record = record_with(vec![user_entry("hi")], "Line one\nLine two");

    let markdown = to_markdown(&record);

    assert!(markdown.starts_with("# Line one Line two\n"));
    assert_eq!(markdown.matches("# Line one").count(), 1);
}

#[test]
fn the_offered_file_name_is_derived_from_the_title() {
    let record = record_with(vec![user_entry("hi")], "Plan the Lisbon launch");
    assert_eq!(default_file_name(&record), "plan-the-lisbon-launch.md");
}

#[test]
fn a_title_with_no_usable_characters_still_offers_a_name() {
    let record = record_with(vec![user_entry("hi")], "***");
    assert_eq!(default_file_name(&record), "conversation.md");
}
