//! Durable storage policy tests.
//!
//! The rule these guard is the one the conversation design rests on: at the
//! cap Plume refuses to save, and never trims or deletes a transcript to make
//! room. The refusal decision is pure, so it is tested directly rather than by
//! filling half a gigabyte of disk.

use super::storage::{admits_branch_usage, admits_write, full_store_refusal, StorageUsage};
use super::tests::{raw_conn, user_entry, TempDir};
use super::*;

fn at(used_bytes: u64) -> StorageUsage {
    StorageUsage {
        used_bytes,
        warn_bytes: 90,
        cap_bytes: 100,
    }
}

#[test]
fn a_store_below_the_cap_admits_a_write_that_fits() {
    let usage = at(50);
    assert!(usage.used_bytes < usage.warn_bytes);
    assert!(admits_write(usage, 0, 40));
}

#[test]
fn a_store_below_the_cap_still_refuses_a_write_that_would_overshoot_it() {
    // Asking only "is it full yet?" would admit any single write while one page
    // remained. A transcript can be megabytes, so that one save carries the
    // store past a cap it was under a moment earlier.
    let usage = at(50);
    assert!(
        usage.used_bytes < usage.cap_bytes,
        "below the cap, and still refused"
    );
    assert!(!admits_write(usage, 0, 10_000));
}

#[test]
fn a_store_nearing_the_cap_warns_but_still_admits_writes() {
    let usage = at(95);
    assert!(
        usage.used_bytes >= usage.warn_bytes,
        "the user needs warning before writes stop"
    );
    assert!(usage.used_bytes < usage.cap_bytes);
    assert!(admits_write(usage, 0, 5), "a write that still fits lands");
    assert!(
        !admits_write(usage, 0, 10_000),
        "one that would overshoot does not"
    );
}

#[test]
fn a_full_store_refuses_a_write_that_grows_it() {
    let usage = at(100);
    assert!(usage.used_bytes >= usage.cap_bytes);
    assert!(!admits_write(usage, 500, 501));
}

#[test]
fn a_full_store_still_admits_a_write_that_shrinks_or_holds() {
    // Otherwise a user who filled the store could not edit their way back under
    // the cap, and deleting whole conversations would be the only exit.
    let usage = at(120);
    assert!(admits_write(usage, 500, 499), "shrinking must land");
    assert!(admits_write(usage, 500, 500), "an unchanged save must land");
}

#[test]
fn a_branch_is_decided_by_actual_page_usage_before_commit() {
    assert!(admits_branch_usage(at(99), at(100)));
    assert!(
        !admits_branch_usage(at(99), at(101)),
        "a transaction that actually crossed the cap must roll back",
    );
    assert!(
        !admits_branch_usage(at(100), at(100)),
        "a branch is growth and cannot start once the store is already full",
    );
}

#[test]
fn branch_measurement_flushes_deferred_fts_pages() {
    let td = TempDir::new("branch-fts-flush");
    let source = create(td.path(), Some("source")).expect("create source");
    let content = (0..20_000)
        .map(|index| format!("term{index:05}"))
        .collect::<Vec<_>>()
        .join(" ");
    let mut conn = raw_conn(td.path());
    let tx = conn.transaction().expect("begin write");
    tx.execute(
        "INSERT INTO chat_messages
         (id, session_id, ordinal, kind, role, content, created_at_ms)
         VALUES (?1, ?2, 0, 'message', 'user', ?3, 1)",
        rusqlite::params!["m_0123456789abcdef0123456789abcdef", source.id, content],
    )
    .expect("insert message");

    let before_flush = super::storage::usage(&tx).expect("usage before FTS flush");
    super::branch::flush_pending_search_index(&tx).expect("flush pending FTS terms");
    let after_flush = super::storage::usage(&tx).expect("usage after FTS flush");

    assert!(
        after_flush.used_bytes > before_flush.used_bytes,
        "the pre-commit gate must include pages FTS5 otherwise defers to transaction sync",
    );
}

#[test]
fn the_refusal_is_its_own_type_and_carries_the_numbers() {
    // Not a `Limit`. The surface has to tell "your history has nowhere to go"
    // apart from an ordinary bounded-input refusal, and it must not decide that
    // by reading the message text — control flow never parses an error string.
    let refusal = full_store_refusal(StorageUsage {
        used_bytes: 400 * 1024 * 1024,
        warn_bytes: 0,
        cap_bytes: 512 * 1024 * 1024,
    });
    let SessionStoreError::StorageFull {
        used_bytes,
        cap_bytes,
    } = refusal
    else {
        panic!("a store with no room must refuse with StorageFull, got {refusal:?}");
    };
    assert_eq!(used_bytes, 400 * 1024 * 1024);
    assert_eq!(cap_bytes, 512 * 1024 * 1024);
    // Below the cap and still refused: the decision is on projected usage, so
    // the numbers the user is shown must be the store's own, not an inference
    // that it must have been at 100%.
    assert!(used_bytes < cap_bytes);
    assert!(
        refusal.to_string().contains("400 MB of 512 MB"),
        "the log-grade message states the fact; got {refusal}",
    );
}

#[test]
fn an_over_cap_branch_rolls_back_the_real_transaction() {
    fn populated_store(label: &str) -> (TempDir, SessionSummary) {
        let td = TempDir::new(label);
        let created = create(td.path(), Some("chat")).expect("create session");
        let entries: Vec<_> = (0..40)
            .map(|index| user_entry(&format!("turn {index}: {}", "lorem ipsum ".repeat(60))))
            .collect();
        save_transcript(td.path(), &created.id, &entries, false).expect("save");
        (td, created)
    }

    let (control_dir, control_source) = populated_store("branch-cap-control");
    let control_before = storage_usage(control_dir.path()).expect("control before");
    fork(control_dir.path(), &control_source.id, false).expect("control fork");
    let control_after = storage_usage(control_dir.path()).expect("control after");
    let branch_growth = control_after
        .used_bytes
        .checked_sub(control_before.used_bytes)
        .expect("a branch grows page usage");
    assert!(branch_growth > 1, "fixture must allocate at least one page");

    let (subject_dir, subject_source) = populated_store("branch-cap-subject");
    let subject_before = storage_usage(subject_dir.path()).expect("subject before");
    assert_eq!(subject_before.used_bytes, control_before.used_bytes);
    let cap = subject_before.used_bytes + branch_growth - 1;

    assert!(matches!(
        super::branch::fork_with_cap(subject_dir.path(), &subject_source.id, false, cap),
        Err(SessionStoreError::Limit(_))
    ));
    assert_eq!(
        list(subject_dir.path(), true)
            .expect("list after refusal")
            .len(),
        1,
        "the over-cap child must be rolled back",
    );
    assert_eq!(
        storage_usage(subject_dir.path())
            .expect("usage after refusal")
            .used_bytes,
        subject_before.used_bytes,
        "the refused transaction must not leave allocated pages in use",
    );
}

#[test]
fn usage_reports_a_real_store_against_the_shipped_budget() {
    let td = TempDir::new("storage-usage");
    let created = create(td.path(), Some("chat")).expect("create session");
    save_transcript(td.path(), &created.id, &[user_entry("hello")], false).expect("save");

    let reported = storage_usage(td.path()).expect("usage");
    assert!(reported.used_bytes > 0, "a store with rows occupies pages");
    assert_eq!(reported.cap_bytes, 512 * 1024 * 1024);
    assert!(reported.warn_bytes < reported.cap_bytes);
    assert!(reported.used_bytes < reported.cap_bytes);
}

#[test]
fn deleting_a_conversation_is_a_real_recovery_path() {
    // Pages in use, not file size and not raw page_count: both keep their value
    // after a delete, so either would leave the documented recovery path a dead
    // end — the user deletes a chat and writes still refuse.
    let td = TempDir::new("storage-recovery");
    let keep = create(td.path(), Some("keep")).expect("keep");
    let drop = create(td.path(), Some("drop")).expect("drop");
    let bulky = vec![user_entry(&"x".repeat(200 * 1024)); 20];
    save_transcript(td.path(), &drop.id, &bulky, false).expect("fill");

    let before = storage_usage(td.path()).expect("before").used_bytes;
    delete(td.path(), &drop.id).expect("delete");
    let after = storage_usage(td.path()).expect("after").used_bytes;

    assert!(
        after < before,
        "deleting a conversation must return space to the budget: {before} -> {after}",
    );
    assert!(load(td.path(), &keep.id).is_ok(), "the other chat survives");
}

#[test]
fn a_non_ascii_transcript_is_measured_in_bytes_on_both_sides() {
    // SQLite's LENGTH() counts characters for TEXT, Rust's len() counts bytes.
    // Comparing one against the other made every non-Latin conversation look
    // smaller in the store than the save being weighed against it, so a user
    // could delete a third of a Cyrillic chat and still be refused.
    let td = TempDir::new("storage-utf8");
    let created = create(td.path(), Some("кириллица")).expect("create");
    let text = "привет".repeat(1000);
    save_transcript(td.path(), &created.id, &[user_entry(&text)], false).expect("save");

    let conn = raw_conn(td.path());
    let stored: i64 = conn
        .query_row(
            "SELECT SUM(LENGTH(CAST(content AS BLOB))) FROM chat_messages WHERE session_id = ?1",
            rusqlite::params![created.id],
            |row| row.get(0),
        )
        .expect("measure stored bytes");

    assert_eq!(
        u64::try_from(stored).unwrap(),
        text.len() as u64,
        "the store and the incoming transcript must be measured the same way",
    );
    assert!(
        text.len() > text.chars().count(),
        "the fixture must be multi-byte"
    );
}

#[test]
fn a_cancelled_turn_is_measured_by_the_text_it_stores() {
    // Cancelled turns persist their partial answer in the same content column.
    // Measuring them as zero would let a user keep writing past a full store
    // simply by stopping each reply.
    let entry = TranscriptEntry::Cancelled {
        partial: "x".repeat(500),
        model_used: None,
        duration_ms: None,
    };
    assert_eq!(validation::entry_content_len(&entry), 500);
}

#[test]
fn a_research_entry_is_measured_as_the_store_actually_writes_it() {
    // Its payload goes to artifact_json, not content, so counting its JSON here
    // would refuse an unchanged save of a chat that contains one.
    let entry = TranscriptEntry::ResearchExport {
        owner: TranscriptArtifactOwner {
            scope: TranscriptArtifactScope::Local,
            session_id: "s".repeat(34),
        },
        artifact_id: "a".repeat(34),
        version: 1,
        file_name: "note.md".into(),
    };
    assert_eq!(validation::entry_content_len(&entry), 0);
}

#[test]
fn the_cap_weighs_the_whole_row_not_only_its_prose() {
    // A save that keeps the same words but carries a heavier per-entry manifest
    // still grows the store. Measuring content alone would call that unchanged
    // and let a full store keep growing.
    let bare = TranscriptEntry::Message {
        message: EntryMessage {
            role: EntryRole::User,
            content: "same words".into(),
        },
        model_used: None,
        duration_ms: None,
        attachment_rel_path: None,
        attachment_line_range: None,
        stats: None,
        sent_in_mode: None,
        context_sources: None,
    };
    let TranscriptEntry::Message { message, .. } = bare.clone() else {
        unreachable!()
    };
    let heavy = TranscriptEntry::Message {
        message,
        model_used: None,
        duration_ms: None,
        attachment_rel_path: None,
        attachment_line_range: None,
        stats: None,
        sent_in_mode: None,
        context_sources: Some(vec![ContextSourceManifestItem::UserMemoryEntry {
            entry_id: "m".repeat(34),
            created_at_ms: 1,
            bytes: 200,
            preview: "a".repeat(200),
        }]),
    };

    assert_eq!(
        validation::entry_content_len(&bare),
        validation::entry_content_len(&heavy),
        "the prose is identical, which is exactly why content alone is not enough",
    );
    assert!(
        validation::entry_row_len(&heavy) > validation::entry_row_len(&bare),
        "the row the store writes is bigger, so the cap must see it as bigger",
    );
}
