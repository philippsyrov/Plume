use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};

use super::user_store::{forget, read_index, remember, search, update, user_memory_dir};
use super::{MAX_BYTES_TOTAL, MAX_ENTRIES};

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(label: &str) -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0);
        let path = std::env::temp_dir().join(format!(
            "plume-user-memory-{label}-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(&path).expect("create temp directory");
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn response_json<T: serde::Serialize>(response: &T) -> Value {
    serde_json::to_value(response).expect("response serializes")
}

fn remembered_id(response: &impl serde::Serialize) -> String {
    response_json(response)["entry"]["id"]
        .as_str()
        .expect("remembered id")
        .to_string()
}

#[test]
fn app_data_owns_one_stable_user_memory_directory() {
    let temp = TempDir::new("owned-path");
    assert_eq!(user_memory_dir(temp.path()), temp.path().join("memory"));
}

#[test]
fn missing_store_reads_as_empty_with_shared_caps() {
    let temp = TempDir::new("empty");
    let index = read_index(&user_memory_dir(temp.path())).expect("empty index");
    assert!(index.entries.is_empty());
    assert_eq!(index.total_bytes, 0);
    assert_eq!(index.limits.max_entries, 100);
    assert_eq!(index.limits.max_bytes_per_entry, 1024);
    assert_eq!(index.limits.max_bytes_total, 64 * 1024);
}

#[test]
fn remember_redacts_before_persistence_and_user_entries_have_no_links() {
    let temp = TempDir::new("redact");
    let dir = user_memory_dir(temp.path());
    let response = remember(&dir, "token sk-secretvalue123456789");
    let value = response_json(&response);
    assert_eq!(value["ok"], true);
    assert_eq!(value["entry"]["redactionCount"], 1);
    assert!(value["entry"].get("links").is_none());

    let raw = fs::read_to_string(dir.join("entries.jsonl")).expect("stored JSONL");
    assert!(!raw.contains("sk-secretvalue123456789"));
    assert!(raw.contains("[REDACTED:"));
    let stored: Value = serde_json::from_str(raw.lines().next().unwrap()).unwrap();
    assert!(stored.get("links").is_none());
}

#[test]
fn crud_survives_a_fresh_read_and_update_preserves_identity() {
    let temp = TempDir::new("crud");
    let dir = user_memory_dir(temp.path());
    let remembered = remember(&dir, "first wording");
    let id = remembered_id(&remembered);
    let created_ms = response_json(&remembered)["entry"]["createdMs"].clone();

    let updated = response_json(&update(&dir, &id, "better wording"));
    assert_eq!(updated["ok"], true);
    assert_eq!(updated["entry"]["id"], id);
    assert_eq!(updated["entry"]["createdMs"], created_ms);
    assert_eq!(updated["entry"]["text"], "better wording");

    let reloaded = read_index(&dir).expect("relaunch read");
    assert_eq!(reloaded.entries.len(), 1);
    assert_eq!(reloaded.entries[0].text, "better wording");

    assert_eq!(
        response_json(&forget(&dir, &id)),
        json!({"ok": true, "removed": true})
    );
    assert!(read_index(&dir)
        .expect("empty after forget")
        .entries
        .is_empty());
    assert_eq!(
        response_json(&forget(&dir, &id)),
        json!({"ok": true, "removed": false})
    );
}

#[test]
fn malformed_and_missing_ids_are_distinguished() {
    let temp = TempDir::new("ids");
    let dir = user_memory_dir(temp.path());
    assert_eq!(
        response_json(&forget(&dir, "../../oops"))["reason"],
        "badId"
    );
    assert_eq!(
        response_json(&update(&dir, "m_00000000000000000000000000000000", "x"))["reason"],
        "notFound"
    );
}

#[test]
fn search_is_capped_ranked_and_read_only() {
    let temp = TempDir::new("search");
    let dir = user_memory_dir(temp.path());
    remember(&dir, "a longer Alpha note");
    remember(&dir, "alpha");
    let before = fs::read(dir.join("entries.jsonl")).unwrap();

    let value = response_json(&search(&dir, "ALPHA", 1));
    assert_eq!(value["ok"], true);
    assert_eq!(value["query"], "ALPHA");
    assert_eq!(value["truncated"], true);
    assert_eq!(value["hits"][0]["entry"]["text"], "alpha");
    assert_eq!(value["hits"][0]["matchCount"], 1);
    assert_eq!(fs::read(dir.join("entries.jsonl")).unwrap(), before);
}

#[test]
fn write_and_search_caps_fail_in_band() {
    let temp = TempDir::new("caps");
    let dir = user_memory_dir(temp.path());
    assert_eq!(response_json(&remember(&dir, ""))["reason"], "empty");
    assert_eq!(
        response_json(&remember(&dir, &"x".repeat(1025)))["reason"],
        "tooLong"
    );
    assert_eq!(response_json(&search(&dir, "", 1))["reason"], "emptyQuery");
    assert_eq!(response_json(&search(&dir, "x", 0))["reason"], "badLimit");
    assert_eq!(
        response_json(&search(&dir, &"x".repeat(257), 1))["reason"],
        "queryTooLong"
    );
}

#[test]
fn externally_oversized_store_fails_closed() {
    let temp = TempDir::new("total-cap");
    let dir = user_memory_dir(temp.path());
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("entries.jsonl"),
        vec![b'x'; MAX_BYTES_TOTAL as usize + 1],
    )
    .unwrap();
    assert!(read_index(&dir).is_err());
    assert_eq!(
        response_json(&remember(&dir, "must not overwrite"))["reason"],
        "storeFailed"
    );
}

#[test]
fn entry_count_cap_is_enforced_without_overwriting_existing_entries() {
    let temp = TempDir::new("entry-cap");
    let dir = user_memory_dir(temp.path());
    for index in 0..MAX_ENTRIES {
        assert_eq!(
            response_json(&remember(&dir, &format!("fact {index}")))["ok"],
            true
        );
    }
    let before = fs::read(dir.join("entries.jsonl")).unwrap();
    assert_eq!(
        response_json(&remember(&dir, "overflow"))["reason"],
        "capacityReached"
    );
    assert_eq!(fs::read(dir.join("entries.jsonl")).unwrap(), before);
}

#[cfg(unix)]
#[test]
fn symlinked_user_memory_directory_is_refused_and_outside_stays_untouched() {
    use std::os::unix::fs::symlink;

    let app_data = TempDir::new("symlink-app-data");
    let outside = TempDir::new("symlink-outside");
    symlink(outside.path(), user_memory_dir(app_data.path())).expect("plant symlink");
    assert_eq!(
        response_json(&remember(&user_memory_dir(app_data.path()), "secret"))["reason"],
        "storeFailed"
    );
    assert!(!outside.path().join("entries.jsonl").exists());
}

#[cfg(unix)]
#[test]
fn symlinked_or_hardlinked_entries_file_is_refused() {
    use std::os::unix::fs::symlink;

    let temp = TempDir::new("entry-aliases");
    let outside = TempDir::new("entry-alias-target");
    let dir = user_memory_dir(temp.path());
    fs::create_dir_all(&dir).unwrap();
    let target = outside.path().join("target.jsonl");
    fs::write(&target, "").unwrap();
    symlink(&target, dir.join("entries.jsonl")).unwrap();
    assert!(read_index(&dir).is_err());
    fs::remove_file(dir.join("entries.jsonl")).unwrap();
    fs::hard_link(&target, dir.join("entries.jsonl")).unwrap();
    assert!(read_index(&dir).is_err());
    assert_eq!(fs::read_to_string(&target).unwrap(), "");
}

#[test]
fn app_private_store_never_creates_or_reads_a_project_plume_store() {
    let temp = TempDir::new("separation");
    let app_data = temp.path().join("app-data");
    let project = temp.path().join("project");
    fs::create_dir_all(&project).unwrap();
    let dir = user_memory_dir(&app_data);
    assert_eq!(
        response_json(&remember(&dir, "global preference"))["ok"],
        true
    );
    assert!(dir.join("entries.jsonl").is_file());
    assert!(!project.join(".plume").exists());
}
