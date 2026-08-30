use std::path::Path;

use rusqlite::params;

use super::tests::{assistant_entry, raw_conn, user_entry, TempDir};
use super::{create, save_transcript};

fn message_ids(sessions_dir: &Path, session_id: &str) -> Vec<String> {
    let conn = raw_conn(sessions_dir);
    let mut stmt = conn
        .prepare("SELECT id FROM chat_messages WHERE session_id=?1 ORDER BY ordinal")
        .unwrap();
    stmt.query_map(params![session_id], |row| row.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

#[test]
fn stable_turn_ids_survive_an_identical_transcript_save() {
    let td = TempDir::new("stable-identical-turns");
    let dir = td.path().join("sessions");
    let session = create(&dir, None).unwrap();
    let entries = vec![user_entry("question"), assistant_entry("answer")];

    save_transcript(&dir, &session.id, &entries, false).unwrap();
    let first_ids = message_ids(&dir, &session.id);
    save_transcript(&dir, &session.id, &entries, false).unwrap();

    assert_eq!(message_ids(&dir, &session.id), first_ids);
}

#[test]
fn stable_turn_ids_survive_when_a_turn_is_appended() {
    let td = TempDir::new("stable-appended-turn");
    let dir = td.path().join("sessions");
    let session = create(&dir, None).unwrap();
    let mut entries = vec![user_entry("question"), assistant_entry("answer")];

    save_transcript(&dir, &session.id, &entries, false).unwrap();
    let first_ids = message_ids(&dir, &session.id);
    entries.push(user_entry("follow-up"));
    save_transcript(&dir, &session.id, &entries, false).unwrap();
    let appended_ids = message_ids(&dir, &session.id);

    assert_eq!(&appended_ids[..first_ids.len()], first_ids);
    assert!(!first_ids.contains(&appended_ids[2]));
}

#[test]
fn stable_turn_ids_change_only_for_a_rewritten_turn() {
    let td = TempDir::new("rewritten-turn-identity");
    let dir = td.path().join("sessions");
    let session = create(&dir, None).unwrap();
    let original = vec![
        user_entry("question"),
        assistant_entry("first answer"),
        user_entry("follow-up"),
    ];
    save_transcript(&dir, &session.id, &original, false).unwrap();
    let original_ids = message_ids(&dir, &session.id);

    let rewritten = vec![
        original[0].clone(),
        assistant_entry("corrected answer"),
        original[2].clone(),
    ];
    save_transcript(&dir, &session.id, &rewritten, false).unwrap();
    let rewritten_ids = message_ids(&dir, &session.id);

    assert_eq!(rewritten_ids[0], original_ids[0]);
    assert_ne!(rewritten_ids[1], original_ids[1]);
    assert_eq!(rewritten_ids[2], original_ids[2]);
}
