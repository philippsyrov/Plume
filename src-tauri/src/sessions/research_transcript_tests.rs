use serde_json::json;

use super::tests::{raw_conn, TempDir};
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
fn fork_keeps_immutable_research_refs_owned_by_the_source_chat() {
    let td = TempDir::new("research-transcript-fork");
    let dir = td.path().join("sessions");
    let source = create(&dir, Some("source")).unwrap();
    let reference = TranscriptEntry::ResearchArtifact {
        owner: TranscriptArtifactOwner {
            scope: TranscriptArtifactScope::Local,
            session_id: source.id.clone(),
        },
        artifact_id: "ra_1".to_string(),
        version: 1,
    };
    save_transcript(&dir, &source.id, std::slice::from_ref(&reference), false).unwrap();

    let child = fork(&dir, &source.id, false).unwrap();
    assert_ne!(child.id, source.id);
    assert_eq!(child.entries, vec![reference.clone()]);

    // The child continues to reference the immutable source-owned artifact;
    // a normal later transcript save must not rewrite or reject that owner.
    save_transcript(&dir, &child.id, &[reference], false).unwrap();
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

#[test]
fn load_rejects_semantically_corrupt_artifact_metadata() {
    let td = TempDir::new("corrupt-research-metadata");
    let dir = td.path().join("sessions");
    let session = create(&dir, Some("source")).unwrap();
    let reference = TranscriptEntry::ResearchArtifact {
        owner: TranscriptArtifactOwner {
            scope: TranscriptArtifactScope::Local,
            session_id: session.id.clone(),
        },
        artifact_id: "ra_1".to_string(),
        version: 1,
    };
    save_transcript(&dir, &session.id, &[reference], false).unwrap();
    let conn = raw_conn(&dir);
    conn.execute(
        "UPDATE chat_messages SET artifact_json=?1 WHERE session_id=?2",
        rusqlite::params![
            json!({
                "owner": { "scope": "local", "sessionId": session.id },
                "artifactId": "ra_1",
                "version": 0
            })
            .to_string(),
            session.id
        ],
    )
    .unwrap();
    drop(conn);

    assert!(matches!(
        load(&dir, &session.id),
        Err(SessionStoreError::Corrupt(_))
    ));
}

#[test]
fn scoped_load_rejects_artifact_owners_from_the_other_store() {
    for (label, stored_scope, corrupted_scope, project_store) in [
        (
            "local-owner-in-project",
            TranscriptArtifactScope::Project,
            "local",
            true,
        ),
        (
            "project-owner-in-local",
            TranscriptArtifactScope::Local,
            "project",
            false,
        ),
    ] {
        let td = TempDir::new(label);
        let dir = td.path().join("sessions");
        let session = create(&dir, Some("source")).unwrap();
        let reference = TranscriptEntry::ResearchArtifact {
            owner: TranscriptArtifactOwner {
                scope: stored_scope,
                session_id: session.id.clone(),
            },
            artifact_id: "ra_1".to_string(),
            version: 1,
        };
        save_transcript(&dir, &session.id, &[reference], project_store).unwrap();
        let conn = raw_conn(&dir);
        conn.execute(
            "UPDATE chat_messages SET artifact_json=?1 WHERE session_id=?2",
            rusqlite::params![
                json!({
                    "owner": { "scope": corrupted_scope, "sessionId": session.id },
                    "artifactId": "ra_1",
                    "version": 1
                })
                .to_string(),
                session.id
            ],
        )
        .unwrap();
        drop(conn);

        assert!(matches!(
            load_for_scope(&dir, &session.id, project_store),
            Err(SessionStoreError::Corrupt(_))
        ));
    }
}
