use super::tests::{user_entry, TempDir};
use super::*;
use crate::browser::evidence::BrowserCaptureKind;

#[test]
fn project_shelf_and_accepted_turn_manifest_round_trip_in_order() {
    let td = TempDir::new("context-round-trip");
    let dir = td.path().join("sessions");
    let session = create(&dir, Some("context")).unwrap();
    let shelf = vec![
        ContextSourceRef::ProjectFile {
            rel_path: "src/lib.rs".into(),
            start_line: Some(4),
            end_line: Some(9),
        },
        ContextSourceRef::MemoryEntry {
            entry_id: "m_0123456789abcdef0123456789abcdef".into(),
        },
        ContextSourceRef::TopicFile {
            name: "topics/architecture.md".into(),
        },
        ContextSourceRef::BrowserTextEvidence {
            evidence_id: "be_0123456789abcdef0123456789abcdef".into(),
        },
        ContextSourceRef::BrowserScreenshotEvidence {
            evidence_id: "bs_0123456789abcdef0123456789abcdef".into(),
        },
    ];
    let manifest = vec![
        ContextSourceManifestItem::ProjectFile {
            rel_path: "src/lib.rs".into(),
            start_line: Some(4),
            end_line: Some(9),
            bytes: 120,
            original_bytes: 180,
            redaction_count: 1,
        },
        ContextSourceManifestItem::MemoryEntry {
            entry_id: "m_0123456789abcdef0123456789abcdef".into(),
            created_at_ms: 7,
            bytes: 12,
            preview: "remember this".into(),
        },
        ContextSourceManifestItem::TopicFile {
            name: "topics/architecture.md".into(),
            bytes: 30,
        },
        ContextSourceManifestItem::BrowserTextEvidence {
            evidence_id: "be_0123456789abcdef0123456789abcdef".into(),
            capture_kind: BrowserCaptureKind::Page,
            source_url: "https://example.com/research".into(),
            title: Some("Research".into()),
            captured_at_ms: 9,
            bytes: 42,
            redaction_count: 1,
            truncated: false,
            preview: "A short research excerpt".into(),
        },
        ContextSourceManifestItem::BrowserScreenshotEvidence {
            evidence_id: "bs_0123456789abcdef0123456789abcdef".into(),
            source_url: "https://example.com/diagram".into(),
            title: Some("Architecture diagram".into()),
            captured_at_ms: 10,
            width: 1440,
            height: 900,
            bytes: 81_135,
            sha256: "ab".repeat(32),
        },
    ];
    let entries = vec![TranscriptEntry::Message {
        message: EntryMessage {
            role: EntryRole::User,
            content: "use these".into(),
        },
        model_used: None,
        duration_ms: None,
        attachment_rel_path: None,
        attachment_line_range: None,
        stats: None,
        sent_in_mode: Some(SentMode::Chat),
        context_sources: Some(manifest),
    }];

    save_transcript_with_context(&dir, &session.id, &entries, &shelf, true).unwrap();
    let loaded = load(&dir, &session.id).unwrap();
    assert_eq!(loaded.context_sources, shelf);
    assert_eq!(loaded.entries, entries);
}

#[test]
fn local_scope_rejects_shelf_and_turn_context_manifest() {
    let td = TempDir::new("local-context-rejected");
    let dir = td.path().join("sessions");
    let session = create(&dir, None).unwrap();
    let shelf = vec![ContextSourceRef::TopicFile {
        name: "topics/testing.md".into(),
    }];
    assert!(matches!(
        save_transcript_with_context(&dir, &session.id, &[], &shelf, false),
        Err(SessionStoreError::Invalid(_))
    ));

    let mut entry = user_entry("local");
    if let TranscriptEntry::Message {
        context_sources, ..
    } = &mut entry
    {
        *context_sources = Some(vec![ContextSourceManifestItem::TopicFile {
            name: "topics/testing.md".into(),
            bytes: 4,
        }]);
    }
    assert!(matches!(
        save_transcript_with_context(&dir, &session.id, &[entry], &[], false),
        Err(SessionStoreError::Invalid(_))
    ));
}
