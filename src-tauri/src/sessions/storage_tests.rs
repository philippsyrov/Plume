//! Durable storage policy tests.
//!
//! The rule these guard is the one the conversation design rests on: at the
//! cap Plume refuses to save, and never trims or deletes a transcript to make
//! room. The refusal decision is pure, so it is tested directly rather than by
//! filling half a gigabyte of disk.

use super::storage::{admits_write, full_store_refusal, StorageUsage};
use super::tests::{user_entry, TempDir};
use super::*;

fn at(used_bytes: u64) -> StorageUsage {
    StorageUsage {
        used_bytes,
        warn_bytes: 90,
        cap_bytes: 100,
    }
}

#[test]
fn a_store_below_the_cap_admits_any_write() {
    let usage = at(50);
    assert!(!usage.is_full());
    assert!(usage.used_bytes < usage.warn_bytes);
    assert!(admits_write(usage, 0, 10_000));
}

#[test]
fn a_store_nearing_the_cap_warns_but_still_admits_writes() {
    let usage = at(95);
    assert!(
        usage.used_bytes >= usage.warn_bytes,
        "the user needs warning before writes stop"
    );
    assert!(!usage.is_full());
    assert!(admits_write(usage, 0, 10_000));
}

#[test]
fn a_full_store_refuses_a_write_that_grows_it() {
    let usage = at(100);
    assert!(usage.is_full());
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
fn the_refusal_names_what_is_full_and_what_to_do() {
    let SessionStoreError::Limit(message) = full_store_refusal(StorageUsage {
        used_bytes: 512 * 1024 * 1024,
        warn_bytes: 0,
        cap_bytes: 512 * 1024 * 1024,
    }) else {
        panic!("a full store must refuse with Limit, which maps to a Blocked IPC error");
    };
    assert!(message.contains("512 MB of 512 MB"));
    assert!(
        message.contains("Nothing has been deleted"),
        "the user must be told their history is intact, since a refusal to save \
         reads like data loss otherwise",
    );
    assert!(message.contains("Delete a conversation"));
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
    assert!(!reported.is_full());
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
