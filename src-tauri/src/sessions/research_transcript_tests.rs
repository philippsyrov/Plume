use serde_json::json;

use super::tests::TempDir;
use super::*;

#[test]
fn transcript_round_trips_research_artifact_and_export_refs_without_paths() {
    let td = TempDir::new("research-transcript-refs");
    let dir = td.path().join("sessions");
    let session = create(&dir, Some("dinosaurs")).unwrap();
    let owner = TranscriptArtifactOwner {
        scope: TranscriptArtifactScope::Local,
        session_id: session.id.clone(),
    };
    let entries = vec![
        TranscriptEntry::ResearchArtifact {
            owner: owner.clone(),
            artifact_id: "ra_1".to_string(),
            version: 2,
        },
        TranscriptEntry::ResearchExport {
            owner,
            artifact_id: "ra_1".to_string(),
            version: 2,
            file_name: "dinosaurs.md".to_string(),
        },
    ];

    save_transcript(&dir, &session.id, &entries, false).unwrap();

    assert_eq!(load(&dir, &session.id).unwrap().entries, entries);
}

#[test]
fn research_transcript_refs_reject_unknown_fields_and_unsafe_filenames() {
    let owner = json!({ "scope": "local", "sessionId": "s_1" });
    let smuggled = json!({
        "kind": "researchArtifact",
        "owner": owner,
        "artifactId": "ra_1",
        "version": 1,
        "path": "/tmp/note.md"
    });
    assert!(parse_entries(&[smuggled]).is_err());

    let unsafe_name = json!({
        "kind": "researchExport",
        "owner": { "scope": "local", "sessionId": "s_1" },
        "artifactId": "ra_1",
        "version": 1,
        "fileName": "../note.md"
    });
    let parsed =
        parse_entries(&[unsafe_name]).expect("wire shape parses before semantic validation");
    let err = save_transcript(
        &TempDir::new("unsafe-export-name").path().join("sessions"),
        "s_1",
        &parsed,
        false,
    )
    .expect_err("unsafe filename rejected");
    assert!(matches!(err, SessionStoreError::Invalid(_)));
}
