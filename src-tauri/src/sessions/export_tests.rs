//! Transcript export tests.
//!
//! Export exists so a full store has an exit that is not "delete your history",
//! which makes fidelity the thing to guard: an export that quietly omits a
//! stopped or failed turn misrepresents the conversation the user is keeping.

use super::export::{default_file_name, to_markdown, ResearchNotes};
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

    let markdown = to_markdown(&record, &ResearchNotes::new());

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

    let markdown = to_markdown(&record, &ResearchNotes::new());
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

    assert!(to_markdown(&record, &ResearchNotes::new()).contains("_Failed: model unavailable_"));
}

#[test]
fn a_title_cannot_restructure_the_document() {
    // Titles are user-supplied, so a newline would otherwise break out of the
    // heading and invent sections in the exported file.
    let record = record_with(vec![user_entry("hi")], "Line one\nLine two");

    let markdown = to_markdown(&record, &ResearchNotes::new());

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

#[test]
fn a_reply_cannot_invent_sections_in_the_exported_file() {
    // Replies routinely contain "# heading" as prose or code. Emitted raw, they
    // become real headings and the export stops matching the transcript's shape.
    let record = record_with(
        vec![user_entry("# Not a heading\n\n---\n\nplain line")],
        "Structure",
    );

    let markdown = to_markdown(&record, &ResearchNotes::new());

    assert!(markdown.contains("\\# Not a heading"));
    assert!(markdown.contains("\\---"));
    assert!(
        markdown.contains("plain line"),
        "ordinary lines stay untouched"
    );
    // The only headings are the ones this exporter wrote.
    let headings = markdown
        .lines()
        .filter(|l| l.starts_with("# ") || l.starts_with("## "));
    assert_eq!(headings.count(), 2, "the title and the one speaker heading");
}

#[test]
fn a_failure_message_cannot_close_the_emphasis_it_sits_inside() {
    let record = record_with(
        vec![
            user_entry("go"),
            TranscriptEntry::Error {
                message: "loading _model_ failed".into(),
            },
        ],
        "Emphasis",
    );

    let markdown = to_markdown(&record, &ResearchNotes::new());

    assert!(markdown.contains("_Failed: loading \\_model\\_ failed_"));
}

#[test]
fn a_research_entry_names_the_note_it_refers_to() {
    // A thread with several notes would otherwise export as identical
    // placeholders, losing which note each turn produced.
    let record = record_with(
        vec![
            user_entry("research this"),
            TranscriptEntry::ResearchExport {
                owner: TranscriptArtifactOwner {
                    scope: TranscriptArtifactScope::Local,
                    session_id: "s".repeat(34),
                },
                artifact_id: "a".repeat(34),
                version: 2,
                file_name: "lisbon.md".into(),
            },
        ],
        "Research",
    );

    let markdown = to_markdown(&record, &ResearchNotes::new());

    assert!(markdown.contains(&"a".repeat(34)));
    assert!(markdown.contains("version 2"));
    assert!(markdown.contains("lisbon.md"));
}

#[test]
fn a_research_note_body_travels_with_the_export() {
    // The note lives in the artifact store and is deleted with the
    // conversation, so an export taken before deleting a full store must carry
    // it — otherwise the backup silently loses the substance of the answer.
    let record = record_with(
        vec![
            user_entry("research this"),
            TranscriptEntry::ResearchArtifact {
                owner: TranscriptArtifactOwner {
                    scope: TranscriptArtifactScope::Local,
                    session_id: "s".repeat(34),
                },
                artifact_id: "a".repeat(34),
                version: 3,
            },
        ],
        "Note body",
    );
    let mut notes = ResearchNotes::new();
    notes.insert(("a".repeat(34), 3), "## Findings\n\nLisbon is warm.".into());

    let markdown = to_markdown(&record, &notes);

    assert!(markdown.contains("Lisbon is warm."));
    assert!(
        markdown.contains("\\## Findings"),
        "the note is prose in this document, not a section of it",
    );
}

#[test]
fn an_unreadable_research_note_says_so_rather_than_going_quiet() {
    let record = record_with(
        vec![TranscriptEntry::ResearchArtifact {
            owner: TranscriptArtifactOwner {
                scope: TranscriptArtifactScope::Local,
                session_id: "s".repeat(34),
            },
            artifact_id: "a".repeat(34),
            version: 1,
        }],
        "Missing note",
    );

    let markdown = to_markdown(&record, &ResearchNotes::new());

    assert!(markdown.contains("could not be read and is not included"));
}
