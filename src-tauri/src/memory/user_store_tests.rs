use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};

use super::user_store::{forget, read_index, remember, search, update, user_memory_dir};
#[cfg(unix)]
use super::user_store_lock::acquire_user_memory_process_lock;
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

fn persisted_entry(index: usize, text: &str, redaction_count: u32) -> Value {
    json!({
        "id": format!("m_{index:032x}"),
        "createdMs": index as u64 + 1,
        "text": text,
        "redactionCount": redaction_count
    })
}

fn write_persisted_entries(dir: &Path, entries: &[Value]) -> Vec<u8> {
    fs::create_dir_all(dir).unwrap();
    let mut bytes = Vec::new();
    for entry in entries {
        bytes.extend(serde_json::to_vec(entry).unwrap());
        bytes.push(b'\n');
    }
    fs::write(dir.join("entries.jsonl"), &bytes).unwrap();
    bytes
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
    assert_eq!(
        read_index(&dir)
            .expect("redacted store reloads")
            .entries
            .len(),
        1
    );
}

#[test]
fn a_legacy_entry_without_a_revision_reads_as_zero_and_an_update_bumps_it() {
    // The app-private store is fail-closed on read: a malformed line is an
    // error, not a skipped row. A missing `revision` must therefore be a
    // legitimate absence rather than a parse failure, or the first launch
    // after this field lands cannot read the user's own memory at all.
    let temp = TempDir::new("legacy-revision");
    let dir = user_memory_dir(temp.path());
    write_persisted_entries(&dir, &[persisted_entry(0, "written before revisions", 0)]);

    let index = read_index(&dir).expect("a legacy store still reads");
    assert_eq!(index.entries.len(), 1);
    assert_eq!(index.entries[0].revision, 0);

    let id = index.entries[0].id.clone();
    let updated = response_json(&update(&dir, &id, "rewritten once"));
    assert_eq!(updated["ok"], true);
    assert_eq!(updated["entry"]["revision"], 1);

    let reloaded = read_index(&dir).expect("relaunch read");
    assert_eq!(
        reloaded.entries[0].revision, 1,
        "the bump is durable, not response-only",
    );
}

#[test]
fn forgetting_from_a_legacy_store_never_makes_the_file_bigger() {
    // Forget is the only rewrite with no cap check, and it must stay that way:
    // refusing to forget would remove the one way back under a cap. That makes
    // "a forget never grows the file" a real invariant rather than a nicety.
    // Stamping `"revision":0` onto every surviving line breaks it — the field
    // costs more across the remaining entries than the removed one frees — and
    // on a store already near the 64 KiB ceiling the rewrite produces a file
    // that the next fail-closed read rejects outright.
    let temp = TempDir::new("legacy-forget-growth");
    let dir = user_memory_dir(temp.path());
    let entries: Vec<Value> = (0..MAX_ENTRIES)
        .map(|index| persisted_entry(index, "a fact from before revisions", 0))
        .collect();
    let before = write_persisted_entries(&dir, &entries).len();

    let id = format!("m_{:032x}", 0);
    let forgotten = response_json(&forget(&dir, &id));
    assert_eq!(forgotten["ok"], true);
    assert_eq!(forgotten["removed"], true);

    let after = fs::read(dir.join("entries.jsonl"))
        .expect("store still on disk")
        .len();
    assert!(
        after < before,
        "forgetting one of {MAX_ENTRIES} entries grew the store from {before} to {after} bytes",
    );
    read_index(&dir).expect("the store the user just trimmed still reads");
}

#[test]
fn an_entry_at_the_revision_ceiling_refuses_the_update_instead_of_repeating_itself() {
    // Saturating at u32::MAX is the one place the counter stops telling the
    // truth: the text changes and the revision does not, so a compaction
    // checkpoint fact pinned to that revision keeps looking current after the
    // user replaced what it quotes. Refusing the write fails closed — the fact
    // stays valid because the text it quotes is still there.
    let temp = TempDir::new("revision-ceiling");
    let dir = user_memory_dir(temp.path());
    let mut entry = persisted_entry(0, "as revised as an entry can get", 0);
    entry["revision"] = json!(u32::MAX);
    write_persisted_entries(&dir, &[entry]);

    let id = format!("m_{:032x}", 0);
    let refused = response_json(&update(&dir, &id, "one rewrite too many"));
    assert_eq!(refused["ok"], false);

    let reloaded = read_index(&dir).expect("the store is untouched");
    assert_eq!(reloaded.entries[0].text, "as revised as an entry can get");
    assert_eq!(reloaded.entries[0].revision, u32::MAX);
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
    let original = vec![b'x'; MAX_BYTES_TOTAL as usize * 16];
    fs::write(dir.join("entries.jsonl"), &original).unwrap();
    assert!(read_index(&dir).is_err());
    assert_eq!(
        response_json(&remember(&dir, "must not overwrite"))["reason"],
        "storeFailed"
    );
    assert_eq!(fs::read(dir.join("entries.jsonl")).unwrap(), original);
}

#[test]
fn malformed_jsonl_fails_closed_and_every_mutation_preserves_exact_bytes() {
    let temp = TempDir::new("malformed-jsonl");
    let dir = user_memory_dir(temp.path());
    let id = "m_00000000000000000000000000000001";
    let mut original = write_persisted_entries(&dir, &[persisted_entry(1, "valid", 0)]);
    original.extend_from_slice(b"{not-json}\n");
    fs::write(dir.join("entries.jsonl"), &original).unwrap();

    assert!(read_index(&dir).is_err());
    assert_eq!(
        response_json(&search(&dir, "valid", 5))["reason"],
        "storeFailed"
    );
    assert_eq!(
        response_json(&remember(&dir, "new"))["reason"],
        "storeFailed"
    );
    assert_eq!(
        response_json(&update(&dir, id, "changed"))["reason"],
        "storeFailed"
    );
    assert_eq!(response_json(&forget(&dir, id))["reason"], "storeFailed");
    assert_eq!(fs::read(dir.join("entries.jsonl")).unwrap(), original);
}

#[test]
fn persisted_entries_revalidate_every_hard_invariant_on_relaunch() {
    let temp = TempDir::new("persisted-invariants");
    let dir = user_memory_dir(temp.path());
    let raw_secret = format!("sk-{}", "a".repeat(20));
    let mut unknown_field = persisted_entry(7, "valid", 0);
    unknown_field["links"] = json!([]);
    let mut wrong_type = persisted_entry(8, "valid", 0);
    wrong_type["createdMs"] = json!("now");

    let invalid_cases = vec![
        vec![json!({"id":"bad", "createdMs":1, "text":"valid", "redactionCount":0})],
        vec![persisted_entry(1, "   ", 0)],
        vec![persisted_entry(2, &"x".repeat(1025), 0)],
        vec![persisted_entry(3, &raw_secret, 0)],
        vec![persisted_entry(4, "valid", 1)],
        vec![unknown_field],
        vec![wrong_type],
    ];

    for entries in invalid_cases {
        let original = write_persisted_entries(&dir, &entries);
        assert!(
            read_index(&dir).is_err(),
            "invalid store must fail closed: {entries:?}"
        );
        assert_eq!(
            response_json(&remember(&dir, "must not rewrite"))["reason"],
            "storeFailed"
        );
        assert_eq!(fs::read(dir.join("entries.jsonl")).unwrap(), original);
    }

    let over_cap = (0..=MAX_ENTRIES)
        .map(|index| persisted_entry(index, "valid", 0))
        .collect::<Vec<_>>();
    let original = write_persisted_entries(&dir, &over_cap);
    assert!(read_index(&dir).is_err());
    assert_eq!(
        response_json(&remember(&dir, "must not rewrite"))["reason"],
        "storeFailed"
    );
    assert_eq!(fs::read(dir.join("entries.jsonl")).unwrap(), original);
}

#[test]
fn duplicate_persisted_ids_block_update_and_forget_without_touching_either_row() {
    let temp = TempDir::new("duplicate-ids");
    let dir = user_memory_dir(temp.path());
    let duplicate = "m_00000000000000000000000000000001";
    let original = write_persisted_entries(
        &dir,
        &[
            persisted_entry(1, "first", 0),
            json!({"id": duplicate, "createdMs":2, "text":"second", "redactionCount":0}),
        ],
    );

    assert!(read_index(&dir).is_err());
    assert_eq!(
        response_json(&update(&dir, duplicate, "changed"))["reason"],
        "storeFailed"
    );
    assert_eq!(
        response_json(&forget(&dir, duplicate))["reason"],
        "storeFailed"
    );
    assert_eq!(fs::read(dir.join("entries.jsonl")).unwrap(), original);
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

#[cfg(unix)]
#[test]
fn process_lock_serializes_independent_file_descriptors() {
    use std::os::unix::fs::PermissionsExt;
    use std::sync::mpsc;
    use std::time::Duration;

    let temp = TempDir::new("process-lock");
    let dir = user_memory_dir(temp.path());
    let held = acquire_user_memory_process_lock(&dir).unwrap();
    assert_eq!(
        fs::metadata(dir.join(".process.lock"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    let contender_dir = dir.clone();
    let (tx, rx) = mpsc::channel();
    let contender = std::thread::spawn(move || {
        let acquired = acquire_user_memory_process_lock(&contender_dir).unwrap();
        tx.send(()).unwrap();
        drop(acquired);
    });

    assert!(rx.recv_timeout(Duration::from_millis(100)).is_err());
    drop(held);
    rx.recv_timeout(Duration::from_secs(2))
        .expect("contender acquires only after the first process lock drops");
    contender.join().unwrap();
}

#[cfg(unix)]
#[test]
fn preexisting_permissive_process_lock_is_tightened_to_owner_only() {
    use std::os::unix::fs::PermissionsExt;

    let temp = TempDir::new("process-lock-mode");
    let dir = user_memory_dir(temp.path());
    fs::create_dir_all(&dir).unwrap();
    let lock = dir.join(".process.lock");
    fs::write(&lock, b"").unwrap();
    fs::set_permissions(&lock, fs::Permissions::from_mode(0o666)).unwrap();

    let held = acquire_user_memory_process_lock(&dir).unwrap();
    assert_eq!(
        fs::metadata(&lock).unwrap().permissions().mode() & 0o777,
        0o600
    );
    drop(held);
}

#[cfg(unix)]
#[test]
fn symlinked_or_hardlinked_process_lock_is_refused() {
    use std::os::unix::fs::symlink;

    let temp = TempDir::new("process-lock-alias");
    let outside = TempDir::new("process-lock-target");
    let dir = user_memory_dir(temp.path());
    fs::create_dir_all(&dir).unwrap();
    let target = outside.path().join("target.lock");
    fs::write(&target, b"outside").unwrap();
    let lock = dir.join(".process.lock");

    symlink(&target, &lock).unwrap();
    assert!(acquire_user_memory_process_lock(&dir).is_err());
    fs::remove_file(&lock).unwrap();
    fs::hard_link(&target, &lock).unwrap();
    assert!(acquire_user_memory_process_lock(&dir).is_err());
    assert_eq!(fs::read(&target).unwrap(), b"outside");
}

#[cfg(unix)]
#[test]
fn user_memory_directory_and_store_are_tightened_to_owner_only() {
    use std::os::unix::fs::PermissionsExt;

    let temp = TempDir::new("owner-only-modes");
    let dir = user_memory_dir(temp.path());
    fs::create_dir_all(&dir).unwrap();
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o777)).unwrap();
    let entries = dir.join("entries.jsonl");
    let original = write_persisted_entries(&dir, &[persisted_entry(1, "private", 0)]);
    fs::set_permissions(&entries, fs::Permissions::from_mode(0o666)).unwrap();

    let index = read_index(&dir).expect("legacy permissive store is tightened");

    assert_eq!(index.entries.len(), 1);
    assert_eq!(fs::read(&entries).unwrap(), original);
    assert_eq!(
        fs::metadata(&dir).unwrap().permissions().mode() & 0o777,
        0o700
    );
    assert_eq!(
        fs::metadata(&entries).unwrap().permissions().mode() & 0o777,
        0o600
    );
}

#[cfg(unix)]
#[test]
fn interrupted_legacy_temp_symlink_does_not_block_rewrite_or_touch_any_inode() {
    use std::os::unix::fs::symlink;

    let temp = TempDir::new("temp-symlink");
    let outside = TempDir::new("temp-symlink-outside");
    let dir = user_memory_dir(temp.path());
    let remembered = remember(&dir, "before");
    let id = remembered_id(&remembered);
    let entries = dir.join("entries.jsonl");
    let before = fs::read(&entries).unwrap();
    let outside_file = outside.path().join("outside.txt");
    fs::write(&outside_file, b"outside").unwrap();
    let planted = dir.join(".entries.jsonl.plume-user-memory.tmp");
    symlink(&outside_file, &planted).unwrap();

    assert_eq!(response_json(&update(&dir, &id, "after"))["ok"], true);
    assert_ne!(fs::read(&entries).unwrap(), before);
    assert_eq!(fs::read(&outside_file).unwrap(), b"outside");
    assert!(fs::symlink_metadata(&planted)
        .expect("planted symlink remains for diagnosis")
        .file_type()
        .is_symlink());
}

#[cfg(unix)]
#[test]
fn interrupted_legacy_temp_hardlink_does_not_block_rewrite_or_touch_any_inode() {
    let temp = TempDir::new("temp-hardlink");
    let outside = TempDir::new("temp-hardlink-outside");
    let dir = user_memory_dir(temp.path());
    let remembered = remember(&dir, "before");
    let id = remembered_id(&remembered);
    let entries = dir.join("entries.jsonl");
    let before = fs::read(&entries).unwrap();
    let outside_file = outside.path().join("outside.txt");
    fs::write(&outside_file, b"outside").unwrap();
    let planted = dir.join(".entries.jsonl.plume-user-memory.tmp");
    fs::hard_link(&outside_file, &planted).unwrap();

    assert_eq!(response_json(&update(&dir, &id, "after"))["ok"], true);
    assert_ne!(fs::read(&entries).unwrap(), before);
    assert_eq!(fs::read(&outside_file).unwrap(), b"outside");
    assert!(planted.exists(), "pre-existing hardlink is not cleaned up");
}

#[cfg(unix)]
#[test]
fn interrupted_legacy_regular_temp_does_not_block_rewrite_or_get_overwritten() {
    let temp = TempDir::new("temp-collision");
    let dir = user_memory_dir(temp.path());
    let remembered = remember(&dir, "before");
    let id = remembered_id(&remembered);
    let entries = dir.join("entries.jsonl");
    let before = fs::read(&entries).unwrap();
    let planted = dir.join(".entries.jsonl.plume-user-memory.tmp");
    fs::write(&planted, b"collision").unwrap();

    assert_eq!(response_json(&update(&dir, &id, "after"))["ok"], true);
    assert_ne!(fs::read(&entries).unwrap(), before);
    assert_eq!(fs::read(&planted).unwrap(), b"collision");
}
