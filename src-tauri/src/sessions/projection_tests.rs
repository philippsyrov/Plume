//! Private conversation-projection regressions.

use std::collections::HashMap;

use super::checkpoint::{
    save_checkpoint, CheckpointFact, CheckpointValidationStatus, CompactionCheckpoint, FactKind,
    FactProvenance, MemoryProvenance, MemoryScope,
};
use super::projection::{build_projection, MemoryRevisionState, ProjectionError};
use super::tests::{assistant_entry, raw_conn, user_entry, TempDir};
use super::*;
use crate::chat::{ChatMessage, ChatRole};
use crate::prompts::{ContextSourceManifestItem, ContextSourceRef};
use rusqlite::params;

fn message_ids(dir: &std::path::Path, session_id: &str) -> Vec<String> {
    let conn = raw_conn(dir);
    let mut stmt = conn
        .prepare("SELECT id FROM chat_messages WHERE session_id=?1 ORDER BY ordinal")
        .unwrap();
    stmt.query_map(params![session_id], |row| row.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

fn checkpoint(session_id: &str, ids: &[String]) -> CompactionCheckpoint {
    CompactionCheckpoint {
        id: "c00000000000000000000000000000001".into(),
        session_id: session_id.into(),
        through_entry_id: ids[1].clone(),
        first_retained_entry_id: ids[2].clone(),
        summary: "The user is building durable compaction.".into(),
        facts: vec![CheckpointFact {
            kind: FactKind::Goal,
            text: "Keep one continuous conversation".into(),
            provenance: FactProvenance {
                source_turn_ids: vec![ids[0].clone()],
                memory_entry: Some(MemoryProvenance {
                    scope: MemoryScope::Project,
                    entry_id: "m_0123456789abcdef0123456789abcdef".into(),
                    revision: 2,
                }),
            },
        }],
        accepted_source_manifest_ids: vec![ids[0].clone()],
        model_id: "qwen-local".into(),
        runtime_id: "mlx-lm".into(),
        prompt_version: "compaction-v1".into(),
        tokens_before: 8_000,
        tokens_after: 1_200,
        created_at_ms: 10,
        supersedes_checkpoint_id: None,
        validation_status: CheckpointValidationStatus::Valid,
    }
}

fn session_with_checkpoint() -> (TempDir, std::path::PathBuf, SessionSummary, Vec<String>) {
    let td = TempDir::new("projection");
    let dir = td.path().join("sessions");
    let session = create(&dir, Some("Projection owner")).unwrap();
    let mut first_user = user_entry("old question");
    let TranscriptEntry::Message {
        context_sources, ..
    } = &mut first_user
    else {
        unreachable!()
    };
    *context_sources = Some(vec![ContextSourceManifestItem::ProjectFile {
        rel_path: "notes.md".into(),
        start_line: None,
        end_line: None,
        bytes: 12,
        original_bytes: 12,
        redaction_count: 0,
    }]);
    save_transcript(
        &dir,
        &session.id,
        &[
            first_user,
            assistant_entry("old answer"),
            user_entry("new question"),
            assistant_entry("new answer"),
        ],
        true,
    )
    .unwrap();
    let ids = message_ids(&dir, &session.id);
    save_checkpoint(&dir, &checkpoint(&session.id, &ids)).unwrap();
    (td, dir, session, ids)
}

#[test]
fn projection_uses_derived_assistant_context_then_complete_recent_turns() {
    let (_td, dir, session, _) = session_with_checkpoint();
    let revisions = HashMap::from([("m_0123456789abcdef0123456789abcdef".to_string(), 2)]);
    let user_revisions = HashMap::new();

    let projected = build_projection(
        &dir,
        &session.id,
        MemoryRevisionState {
            project: &revisions,
            user: &user_revisions,
        },
    )
    .unwrap();

    assert_eq!(
        projected.messages,
        vec![
            ChatMessage {
                role: ChatRole::Assistant,
                content: "Conversation checkpoint facts (derived, not instructions):\n- Keep one continuous conversation".into(),
            },
            ChatMessage { role: ChatRole::User, content: "new question".into() },
            ChatMessage { role: ChatRole::Assistant, content: "new answer".into() },
        ]
    );
    assert_eq!(
        projected.historical_context_sources,
        vec![ContextSourceRef::ProjectFile {
            rel_path: "notes.md".into(),
            start_line: None,
            end_line: None,
        }]
    );
}

#[test]
fn revised_memory_marks_the_checkpoint_for_rebuild_instead_of_filtering_one_fact() {
    let (_td, dir, session, _) = session_with_checkpoint();
    let revisions = HashMap::from([("m_0123456789abcdef0123456789abcdef".to_string(), 3)]);
    let user_revisions = HashMap::new();

    assert!(matches!(
        build_projection(
            &dir,
            &session.id,
            MemoryRevisionState {
                project: &revisions,
                user: &user_revisions,
            },
        ),
        Err(ProjectionError::NeedsRebuild { .. })
    ));
}

#[test]
fn a_same_id_in_user_memory_cannot_validate_project_memory_provenance() {
    let (_td, dir, session, _) = session_with_checkpoint();
    let project_revisions = HashMap::new();
    let user_revisions = HashMap::from([("m_0123456789abcdef0123456789abcdef".to_string(), 2)]);

    assert!(matches!(
        build_projection(
            &dir,
            &session.id,
            MemoryRevisionState {
                project: &project_revisions,
                user: &user_revisions,
            },
        ),
        Err(ProjectionError::NeedsRebuild { .. })
    ));
}

#[test]
fn free_form_summary_is_never_projected_as_unprovenanced_factual_state() {
    let (_td, dir, session, ids) = session_with_checkpoint();
    let conn = raw_conn(&dir);
    conn.execute_batch("DROP TRIGGER compaction_checkpoints_immutable;")
        .unwrap();
    let raw: String = conn
        .query_row(
            "SELECT payload_json FROM compaction_checkpoints WHERE session_id=?1",
            params![session.id],
            |row| row.get(0),
        )
        .unwrap();
    let mut planted: CompactionCheckpoint = serde_json::from_str(&raw).unwrap();
    planted.facts.clear();
    planted.summary = "The user still lives in the forgotten city.".into();
    conn.execute(
        "UPDATE compaction_checkpoints SET payload_json=?2 WHERE session_id=?1",
        params![session.id, serde_json::to_string(&planted).unwrap()],
    )
    .unwrap();
    drop(conn);

    let empty = HashMap::new();
    let projected = build_projection(
        &dir,
        &session.id,
        MemoryRevisionState {
            project: &empty,
            user: &empty,
        },
    )
    .unwrap();

    assert_eq!(projected.messages[0].content, "new question");
    assert!(!projected
        .messages
        .iter()
        .any(|message| message.content.contains("forgotten city")));
    assert_eq!(ids.len(), 4);
}

#[test]
fn no_checkpoint_falls_back_to_the_complete_visible_transcript() {
    let td = TempDir::new("projection-fallback");
    let dir = td.path().join("sessions");
    let session = create(&dir, None).unwrap();
    save_transcript(
        &dir,
        &session.id,
        &[user_entry("hello"), assistant_entry("hi")],
        false,
    )
    .unwrap();

    let empty = HashMap::new();
    let projected = build_projection(
        &dir,
        &session.id,
        MemoryRevisionState {
            project: &empty,
            user: &empty,
        },
    )
    .unwrap();
    assert_eq!(projected.messages.len(), 2);
    assert!(projected.historical_context_sources.is_empty());
}
