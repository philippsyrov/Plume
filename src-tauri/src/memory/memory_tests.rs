//! Tests for `memory`. Split into a sibling file via `#[path]` so
//! the production module stays under the decomposition cap.

use super::distill::{
    append_distill_log, normalize_for_distill, DuplicateGroup, MemoryDistillApplyFailure,
    MemoryDistillApplyOk, DISTILL_LOG_MAX_RECORDS,
};
use super::links::MemorySetLinksFailure;
use super::topics::{TopicKind, MAX_CORE_FILE_BYTES, MAX_TOPIC_FILES, MAX_TOPIC_FILE_BYTES};
use super::*;
use std::fs;
use std::path::PathBuf;

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(label: &str) -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock before epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "plume-memory-test-{}-{}-{}",
            label,
            std::process::id(),
            nanos
        ));
        fs::create_dir_all(&path).expect("create tempdir");
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

fn canon_root(td: &TempDir) -> PathBuf {
    fs::canonicalize(td.path()).expect("canonicalize tempdir")
}

fn unwrap_ok(resp: MemoryRememberResponse) -> MemoryRememberOk {
    match resp {
        MemoryRememberResponse::Ok(ok) => ok,
        MemoryRememberResponse::Err(e) => {
            panic!(
                "expected ok, got err: reason={:?} msg={:?}",
                e.reason, e.message
            )
        }
    }
}

#[test]
fn legacy_entry_without_links_migrates_to_empty_links() {
    let raw = r#"{"id":"m_00000000000000000000000000000000","createdMs":1,"text":"legacy","redactionCount":0}"#;
    let entry: MemoryEntry = serde_json::from_str(raw).expect("legacy entry");
    assert!(entry.links.is_empty());
    assert_eq!(
        serde_json::to_value(entry).unwrap()["links"],
        serde_json::json!([])
    );
}

fn unwrap_links(resp: MemorySetLinksResponse) -> MemoryEntry {
    match resp {
        MemorySetLinksResponse::Ok(ok) => ok.entry,
        MemorySetLinksResponse::Err(err) => {
            panic!("set links failed: {:?}: {}", err.reason, err.message)
        }
    }
}

fn links_reason(resp: MemorySetLinksResponse) -> MemorySetLinksFailure {
    match resp {
        MemorySetLinksResponse::Err(err) => err.reason,
        MemorySetLinksResponse::Ok(_) => panic!("expected link failure"),
    }
}

#[test]
fn set_links_sets_replaces_clears_sorts_and_preserves_entry_order_and_fields() {
    let td = TempDir::new("links-happy");
    let root = canon_root(&td);
    write_memory_file(&root, "topics/zeta.md", "z");
    write_memory_file(&root, "topics/alpha.md", "a");
    let first = unwrap_ok(remember(&root, "first"));
    let second = unwrap_ok(remember(&root, "second"));

    let linked = unwrap_links(set_links(
        &root,
        &first.entry.id,
        &["topics/zeta.md".into(), "topics/alpha.md".into()],
    ));
    assert_eq!(linked.links, vec!["topics/alpha.md", "topics/zeta.md"]);
    assert_eq!(linked.id, first.entry.id);
    assert_eq!(linked.created_ms, first.entry.created_ms);
    assert_eq!(linked.text, first.entry.text);
    assert_eq!(linked.redaction_count, first.entry.redaction_count);

    let replaced = unwrap_links(set_links(
        &root,
        &first.entry.id,
        &["topics/zeta.md".into()],
    ));
    assert_eq!(replaced.links, vec!["topics/zeta.md"]);
    assert!(unwrap_links(set_links(&root, &first.entry.id, &[]))
        .links
        .is_empty());
    let index = read_index(&root).unwrap();
    assert_eq!(
        index
            .entries
            .iter()
            .map(|entry| entry.id.as_str())
            .collect::<Vec<_>>(),
        vec![first.entry.id, second.entry.id]
    );
}

#[test]
fn set_links_rejects_bad_id_missing_id_caps_duplicates_and_bad_paths() {
    let td = TempDir::new("links-invalid");
    let root = canon_root(&td);
    write_memory_file(&root, "topics/good.md", "good");
    let entry = unwrap_ok(remember(&root, "entry"));
    assert_eq!(
        links_reason(set_links(&root, "bad", &[])),
        MemorySetLinksFailure::BadId
    );
    assert_eq!(
        links_reason(set_links(&root, "m_00000000000000000000000000000000", &[])),
        MemorySetLinksFailure::NotFound
    );
    assert_eq!(
        links_reason(set_links(
            &root,
            &entry.entry.id,
            &vec!["topics/good.md".into(); 6]
        )),
        MemorySetLinksFailure::TooMany
    );
    assert_eq!(
        links_reason(set_links(
            &root,
            &entry.entry.id,
            &["topics/good.md".into(), "topics/good.md".into()]
        )),
        MemorySetLinksFailure::Duplicate
    );
    for bad in [
        "INDEX.md",
        "topics/../USER.md",
        "topics/.hidden.md",
        "topics/nested/deep.md",
        "topics/file.txt",
        "topics\\evil.md",
    ] {
        assert_eq!(
            links_reason(set_links(&root, &entry.entry.id, &[bad.into()])),
            MemorySetLinksFailure::InvalidTopic,
            "{bad}"
        );
    }
    assert_eq!(
        links_reason(set_links(
            &root,
            &entry.entry.id,
            &["topics/missing.md".into()]
        )),
        MemorySetLinksFailure::TopicNotFound
    );
}

#[test]
fn set_links_rejects_unsafe_or_oversize_topic_files() {
    let td = TempDir::new("links-files");
    let root = canon_root(&td);
    let entry = unwrap_ok(remember(&root, "entry"));
    write_memory_file(
        &root,
        "topics/large.md",
        &"x".repeat(MAX_TOPIC_FILE_BYTES + 1),
    );
    assert_eq!(
        links_reason(set_links(
            &root,
            &entry.entry.id,
            &["topics/large.md".into()]
        )),
        MemorySetLinksFailure::InvalidTopic
    );
    #[cfg(unix)]
    {
        write_memory_file(&root, "topics/real.md", "real");
        let dir = root.join(".plume/memory/topics");
        std::fs::hard_link(dir.join("real.md"), dir.join("hard.md")).unwrap();
        assert_eq!(
            links_reason(set_links(
                &root,
                &entry.entry.id,
                &["topics/hard.md".into()]
            )),
            MemorySetLinksFailure::InvalidTopic
        );
        let outside = root.join("outside.md");
        fs::write(&outside, "outside").unwrap();
        std::os::unix::fs::symlink(&outside, dir.join("link.md")).unwrap();
        assert_eq!(
            links_reason(set_links(
                &root,
                &entry.entry.id,
                &["topics/link.md".into()]
            )),
            MemorySetLinksFailure::InvalidTopic
        );
    }
}

#[test]
fn stale_links_remain_visible_and_prompt_budget_ignores_link_bytes() {
    let td = TempDir::new("links-stale");
    let root = canon_root(&td);
    write_memory_file(&root, "topics/keep.md", "topic");
    let entry = unwrap_ok(remember(&root, "four"));
    unwrap_links(set_links(
        &root,
        &entry.entry.id,
        &["topics/keep.md".into()],
    ));
    fs::remove_file(root.join(".plume/memory/topics/keep.md")).unwrap();
    let index = read_index(&root).unwrap();
    assert_eq!(index.entries[0].links, vec!["topics/keep.md"]);
    let prompt = read_for_prompt(&root, 4).unwrap();
    assert_eq!(prompt.used_bytes, 4);
    assert_eq!(prompt.entries.len(), 1);
    assert!(unwrap_links(set_links(&root, &entry.entry.id, &[]))
        .links
        .is_empty());
}

#[test]
fn concurrent_set_links_updates_different_entries_without_lost_update() {
    let td = TempDir::new("links-concurrent");
    let root = canon_root(&td);
    write_memory_file(&root, "topics/a.md", "a");
    write_memory_file(&root, "topics/b.md", "b");
    let a = unwrap_ok(remember(&root, "a"));
    let b = unwrap_ok(remember(&root, "b"));
    let root_a = root.clone();
    let root_b = root.clone();
    let a_id = a.entry.id.clone();
    let b_id = b.entry.id.clone();
    let first = std::thread::spawn(move || set_links(&root_a, &a_id, &["topics/a.md".into()]));
    let second = std::thread::spawn(move || set_links(&root_b, &b_id, &["topics/b.md".into()]));
    unwrap_links(first.join().unwrap());
    unwrap_links(second.join().unwrap());
    let index = read_index(&root).unwrap();
    assert_eq!(index.entries[0].links, vec!["topics/a.md"]);
    assert_eq!(index.entries[1].links, vec!["topics/b.md"]);
}

#[test]
#[cfg(unix)]
fn set_links_reports_store_failed_without_changing_existing_entry() {
    use std::os::unix::fs::PermissionsExt;

    let td = TempDir::new("links-write-fail");
    let root = canon_root(&td);
    write_memory_file(&root, "topics/a.md", "a");
    let entry = unwrap_ok(remember(&root, "entry"));
    let memory_dir = root.join(".plume/memory");
    let original_mode = fs::metadata(&memory_dir).unwrap().permissions().mode();
    fs::set_permissions(&memory_dir, fs::Permissions::from_mode(0o555)).unwrap();
    let response = set_links(&root, &entry.entry.id, &["topics/a.md".into()]);
    fs::set_permissions(&memory_dir, fs::Permissions::from_mode(original_mode)).unwrap();
    assert_eq!(links_reason(response), MemorySetLinksFailure::StoreFailed);
    assert!(read_index(&root).unwrap().entries[0].links.is_empty());
}

#[test]
fn set_links_enforces_total_jsonl_capacity_before_rewrite() {
    let td = TempDir::new("links-capacity");
    let root = canon_root(&td);
    let memory_dir = root.join(".plume/memory");
    fs::create_dir_all(memory_dir.join("topics")).unwrap();
    let id = "m_00000000000000000000000000000001";
    let legacy_large = MemoryEntry {
        id: id.to_string(),
        created_ms: 1,
        text: "x".repeat(MAX_BYTES_TOTAL as usize - 800),
        redaction_count: 0,
        links: Vec::new(),
    };
    fs::write(
        memory_dir.join("entries.jsonl"),
        format!("{}\n", serde_json::to_string(&legacy_large).unwrap()),
    )
    .unwrap();
    let links: Vec<String> = (0..5)
        .map(|index| format!("topics/{index}-{}.md", "a".repeat(210)))
        .collect();
    for link in &links {
        fs::write(memory_dir.join(link), "topic").unwrap();
    }
    assert_eq!(
        links_reason(set_links(&root, id, &links)),
        MemorySetLinksFailure::CapacityReached
    );
    assert!(read_index(&root).unwrap().entries[0].links.is_empty());
}

// ─── Happy paths ────────────────────────────────────────────────────────────

#[test]
fn empty_index_when_no_file() {
    let td = TempDir::new("empty");
    let root = canon_root(&td);
    let index = read_index(&root).unwrap();
    assert!(index.entries.is_empty());
    assert_eq!(index.total_bytes, 0);
    assert_eq!(index.limits.max_entries, MAX_ENTRIES as u32);
    assert_eq!(index.limits.max_bytes_per_entry, MAX_BYTES_PER_ENTRY as u32);
    assert_eq!(index.limits.max_bytes_total, MAX_BYTES_TOTAL as u32);
}

#[test]
fn remember_then_index_returns_entry() {
    let td = TempDir::new("remember");
    let root = canon_root(&td);

    let ok = unwrap_ok(remember(&root, "Test command is `cargo test`."));
    assert!(ok.ok);
    assert_eq!(ok.entry.text, "Test command is `cargo test`.");
    assert_eq!(ok.entry.redaction_count, 0);
    assert!(ok.entry.id.starts_with("m_"));
    assert_eq!(ok.entry.id.len(), 34);

    let index = read_index(&root).unwrap();
    assert_eq!(index.entries.len(), 1);
    assert_eq!(index.entries[0].id, ok.entry.id);
    assert_eq!(index.entries[0].text, "Test command is `cargo test`.");
    assert!(index.total_bytes > 0);
}

#[test]
fn remember_three_then_forget_middle() {
    let td = TempDir::new("forget-middle");
    let root = canon_root(&td);

    let a = unwrap_ok(remember(&root, "first memory"));
    let b = unwrap_ok(remember(&root, "second memory"));
    let c = unwrap_ok(remember(&root, "third memory"));

    let resp = forget(&root, &b.entry.id);
    match resp {
        MemoryForgetResponse::Ok(ok) => {
            assert!(ok.ok);
            assert!(ok.removed);
        }
        MemoryForgetResponse::Err(e) => panic!("expected ok, got {:?}", e.reason),
    }

    let index = read_index(&root).unwrap();
    assert_eq!(index.entries.len(), 2);
    let ids: Vec<&str> = index.entries.iter().map(|e| e.id.as_str()).collect();
    assert!(ids.contains(&a.entry.id.as_str()));
    assert!(ids.contains(&c.entry.id.as_str()));
    assert!(!ids.contains(&b.entry.id.as_str()));
}

#[test]
fn forget_nonexistent_id_is_idempotent_noop() {
    let td = TempDir::new("forget-noop");
    let root = canon_root(&td);

    // Forget on an empty store with a well-shaped id.
    let resp = forget(&root, "m_00000000000000000000000000000000");
    match resp {
        MemoryForgetResponse::Ok(ok) => {
            assert!(ok.ok);
            assert!(!ok.removed, "no entry should have been removed");
        }
        MemoryForgetResponse::Err(e) => panic!("expected ok, got {:?}", e.reason),
    }
}

#[test]
fn forget_last_entry_removes_file() {
    let td = TempDir::new("forget-last");
    let root = canon_root(&td);

    let a = unwrap_ok(remember(&root, "only memory"));
    let path = root.join(".plume").join("memory").join("entries.jsonl");
    assert!(path.exists(), "entries file should exist after remember");

    match forget(&root, &a.entry.id) {
        MemoryForgetResponse::Ok(ok) => assert!(ok.removed),
        MemoryForgetResponse::Err(e) => panic!("expected ok, got {:?}", e.reason),
    }

    // File is removed when the last entry leaves.
    assert!(!path.exists(), "entries file should be removed when empty");
    let index = read_index(&root).unwrap();
    assert!(index.entries.is_empty());
    assert_eq!(index.total_bytes, 0);
}

// ─── Validation: empty / too long / bad id ─────────────────────────────────

#[test]
fn remember_rejects_empty_text() {
    let td = TempDir::new("empty-text");
    let root = canon_root(&td);

    match remember(&root, "") {
        MemoryRememberResponse::Err(e) => {
            assert_eq!(e.reason, MemoryRememberFailure::Empty);
        }
        MemoryRememberResponse::Ok(_) => panic!("expected rejection"),
    }
    match remember(&root, "   \n\t  ") {
        MemoryRememberResponse::Err(e) => {
            assert_eq!(e.reason, MemoryRememberFailure::Empty);
        }
        MemoryRememberResponse::Ok(_) => panic!("expected rejection"),
    }
}

#[test]
fn remember_rejects_over_per_entry_cap() {
    let td = TempDir::new("too-long");
    let root = canon_root(&td);

    let oversize = "x".repeat(MAX_BYTES_PER_ENTRY + 1);
    match remember(&root, &oversize) {
        MemoryRememberResponse::Err(e) => {
            assert_eq!(e.reason, MemoryRememberFailure::TooLong);
        }
        MemoryRememberResponse::Ok(_) => panic!("expected rejection"),
    }
}

#[test]
fn forget_rejects_bad_id_shape() {
    let td = TempDir::new("bad-id");
    let root = canon_root(&td);

    // Various malformed ids — none should reach the store.
    let cases = [
        "",
        "abc",
        "m_short",
        "m_../escape",
        "m_../../../etc/passwd",
        "m_ZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZ",
        "x_00000000000000000000000000000000",
    ];
    for id in cases {
        match forget(&root, id) {
            MemoryForgetResponse::Err(e) => {
                assert_eq!(
                    e.reason,
                    MemoryForgetFailure::BadId,
                    "expected BadId for {id:?}, got {:?}",
                    e.reason
                );
            }
            MemoryForgetResponse::Ok(_) => {
                panic!("forget should have rejected bad id {id:?}")
            }
        }
    }
}

// ─── Secret redaction ──────────────────────────────────────────────────────

#[test]
fn remember_redacts_secret_patterns_in_text() {
    let td = TempDir::new("redact");
    let root = canon_root(&td);

    // OpenAI-style key inside the memory text. The redactor
    // catches `sk-` + ≥ 20 chars.
    let raw = "API key for staging is sk-abcdefghij0123456789xyzABCDEFGH";
    let ok = unwrap_ok(remember(&root, raw));
    assert!(!ok.entry.text.contains("sk-abcdefg"));
    assert!(ok.entry.text.contains("[REDACTED:api-key]"));
    assert_eq!(ok.entry.redaction_count, 1);

    // The original raw text never reaches disk — read it back to
    // be sure.
    let on_disk =
        fs::read_to_string(root.join(".plume").join("memory").join("entries.jsonl")).unwrap();
    assert!(!on_disk.contains("sk-abcdefg"));
    assert!(on_disk.contains("[REDACTED:api-key]"));
}

// `MemoryRememberFailure::RedactedToEmpty` is intentionally not
// unit-tested. The redactor replaces every matched secret with
// `[REDACTED:<kind>]`, so even an input that's nothing but secret
// patterns leaves the marker text — `trim().is_empty()` returns
// false. The variant is defensive: if a future slice swaps the
// redactor for one that strips matches instead of marking them,
// this check is the safety net that stops empty entries from
// landing on disk. Keep it; no current path triggers it.

// ─── Cap behaviour ─────────────────────────────────────────────────────────

#[test]
fn remember_rejects_when_entry_count_cap_reached() {
    let td = TempDir::new("cap-count");
    let root = canon_root(&td);

    // Pre-populate the file with `MAX_ENTRIES` valid entries.
    let memory_dir = root.join(".plume").join("memory");
    fs::create_dir_all(&memory_dir).unwrap();
    let mut serialized = String::new();
    for i in 0..MAX_ENTRIES {
        let entry = MemoryEntry {
            id: format!("m_{:032x}", i as u128),
            created_ms: 1_700_000_000_000 + i as u64,
            text: format!("prefilled #{i}"),
            redaction_count: 0,
            links: Vec::new(),
        };
        serialized.push_str(&serde_json::to_string(&entry).unwrap());
        serialized.push('\n');
    }
    fs::write(memory_dir.join("entries.jsonl"), &serialized).unwrap();

    match remember(&root, "one too many") {
        MemoryRememberResponse::Err(e) => {
            assert_eq!(e.reason, MemoryRememberFailure::CapacityReached);
        }
        MemoryRememberResponse::Ok(_) => panic!("expected capacity rejection"),
    }
    // No entry was appended.
    let index = read_index(&root).unwrap();
    assert_eq!(index.entries.len(), MAX_ENTRIES);
}

#[test]
fn read_index_rejects_oversize_store_file() {
    let td = TempDir::new("oversize");
    let root = canon_root(&td);

    let memory_dir = root.join(".plume").join("memory");
    fs::create_dir_all(&memory_dir).unwrap();
    // 128 KiB file — well past `MAX_BYTES_TOTAL` of 64 KiB.
    let blob = "x".repeat((MAX_BYTES_TOTAL as usize) * 2);
    fs::write(memory_dir.join("entries.jsonl"), blob).unwrap();

    let err = read_index(&root).unwrap_err();
    assert!(
        err.0.contains("max is"),
        "oversize message should explain the cap: {:?}",
        err.0
    );
}

// ─── Symlink defense ───────────────────────────────────────────────────────

/// `.plume/` symlinked outside the project must be refused — the
/// same guard `patch::checkpoint` enforces, mirrored locally for
/// memory writes.
#[cfg(unix)]
#[test]
fn remember_rejects_when_plume_dir_is_symlink() {
    use std::os::unix::fs::symlink;

    let td = TempDir::new("symlink");
    let root = canon_root(&td);
    let outside = TempDir::new("symlink-target");
    symlink(outside.path(), root.join(".plume")).unwrap();

    match remember(&root, "hello") {
        MemoryRememberResponse::Err(e) => {
            assert_eq!(e.reason, MemoryRememberFailure::StoreFailed);
            assert!(
                e.message.contains("symlink"),
                "expected symlink rejection message: {:?}",
                e.message
            );
        }
        MemoryRememberResponse::Ok(_) => panic!("expected symlink rejection"),
    }
    // Nothing was written through the symlink.
    assert!(
        fs::read_dir(outside.path()).map(|d| d.count()).unwrap_or(0) == 0,
        "outside dir must stay empty"
    );
}

/// Codex D37 HIGH regression: `read_index` and `forget` must refuse
/// a symlinked `.plume/` the same way `remember` does. Pre-fix they
/// went through a raw `entries_path()` join that dereferenced the
/// symlink — `forget`'s `remove_file` / atomic-rename could have
/// touched a file outside the project.
#[cfg(unix)]
#[test]
fn read_index_and_forget_refuse_symlinked_plume_dir() {
    use std::os::unix::fs::symlink;

    let td = TempDir::new("read-forget-symlink");
    let root = canon_root(&td);
    let outside = TempDir::new("read-forget-symlink-target");

    // Plant a sentinel inside the outside dir so we can detect any
    // accidental rewrite through the symlink.
    let sentinel = outside.path().join("entries.jsonl");
    fs::write(&sentinel, "sentinel\n").unwrap();

    symlink(outside.path(), root.join(".plume")).unwrap();

    // `read_index` rejects.
    let err = read_index(&root).unwrap_err();
    assert!(
        err.0.contains("symlink"),
        "read_index should reject symlinked .plume: {:?}",
        err.0
    );

    // `forget` rejects with `StoreFailed` (well-shaped id so we
    // don't fall into the BadId branch first).
    match forget(&root, "m_00000000000000000000000000000000") {
        MemoryForgetResponse::Err(e) => {
            assert_eq!(e.reason, MemoryForgetFailure::StoreFailed);
            assert!(
                e.message.contains("symlink"),
                "forget should surface symlink rejection: {:?}",
                e.message
            );
        }
        MemoryForgetResponse::Ok(_) => {
            panic!("forget should have refused the symlinked .plume")
        }
    }

    // Sentinel outside the project untouched — defense in depth
    // that no remove or rename leaked through the symlink.
    assert_eq!(
        fs::read_to_string(&sentinel).unwrap(),
        "sentinel\n",
        "outside sentinel must be intact"
    );
}

// ─── Wire shape ────────────────────────────────────────────────────────────

#[test]
fn memory_entry_round_trips_through_serde() {
    let entry = MemoryEntry {
        id: "m_00000000000000000000000000000000".to_string(),
        created_ms: 1_700_000_000_000,
        text: "hello".to_string(),
        redaction_count: 2,
        links: Vec::new(),
    };
    let json = serde_json::to_string(&entry).unwrap();
    assert!(json.contains("\"id\":\"m_00000000000000000000000000000000\""));
    assert!(json.contains("\"createdMs\":1700000000000"));
    assert!(json.contains("\"redactionCount\":2"));
    let back: MemoryEntry = serde_json::from_str(&json).unwrap();
    assert_eq!(back, entry);
}

#[test]
fn entry_id_validator_accepts_minted_shape() {
    let id = mint_entry_id();
    assert!(is_valid_entry_id(&id), "minted id must validate: {id}");
    assert_eq!(id.len(), 34);
}

#[test]
fn entry_id_validator_rejects_path_like_ids() {
    assert!(!is_valid_entry_id(""));
    assert!(!is_valid_entry_id("m_"));
    assert!(!is_valid_entry_id("m_../escape"));
    assert!(!is_valid_entry_id("m_/etc/passwd"));
    assert!(!is_valid_entry_id("m_AAAA")); // too short
    assert!(!is_valid_entry_id("x_00000000000000000000000000000000")); // wrong prefix
                                                                       // Uppercase hex is intentionally accepted — `is_ascii_hexdigit`
                                                                       // permits both cases, and case-folding the id at the boundary
                                                                       // would be a footgun (panel does string-equality against the
                                                                       // stored id). The important property is that no `/`, `\\`,
                                                                       // `..`, or NUL slip through; assertions above cover that.
    assert!(is_valid_entry_id("m_AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"));
}

// --- D42: read_for_prompt -----------------------------------------------

#[test]
fn read_for_prompt_returns_empty_when_no_store_exists() {
    let td = TempDir::new("d42-no-store");
    let read = read_for_prompt(td.path(), 4096).expect("ok");
    assert!(read.entries.is_empty());
    assert_eq!(read.used_bytes, 0);
    assert_eq!(read.byte_cap, 4096);
    assert!(!read.truncated);
}

#[test]
fn read_for_prompt_returns_newest_entries_first() {
    let td = TempDir::new("d42-order");
    // `remember` stamps `created_ms` from the system clock, so to
    // pin ordering we write the JSONL directly.
    let memory_dir = td.path().join(".plume").join("memory");
    fs::create_dir_all(&memory_dir).unwrap();
    let jsonl = "{\"id\":\"m_a0000000000000000000000000000000\",\"createdMs\":100,\"text\":\"old\",\"redactionCount\":0}\n\
{\"id\":\"m_b0000000000000000000000000000000\",\"createdMs\":200,\"text\":\"mid\",\"redactionCount\":0}\n\
{\"id\":\"m_c0000000000000000000000000000000\",\"createdMs\":300,\"text\":\"new\",\"redactionCount\":0}\n";
    fs::write(memory_dir.join("entries.jsonl"), jsonl).unwrap();

    let read = read_for_prompt(td.path(), 4096).expect("ok");
    assert_eq!(read.entries.len(), 3);
    assert_eq!(read.entries[0].text, "new");
    assert_eq!(read.entries[1].text, "mid");
    assert_eq!(read.entries[2].text, "old");
    assert_eq!(read.used_bytes, 9);
    assert!(!read.truncated);
}

#[test]
fn read_for_prompt_drops_oldest_when_byte_cap_exceeded() {
    let td = TempDir::new("d42-cap");
    let memory_dir = td.path().join(".plume").join("memory");
    fs::create_dir_all(&memory_dir).unwrap();
    let big = "x".repeat(1000);
    let jsonl = format!(
        "{{\"id\":\"m_a0000000000000000000000000000000\",\"createdMs\":100,\"text\":{big:?},\"redactionCount\":0}}\n\
{{\"id\":\"m_b0000000000000000000000000000000\",\"createdMs\":200,\"text\":{big:?},\"redactionCount\":0}}\n\
{{\"id\":\"m_c0000000000000000000000000000000\",\"createdMs\":300,\"text\":{big:?},\"redactionCount\":0}}\n"
    );
    fs::write(memory_dir.join("entries.jsonl"), jsonl).unwrap();

    // Cap of 2500 bytes: two 1000-byte entries fit (2000), third
    // would push to 3000 and is dropped.
    let read = read_for_prompt(td.path(), 2500).expect("ok");
    assert_eq!(read.entries.len(), 2);
    assert!(read.truncated);
    // Newest two kept.
    assert_eq!(read.entries[0].id, "m_c0000000000000000000000000000000");
    assert_eq!(read.entries[1].id, "m_b0000000000000000000000000000000");
    assert_eq!(read.used_bytes, 2000);
}

#[test]
fn read_for_prompt_with_zero_cap_returns_empty_and_truncated_when_store_nonempty() {
    let td = TempDir::new("d42-zero-cap");
    let memory_dir = td.path().join(".plume").join("memory");
    fs::create_dir_all(&memory_dir).unwrap();
    fs::write(
        memory_dir.join("entries.jsonl"),
        "{\"id\":\"m_a0000000000000000000000000000000\",\"createdMs\":100,\"text\":\"hi\",\"redactionCount\":0}\n",
    )
    .unwrap();

    let read = read_for_prompt(td.path(), 0).expect("ok");
    assert!(read.entries.is_empty());
    assert_eq!(read.used_bytes, 0);
    assert_eq!(read.byte_cap, 0);
    // A non-empty store with cap 0 means "we tried to fold something
    // in but couldn't" — truncated is true so the UI surfaces the
    // warn marker.
    assert!(read.truncated);
}

#[test]
fn read_for_prompt_refuses_symlinked_plume_dir() {
    #[cfg(unix)]
    {
        let td = TempDir::new("d42-symlink");
        let real_target = td.path().join("not_plume");
        fs::create_dir_all(&real_target).unwrap();
        let plume_link = td.path().join(".plume");
        std::os::unix::fs::symlink(&real_target, &plume_link).unwrap();

        let err = read_for_prompt(td.path(), 4096).expect_err("symlinked .plume must refuse");
        let msg = err.to_string();
        assert!(
            msg.contains("symlink") || msg.contains(".plume"),
            "error must mention symlink defense; got: {msg}"
        );
    }
}

// --- D43: memory.search -------------------------------------------------

fn write_search_fixtures(root: &Path) {
    let memory_dir = root.join(".plume").join("memory");
    fs::create_dir_all(&memory_dir).unwrap();
    let jsonl = concat!(
        r#"{"id":"m_a0000000000000000000000000000000","createdMs":100,"text":"the build script lives at scripts/verify.sh","redactionCount":0}"#,
        "\n",
        r#"{"id":"m_b0000000000000000000000000000000","createdMs":200,"text":"BUILD steps: lint then tests then verify","redactionCount":0}"#,
        "\n",
        r#"{"id":"m_c0000000000000000000000000000000","createdMs":300,"text":"unrelated note","redactionCount":0}"#,
        "\n",
        r#"{"id":"m_d0000000000000000000000000000000","createdMs":400,"text":"build","redactionCount":0}"#,
        "\n",
    );
    fs::write(memory_dir.join("entries.jsonl"), jsonl).unwrap();
}

#[test]
fn search_rejects_empty_query() {
    let td = TempDir::new("d43-empty");
    write_search_fixtures(td.path());
    match search(td.path(), "   ", 10) {
        MemorySearchResponse::Err(err) => {
            assert_eq!(err.reason, MemorySearchFailure::EmptyQuery);
        }
        other => panic!("expected EmptyQuery err, got {other:?}"),
    }
}

#[test]
fn search_rejects_oversize_query() {
    let td = TempDir::new("d43-toolong");
    write_search_fixtures(td.path());
    let big = "x".repeat(SEARCH_MAX_QUERY_BYTES + 1);
    match search(td.path(), &big, 10) {
        MemorySearchResponse::Err(err) => {
            assert_eq!(err.reason, MemorySearchFailure::QueryTooLong);
        }
        other => panic!("expected QueryTooLong, got {other:?}"),
    }
}

#[test]
fn search_rejects_zero_limit() {
    let td = TempDir::new("d43-zerolimit");
    write_search_fixtures(td.path());
    match search(td.path(), "build", 0) {
        MemorySearchResponse::Err(err) => {
            assert_eq!(err.reason, MemorySearchFailure::BadLimit);
        }
        other => panic!("expected BadLimit, got {other:?}"),
    }
}

#[test]
fn search_rejects_oversize_limit() {
    let td = TempDir::new("d43-biglimit");
    write_search_fixtures(td.path());
    match search(td.path(), "build", SEARCH_MAX_LIMIT + 1) {
        MemorySearchResponse::Err(err) => {
            assert_eq!(err.reason, MemorySearchFailure::BadLimit);
        }
        other => panic!("expected BadLimit, got {other:?}"),
    }
}

#[test]
fn search_returns_empty_hits_when_store_is_empty() {
    let td = TempDir::new("d43-emptystore");
    let result = search(td.path(), "anything", 10);
    match result {
        MemorySearchResponse::Ok(ok) => {
            assert!(ok.hits.is_empty());
            assert!(!ok.truncated);
            assert_eq!(ok.query, "anything");
        }
        other => panic!("expected Ok with no hits, got {other:?}"),
    }
}

#[test]
fn search_is_case_insensitive() {
    let td = TempDir::new("d43-case");
    write_search_fixtures(td.path());
    let result = search(td.path(), "BUILD", 10);
    match result {
        MemorySearchResponse::Ok(ok) => {
            // 3 entries contain "build" case-insensitively
            assert_eq!(ok.hits.len(), 3, "got hits: {:?}", ok.hits);
        }
        other => panic!("expected Ok, got {other:?}"),
    }
}

#[test]
fn search_ranks_shorter_matches_first_then_newer_first() {
    let td = TempDir::new("d43-rank");
    write_search_fixtures(td.path());
    let result = search(td.path(), "build", 10);
    let MemorySearchResponse::Ok(ok) = result else {
        panic!("expected Ok");
    };
    // Shortest entry containing "build" is the literal `"build"`
    // at id `m_d…`. It must come first.
    assert_eq!(ok.hits[0].entry.id, "m_d0000000000000000000000000000000");
    // The other two have similar-ish length; newer first wins.
    // entry b has createdMs 200, entry a has createdMs 100, so b before a.
    assert_eq!(ok.hits[1].entry.id, "m_b0000000000000000000000000000000");
    assert_eq!(ok.hits[2].entry.id, "m_a0000000000000000000000000000000");
}

#[test]
fn search_truncates_to_limit_and_sets_truncated_flag() {
    let td = TempDir::new("d43-trunc");
    write_search_fixtures(td.path());
    // 3 entries match "build"; cap at 2.
    let result = search(td.path(), "build", 2);
    let MemorySearchResponse::Ok(ok) = result else {
        panic!("expected Ok");
    };
    assert_eq!(ok.hits.len(), 2);
    assert!(
        ok.truncated,
        "truncated must flip when more hits were available"
    );
}

#[test]
fn search_reports_match_count_and_first_index() {
    let td = TempDir::new("d43-counts");
    let memory_dir = td.path().join(".plume").join("memory");
    fs::create_dir_all(&memory_dir).unwrap();
    fs::write(
        memory_dir.join("entries.jsonl"),
        r#"{"id":"m_a0000000000000000000000000000000","createdMs":100,"text":"abcabcabc","redactionCount":0}"#.to_string() + "\n",
    )
    .unwrap();
    let result = search(td.path(), "abc", 10);
    let MemorySearchResponse::Ok(ok) = result else {
        panic!("expected Ok");
    };
    assert_eq!(ok.hits.len(), 1);
    assert_eq!(ok.hits[0].match_count, 3);
    assert_eq!(ok.hits[0].first_match_index, 0);
}

#[test]
fn search_refuses_symlinked_plume_dir() {
    #[cfg(unix)]
    {
        let td = TempDir::new("d43-symlink");
        let real = td.path().join("not_plume");
        fs::create_dir_all(&real).unwrap();
        let link = td.path().join(".plume");
        std::os::unix::fs::symlink(&real, &link).unwrap();

        match search(td.path(), "build", 10) {
            MemorySearchResponse::Err(err) => {
                assert_eq!(err.reason, MemorySearchFailure::StoreFailed);
                assert!(err.message.contains("symlink") || err.message.contains(".plume"));
            }
            other => panic!("expected StoreFailed for symlinked .plume, got {other:?}"),
        }
    }
}

#[test]
fn search_does_not_mutate_store() {
    // The whole point of a read-only verb is that running it
    // shouldn't change the JSONL on disk. Capture the bytes before
    // and after a search and assert byte-equality.
    let td = TempDir::new("d43-readonly");
    write_search_fixtures(td.path());
    let entries_path = td.path().join(".plume/memory/entries.jsonl");
    let before = fs::read(&entries_path).unwrap();
    let _ = search(td.path(), "build", 10);
    let _ = search(td.path(), "verify", 5);
    let after = fs::read(&entries_path).unwrap();
    assert_eq!(before, after, "search must not mutate the JSONL store");
}

// --- D48: distill_preview -----------------------------------------------

fn write_distill_fixtures(root: &Path, jsonl: &str) {
    let memory_dir = root.join(".plume").join("memory");
    fs::create_dir_all(&memory_dir).unwrap();
    fs::write(memory_dir.join("entries.jsonl"), jsonl).unwrap();
}

#[test]
fn distill_preview_returns_empty_preview_when_no_store_exists() {
    let td = TempDir::new("d48-empty");
    let preview = distill_preview(td.path()).expect("ok");
    assert!(preview.duplicate_groups.is_empty());
    assert_eq!(preview.total_entries, 0);
    assert_eq!(preview.would_remove, 0);
}

#[test]
fn distill_preview_returns_empty_groups_when_no_duplicates() {
    let td = TempDir::new("d48-nodupes");
    let jsonl = concat!(
        r#"{"id":"m_a0000000000000000000000000000000","createdMs":100,"text":"hello","redactionCount":0}"#,
        "\n",
        r#"{"id":"m_b0000000000000000000000000000000","createdMs":200,"text":"world","redactionCount":0}"#,
        "\n",
    );
    write_distill_fixtures(td.path(), jsonl);
    let preview = distill_preview(td.path()).expect("ok");
    assert!(preview.duplicate_groups.is_empty());
    assert_eq!(preview.total_entries, 2);
    assert_eq!(preview.would_remove, 0);
}

#[test]
fn distill_preview_groups_exact_duplicates_newest_first() {
    let td = TempDir::new("d48-exact");
    let jsonl = concat!(
        r#"{"id":"m_a0000000000000000000000000000000","createdMs":100,"text":"same fact","redactionCount":0}"#,
        "\n",
        r#"{"id":"m_b0000000000000000000000000000000","createdMs":200,"text":"same fact","redactionCount":0}"#,
        "\n",
        r#"{"id":"m_c0000000000000000000000000000000","createdMs":300,"text":"different","redactionCount":0}"#,
        "\n",
    );
    write_distill_fixtures(td.path(), jsonl);
    let preview = distill_preview(td.path()).expect("ok");
    assert_eq!(preview.total_entries, 3);
    assert_eq!(preview.duplicate_groups.len(), 1);
    let group = &preview.duplicate_groups[0];
    assert_eq!(group.entries.len(), 2);
    // Newest-first inside the group: b (created_ms 200) before a (100).
    assert_eq!(group.entries[0].id, "m_b0000000000000000000000000000000");
    assert_eq!(group.entries[1].id, "m_a0000000000000000000000000000000");
    assert_eq!(group.removable_count, 1);
    assert_eq!(preview.would_remove, 1);
}

#[test]
fn distill_preview_is_case_insensitive() {
    let td = TempDir::new("d48-case");
    let jsonl = concat!(
        r#"{"id":"m_a0000000000000000000000000000000","createdMs":100,"text":"PI is 3.14","redactionCount":0}"#,
        "\n",
        r#"{"id":"m_b0000000000000000000000000000000","createdMs":200,"text":"pi is 3.14","redactionCount":0}"#,
        "\n",
    );
    write_distill_fixtures(td.path(), jsonl);
    let preview = distill_preview(td.path()).expect("ok");
    assert_eq!(preview.duplicate_groups.len(), 1);
    assert_eq!(preview.duplicate_groups[0].entries.len(), 2);
}

#[test]
fn distill_preview_collapses_whitespace_runs() {
    let td = TempDir::new("d48-ws");
    let jsonl = concat!(
        r#"{"id":"m_a0000000000000000000000000000000","createdMs":100,"text":"two  spaces","redactionCount":0}"#,
        "\n",
        r#"{"id":"m_b0000000000000000000000000000000","createdMs":200,"text":"two spaces","redactionCount":0}"#,
        "\n",
        r#"{"id":"m_c0000000000000000000000000000000","createdMs":300,"text":"  two spaces  ","redactionCount":0}"#,
        "\n",
    );
    write_distill_fixtures(td.path(), jsonl);
    let preview = distill_preview(td.path()).expect("ok");
    assert_eq!(preview.duplicate_groups.len(), 1);
    assert_eq!(preview.duplicate_groups[0].entries.len(), 3);
    assert_eq!(preview.would_remove, 2);
}

#[test]
fn distill_preview_collapses_tabs_and_newlines_to_single_spaces() {
    let td = TempDir::new("d48-tabs");
    let jsonl = concat!(
        r#"{"id":"m_a0000000000000000000000000000000","createdMs":100,"text":"line break\nhere","redactionCount":0}"#,
        "\n",
        r#"{"id":"m_b0000000000000000000000000000000","createdMs":200,"text":"line break here","redactionCount":0}"#,
        "\n",
    );
    write_distill_fixtures(td.path(), jsonl);
    let preview = distill_preview(td.path()).expect("ok");
    assert_eq!(preview.duplicate_groups.len(), 1);
}

#[test]
fn distill_preview_reports_multiple_distinct_groups() {
    let td = TempDir::new("d48-multi");
    let jsonl = concat!(
        r#"{"id":"m_a0000000000000000000000000000000","createdMs":100,"text":"alpha","redactionCount":0}"#,
        "\n",
        r#"{"id":"m_b0000000000000000000000000000000","createdMs":150,"text":"alpha","redactionCount":0}"#,
        "\n",
        r#"{"id":"m_c0000000000000000000000000000000","createdMs":200,"text":"beta","redactionCount":0}"#,
        "\n",
        r#"{"id":"m_d0000000000000000000000000000000","createdMs":250,"text":"BETA","redactionCount":0}"#,
        "\n",
        r#"{"id":"m_e0000000000000000000000000000000","createdMs":300,"text":"beta","redactionCount":0}"#,
        "\n",
        r#"{"id":"m_f0000000000000000000000000000000","createdMs":400,"text":"unique","redactionCount":0}"#,
        "\n",
    );
    write_distill_fixtures(td.path(), jsonl);
    let preview = distill_preview(td.path()).expect("ok");
    assert_eq!(preview.total_entries, 6);
    assert_eq!(preview.duplicate_groups.len(), 2);
    // would_remove = (2-1) + (3-1) = 3
    assert_eq!(preview.would_remove, 3);
}

#[test]
fn distill_preview_group_ids_are_stable_across_calls() {
    let td = TempDir::new("d48-stable");
    let jsonl = concat!(
        r#"{"id":"m_a0000000000000000000000000000000","createdMs":100,"text":"same","redactionCount":0}"#,
        "\n",
        r#"{"id":"m_b0000000000000000000000000000000","createdMs":200,"text":"same","redactionCount":0}"#,
        "\n",
    );
    write_distill_fixtures(td.path(), jsonl);
    let a = distill_preview(td.path()).expect("ok");
    let b = distill_preview(td.path()).expect("ok");
    assert_eq!(a.duplicate_groups.len(), 1);
    assert_eq!(b.duplicate_groups.len(), 1);
    // The id encodes the normalized key + group size, both
    // unchanged between calls.
    assert_eq!(a.duplicate_groups[0].id, b.duplicate_groups[0].id);
}

#[test]
fn distill_preview_group_id_changes_when_group_size_grows() {
    // A future apply step would re-fetch the preview INSIDE the
    // mutex before committing; if the user remembers another
    // duplicate between preview and apply the group id changes,
    // signalling "the set you confirmed is no longer current,
    // re-preview." Pin that property.
    let td = TempDir::new("d48-size-id");
    let pair_only = concat!(
        r#"{"id":"m_a0000000000000000000000000000000","createdMs":100,"text":"same","redactionCount":0}"#,
        "\n",
        r#"{"id":"m_b0000000000000000000000000000000","createdMs":200,"text":"same","redactionCount":0}"#,
        "\n",
    );
    write_distill_fixtures(td.path(), pair_only);
    let pair_id = distill_preview(td.path()).expect("ok").duplicate_groups[0]
        .id
        .clone();
    let trio = pair_only.to_owned()
        + r#"{"id":"m_c0000000000000000000000000000000","createdMs":300,"text":"same","redactionCount":0}"#
        + "\n";
    write_distill_fixtures(td.path(), &trio);
    let trio_id = distill_preview(td.path()).expect("ok").duplicate_groups[0]
        .id
        .clone();
    assert_ne!(pair_id, trio_id, "group id must change when size changes");
}

#[test]
fn distill_preview_group_id_changes_when_member_set_drifts_with_same_size() {
    // D48 Codex MEDIUM regression. Pre-fix the group id encoded
    // only normalized text + count, so swapping one member for
    // another while keeping the count constant left the id
    // identical — a future apply step would stale-match and could
    // remove entries the user didn't confirm. The fix encodes
    // sorted member ids into the hash. Pin the property end-to-end.
    let td = TempDir::new("d48-member-drift");
    let pair_ab = concat!(
        r#"{"id":"m_a0000000000000000000000000000000","createdMs":100,"text":"same","redactionCount":0}"#,
        "\n",
        r#"{"id":"m_b0000000000000000000000000000000","createdMs":200,"text":"same","redactionCount":0}"#,
        "\n",
    );
    write_distill_fixtures(td.path(), pair_ab);
    let id_ab = distill_preview(td.path()).expect("ok").duplicate_groups[0]
        .id
        .clone();
    // Forget `a`, remember a new duplicate `c`. Group size stays
    // at 2 but the member set is {b, c} instead of {a, b}.
    let pair_bc = concat!(
        r#"{"id":"m_b0000000000000000000000000000000","createdMs":200,"text":"same","redactionCount":0}"#,
        "\n",
        r#"{"id":"m_c0000000000000000000000000000000","createdMs":300,"text":"same","redactionCount":0}"#,
        "\n",
    );
    write_distill_fixtures(td.path(), pair_bc);
    let id_bc = distill_preview(td.path()).expect("ok").duplicate_groups[0]
        .id
        .clone();
    assert_ne!(
        id_ab, id_bc,
        "group id must change when member set drifts even with the same size"
    );
}

#[test]
fn distill_preview_group_id_is_independent_of_input_order() {
    // The hash sorts member ids before mixing them in, so two
    // stores with the same members in different on-disk order
    // produce the same group id. Pin that property — without
    // it, JSONL re-ordering by a future compaction would
    // invalidate every saved group id.
    let td_ab = TempDir::new("d48-order-ab");
    let ab = concat!(
        r#"{"id":"m_a0000000000000000000000000000000","createdMs":100,"text":"same","redactionCount":0}"#,
        "\n",
        r#"{"id":"m_b0000000000000000000000000000000","createdMs":200,"text":"same","redactionCount":0}"#,
        "\n",
    );
    write_distill_fixtures(td_ab.path(), ab);
    let id_ab = distill_preview(td_ab.path()).expect("ok").duplicate_groups[0]
        .id
        .clone();
    let td_ba = TempDir::new("d48-order-ba");
    let ba = concat!(
        r#"{"id":"m_b0000000000000000000000000000000","createdMs":200,"text":"same","redactionCount":0}"#,
        "\n",
        r#"{"id":"m_a0000000000000000000000000000000","createdMs":100,"text":"same","redactionCount":0}"#,
        "\n",
    );
    write_distill_fixtures(td_ba.path(), ba);
    let id_ba = distill_preview(td_ba.path()).expect("ok").duplicate_groups[0]
        .id
        .clone();
    assert_eq!(
        id_ab, id_ba,
        "group id must be invariant under member-input reordering"
    );
}

#[test]
fn distill_preview_does_not_mutate_store() {
    let td = TempDir::new("d48-readonly");
    let jsonl = concat!(
        r#"{"id":"m_a0000000000000000000000000000000","createdMs":100,"text":"same","redactionCount":0}"#,
        "\n",
        r#"{"id":"m_b0000000000000000000000000000000","createdMs":200,"text":"same","redactionCount":0}"#,
        "\n",
    );
    write_distill_fixtures(td.path(), jsonl);
    let entries_path = td.path().join(".plume/memory/entries.jsonl");
    let before = fs::read(&entries_path).unwrap();
    let _ = distill_preview(td.path()).expect("ok");
    let _ = distill_preview(td.path()).expect("ok");
    let after = fs::read(&entries_path).unwrap();
    assert_eq!(
        before, after,
        "distill_preview must not mutate the JSONL store"
    );
}

#[test]
fn distill_preview_skips_entries_that_normalize_to_empty() {
    // A whitespace-only entry shouldn't anchor a duplicate cluster
    // — if two entries both normalize to the empty string they
    // would otherwise group together, which is noise, not signal.
    // (The remember verb already rejects empty text, so this is
    // defense in depth against hand-edited JSONL.)
    let td = TempDir::new("d48-empty-norm");
    let jsonl = concat!(
        r#"{"id":"m_a0000000000000000000000000000000","createdMs":100,"text":"   ","redactionCount":0}"#,
        "\n",
        r#"{"id":"m_b0000000000000000000000000000000","createdMs":200,"text":"\t\n","redactionCount":0}"#,
        "\n",
    );
    write_distill_fixtures(td.path(), jsonl);
    let preview = distill_preview(td.path()).expect("ok");
    assert!(preview.duplicate_groups.is_empty());
    assert_eq!(preview.would_remove, 0);
}

#[test]
fn distill_preview_refuses_symlinked_plume_dir() {
    #[cfg(unix)]
    {
        let td = TempDir::new("d48-symlink");
        let real = td.path().join("not_plume");
        fs::create_dir_all(&real).unwrap();
        let link = td.path().join(".plume");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let err = distill_preview(td.path()).expect_err("symlinked .plume must refuse");
        let msg = err.to_string();
        assert!(
            msg.contains("symlink") || msg.contains(".plume"),
            "error must mention symlink defense; got: {msg}"
        );
    }
}

#[test]
fn normalize_for_distill_matches_documented_rules() {
    // Pin the three documented normalization rules so a future
    // refactor that changes any of them fires a test instead of
    // silently shifting the cluster boundaries.
    // 1) Trim leading/trailing whitespace.
    assert_eq!(normalize_for_distill("  hi  "), "hi");
    // 2) Collapse internal whitespace runs (any whitespace
    //    character, not just spaces) to a single space.
    assert_eq!(normalize_for_distill("a   b\t\tc\nd"), "a b c d");
    // 3) Lowercase.
    assert_eq!(normalize_for_distill("Foo BAR"), "foo bar");
    // Combined.
    assert_eq!(normalize_for_distill("  Foo\tBAR\n"), "foo bar");
    // Empty edge case.
    assert_eq!(normalize_for_distill(""), "");
    assert_eq!(normalize_for_distill("   "), "");
    // Redaction markers survive: only inner-whitespace is collapsed.
    assert_eq!(
        normalize_for_distill("[REDACTED:githubPat]   landed"),
        "[redacted:githubpat] landed"
    );
}

// ─── D54: wire-shape pins for the `memory.distillPreview` IPC verb ──────

/// `DistillPreview` and `DuplicateGroup` cross the wire as camelCase.
/// The IPC contract names `totalEntries`, `wouldRemove`,
/// `duplicateGroups`, and `removableCount`; a rename rule drift would
/// silently break the frontend, so pin the serialization shape here.
#[test]
fn distill_preview_serializes_with_camel_case_field_names() {
    let preview = DistillPreview {
        duplicate_groups: Vec::new(),
        total_entries: 0,
        would_remove: 0,
    };
    let json = serde_json::to_value(&preview).expect("serialize");
    assert_eq!(
        json,
        serde_json::json!({
            "duplicateGroups": [],
            "totalEntries": 0,
            "wouldRemove": 0,
        })
    );
}

/// Same shape pin for `DuplicateGroup`. `removableCount` is the
/// renamed field the wire expects; a serde drift to snake_case
/// would break the panel without obvious failure.
#[test]
fn duplicate_group_serializes_with_camel_case_field_names() {
    let group = DuplicateGroup {
        id: "group_xyz".into(),
        entries: Vec::new(),
        removable_count: 2,
        merged_links: vec!["topics/a.md".into()],
        link_cap_exceeded: false,
    };
    let json = serde_json::to_value(&group).expect("serialize");
    assert_eq!(
        json,
        serde_json::json!({
            "id": "group_xyz",
            "entries": [],
            "removableCount": 2,
            "mergedLinks": ["topics/a.md"],
            "linkCapExceeded": false,
        })
    );
}

/// D54 frontend-contract floor: a freshly-trusted project with no
/// entries returns an empty preview. The panel button is safe to
/// click without first running `memory.index`. (The D48 tests cover
/// the same shape under the pre-IPC name; this pin lives here so the
/// renamed types don't quietly stop honoring it.)
#[test]
fn distill_preview_d54_empty_store_pin() {
    let td = TempDir::new("d54-empty");
    let preview = distill_preview(td.path()).expect("preview");
    assert_eq!(preview.total_entries, 0);
    assert_eq!(preview.would_remove, 0);
    assert!(preview.duplicate_groups.is_empty());
}

// ─── D64: distill_apply (rule-based dedupe write path) ──────────────────

fn unwrap_distill_apply_ok(resp: MemoryDistillApplyResponse) -> MemoryDistillApplyOk {
    match resp {
        MemoryDistillApplyResponse::Ok(ok) => ok,
        MemoryDistillApplyResponse::Err(e) => {
            panic!(
                "expected ok, got err: reason={:?} msg={:?}",
                e.reason, e.message
            )
        }
    }
}

/// Apply the single duplicate group: the newest entry survives, the
/// older duplicate is removed, and the unrelated entry is untouched.
#[test]
fn distill_apply_removes_non_survivors_keeps_newest() {
    let td = TempDir::new("d64-basic");
    let jsonl = concat!(
        r#"{"id":"m_a0000000000000000000000000000000","createdMs":100,"text":"same fact","redactionCount":0}"#,
        "\n",
        r#"{"id":"m_b0000000000000000000000000000000","createdMs":200,"text":"same fact","redactionCount":0}"#,
        "\n",
        r#"{"id":"m_c0000000000000000000000000000000","createdMs":300,"text":"different","redactionCount":0}"#,
        "\n",
    );
    write_distill_fixtures(td.path(), jsonl);

    let group_id = distill_preview(td.path())
        .expect("preview")
        .duplicate_groups[0]
        .id
        .clone();
    let ok = unwrap_distill_apply_ok(distill_apply(td.path(), &[group_id]));
    assert_eq!(ok.removed_entry_count, 1);
    assert_eq!(ok.remaining_entry_count, 2);
    assert!(ok.unmatched_group_ids.is_empty());

    let index = read_index(td.path()).expect("index");
    let ids: Vec<&str> = index.entries.iter().map(|e| e.id.as_str()).collect();
    // The newest of the dup group (b) survives; the older (a) is gone;
    // the unrelated entry (c) stays.
    assert!(ids.contains(&"m_b0000000000000000000000000000000"));
    assert!(ids.contains(&"m_c0000000000000000000000000000000"));
    assert!(!ids.contains(&"m_a0000000000000000000000000000000"));
    // Re-preview: no duplicates remain.
    assert!(distill_preview(td.path())
        .expect("preview")
        .duplicate_groups
        .is_empty());
}

/// An empty group-id list is a successful no-op that leaves the store
/// byte-identical.
#[test]
fn distill_apply_empty_group_ids_is_noop() {
    let td = TempDir::new("d64-empty-ids");
    let jsonl = concat!(
        r#"{"id":"m_a0000000000000000000000000000000","createdMs":100,"text":"same fact","redactionCount":0}"#,
        "\n",
        r#"{"id":"m_b0000000000000000000000000000000","createdMs":200,"text":"same fact","redactionCount":0}"#,
        "\n",
    );
    write_distill_fixtures(td.path(), jsonl);
    let before = fs::read_to_string(td.path().join(".plume/memory/entries.jsonl")).unwrap();

    let ok = unwrap_distill_apply_ok(distill_apply(td.path(), &[]));
    assert_eq!(ok.removed_entry_count, 0);
    assert_eq!(ok.remaining_entry_count, 2);
    assert!(ok.unmatched_group_ids.is_empty());

    let after = fs::read_to_string(td.path().join(".plume/memory/entries.jsonl")).unwrap();
    assert_eq!(before, after, "no-op apply must not rewrite the store");
}

/// A stale / unknown group id is a no-op: nothing is removed and the
/// id surfaces in `unmatched_group_ids` so the UI can prompt a re-scan.
#[test]
fn distill_apply_stale_id_is_noop_and_reported_unmatched() {
    let td = TempDir::new("d64-stale");
    let jsonl = concat!(
        r#"{"id":"m_a0000000000000000000000000000000","createdMs":100,"text":"same fact","redactionCount":0}"#,
        "\n",
        r#"{"id":"m_b0000000000000000000000000000000","createdMs":200,"text":"same fact","redactionCount":0}"#,
        "\n",
    );
    write_distill_fixtures(td.path(), jsonl);

    let ok = unwrap_distill_apply_ok(distill_apply(
        td.path(),
        &["dup_deadbeefdeadbeef_2".to_string()],
    ));
    assert_eq!(ok.removed_entry_count, 0);
    assert_eq!(ok.remaining_entry_count, 2);
    assert_eq!(
        ok.unmatched_group_ids,
        vec!["dup_deadbeefdeadbeef_2".to_string()]
    );
}

/// With two duplicate groups, applying only one id compacts that
/// group and leaves the other intact.
#[test]
fn distill_apply_only_touches_requested_group() {
    let td = TempDir::new("d64-subset");
    let jsonl = concat!(
        r#"{"id":"m_a0000000000000000000000000000000","createdMs":100,"text":"alpha","redactionCount":0}"#,
        "\n",
        r#"{"id":"m_b0000000000000000000000000000000","createdMs":150,"text":"alpha","redactionCount":0}"#,
        "\n",
        r#"{"id":"m_c0000000000000000000000000000000","createdMs":200,"text":"beta","redactionCount":0}"#,
        "\n",
        r#"{"id":"m_d0000000000000000000000000000000","createdMs":250,"text":"beta","redactionCount":0}"#,
        "\n",
    );
    write_distill_fixtures(td.path(), jsonl);

    let preview = distill_preview(td.path()).expect("preview");
    assert_eq!(preview.duplicate_groups.len(), 2);
    // Pick the group whose survivor text normalizes to "alpha".
    let alpha_id = preview
        .duplicate_groups
        .iter()
        .find(|g| g.entries[0].text == "alpha")
        .expect("alpha group")
        .id
        .clone();

    let ok = unwrap_distill_apply_ok(distill_apply(td.path(), &[alpha_id]));
    assert_eq!(ok.removed_entry_count, 1);
    assert_eq!(ok.remaining_entry_count, 3);

    // The beta group is still a live duplicate.
    let after = distill_preview(td.path()).expect("preview");
    assert_eq!(after.duplicate_groups.len(), 1);
    assert_eq!(after.duplicate_groups[0].entries[0].text, "beta");
}

/// Applying against a project with no store is a clean no-op, not an
/// error.
#[test]
fn distill_apply_no_store_is_ok_noop() {
    let td = TempDir::new("d64-no-store");
    let ok = unwrap_distill_apply_ok(distill_apply(td.path(), &[]));
    assert_eq!(ok.removed_entry_count, 0);
    assert_eq!(ok.remaining_entry_count, 0);
}

/// Survivors keep their original on-disk order — apply only drops the
/// removed lines, it does not reorder the file.
#[test]
fn distill_apply_preserves_survivor_disk_order() {
    let td = TempDir::new("d64-order");
    let jsonl = concat!(
        r#"{"id":"m_a0000000000000000000000000000000","createdMs":100,"text":"zeta","redactionCount":0}"#,
        "\n",
        r#"{"id":"m_b0000000000000000000000000000000","createdMs":150,"text":"dup","redactionCount":0}"#,
        "\n",
        r#"{"id":"m_c0000000000000000000000000000000","createdMs":200,"text":"dup","redactionCount":0}"#,
        "\n",
        r#"{"id":"m_d0000000000000000000000000000000","createdMs":250,"text":"omega","redactionCount":0}"#,
        "\n",
    );
    write_distill_fixtures(td.path(), jsonl);

    let group_id = distill_preview(td.path())
        .expect("preview")
        .duplicate_groups[0]
        .id
        .clone();
    unwrap_distill_apply_ok(distill_apply(td.path(), &[group_id]));

    let index = read_index(td.path()).expect("index");
    let ids: Vec<&str> = index.entries.iter().map(|e| e.id.as_str()).collect();
    // a (zeta), c (newest dup survivor), d (omega) — original order,
    // with b (older dup) removed.
    assert_eq!(
        ids,
        vec![
            "m_a0000000000000000000000000000000",
            "m_c0000000000000000000000000000000",
            "m_d0000000000000000000000000000000",
        ]
    );
}

#[test]
fn distill_apply_refuses_symlinked_plume_dir() {
    #[cfg(unix)]
    {
        let td = TempDir::new("d64-symlink");
        let real = td.path().join("not_plume");
        fs::create_dir_all(&real).unwrap();
        let link = td.path().join(".plume");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        match distill_apply(td.path(), &["dup_x_2".to_string()]) {
            MemoryDistillApplyResponse::Err(e) => {
                assert_eq!(e.reason, MemoryDistillApplyFailure::StoreFailed);
            }
            MemoryDistillApplyResponse::Ok(_) => panic!("symlinked .plume must refuse"),
        }
    }
}

// ─── D69: distillation audit log ────────────────────────────────────────

/// A compaction that removes entries appends one audit record naming
/// the removed (older) and kept (newest survivor) ids.
#[test]
fn distill_apply_writes_audit_log_record() {
    let td = TempDir::new("d69-record");
    let jsonl = concat!(
        r#"{"id":"m_a0000000000000000000000000000000","createdMs":100,"text":"same fact","redactionCount":0}"#,
        "\n",
        r#"{"id":"m_b0000000000000000000000000000000","createdMs":200,"text":"same fact","redactionCount":0}"#,
        "\n",
        r#"{"id":"m_c0000000000000000000000000000000","createdMs":300,"text":"different","redactionCount":0}"#,
        "\n",
    );
    write_distill_fixtures(td.path(), jsonl);

    let group_id = distill_preview(td.path())
        .expect("preview")
        .duplicate_groups[0]
        .id
        .clone();
    unwrap_distill_apply_ok(distill_apply(td.path(), &[group_id]));

    let log = read_distill_log(td.path()).expect("log");
    assert_eq!(log.len(), 1);
    let record = &log[0];
    assert_eq!(record.rule, "dedupeExact");
    assert!(record.ts_ms > 0);
    // The older entry (a) is removed; the newest (b) survives.
    assert_eq!(
        record.removed_ids,
        vec!["m_a0000000000000000000000000000000"]
    );
    assert_eq!(record.kept_ids, vec!["m_b0000000000000000000000000000000"]);
}

/// A no-op apply (empty id list) writes no audit record.
#[test]
fn distill_apply_noop_writes_no_audit_log() {
    let td = TempDir::new("d69-noop");
    let jsonl = concat!(
        r#"{"id":"m_a0000000000000000000000000000000","createdMs":100,"text":"same fact","redactionCount":0}"#,
        "\n",
        r#"{"id":"m_b0000000000000000000000000000000","createdMs":200,"text":"same fact","redactionCount":0}"#,
        "\n",
    );
    write_distill_fixtures(td.path(), jsonl);

    unwrap_distill_apply_ok(distill_apply(td.path(), &[]));
    assert!(read_distill_log(td.path()).expect("log").is_empty());
}

/// Successive compactions accumulate records, newest first.
#[test]
fn distill_apply_appends_across_compactions() {
    let td = TempDir::new("d69-multi");
    let jsonl = concat!(
        r#"{"id":"m_a0000000000000000000000000000000","createdMs":100,"text":"alpha","redactionCount":0}"#,
        "\n",
        r#"{"id":"m_b0000000000000000000000000000000","createdMs":150,"text":"alpha","redactionCount":0}"#,
        "\n",
        r#"{"id":"m_c0000000000000000000000000000000","createdMs":200,"text":"beta","redactionCount":0}"#,
        "\n",
        r#"{"id":"m_d0000000000000000000000000000000","createdMs":250,"text":"beta","redactionCount":0}"#,
        "\n",
    );
    write_distill_fixtures(td.path(), jsonl);

    let preview = distill_preview(td.path()).expect("preview");
    let alpha = preview
        .duplicate_groups
        .iter()
        .find(|g| g.entries[0].text == "alpha")
        .unwrap()
        .id
        .clone();
    let beta = preview
        .duplicate_groups
        .iter()
        .find(|g| g.entries[0].text == "beta")
        .unwrap()
        .id
        .clone();

    unwrap_distill_apply_ok(distill_apply(td.path(), &[alpha]));
    unwrap_distill_apply_ok(distill_apply(td.path(), &[beta]));

    let log = read_distill_log(td.path()).expect("log");
    assert_eq!(log.len(), 2);
    // Newest first: the beta compaction (second) leads.
    assert_eq!(log[0].kept_ids, vec!["m_d0000000000000000000000000000000"]);
    assert_eq!(log[1].kept_ids, vec!["m_b0000000000000000000000000000000"]);
}

#[test]
fn read_distill_log_empty_when_no_file() {
    let td = TempDir::new("d69-empty");
    assert!(read_distill_log(td.path()).expect("log").is_empty());
}

/// The audit log is bounded: appending past the cap drops the oldest
/// records and keeps the newest `DISTILL_LOG_MAX_RECORDS`, newest first.
#[test]
fn append_distill_log_caps_to_max_records() {
    let td = TempDir::new("d69-cap");
    fs::create_dir_all(td.path().join(".plume").join("memory")).unwrap();

    let total = DISTILL_LOG_MAX_RECORDS + 10;
    for i in 0..total {
        let record = DistillLogEntry {
            ts_ms: 1_000 + i as u64,
            rule: "dedupeExact".to_string(),
            removed_ids: vec![format!("m_{:032x}", i)],
            kept_ids: vec![format!("m_{:032x}", i + 1)],
            link_merges: Vec::new(),
        };
        append_distill_log(td.path(), &record).expect("append");
    }

    let log = read_distill_log(td.path()).expect("log");
    assert_eq!(log.len(), DISTILL_LOG_MAX_RECORDS);
    // Newest first: the last appended (ts 1000 + total - 1) leads, and
    // the oldest surviving record is `total - MAX`.
    assert_eq!(log[0].ts_ms, 1_000 + (total as u64) - 1);
    assert_eq!(
        log[DISTILL_LOG_MAX_RECORDS - 1].ts_ms,
        1_000 + (total - DISTILL_LOG_MAX_RECORDS) as u64
    );
}

#[test]
fn read_distill_log_refuses_symlinked_plume_dir() {
    #[cfg(unix)]
    {
        let td = TempDir::new("d69-symlink");
        let real = td.path().join("not_plume");
        fs::create_dir_all(&real).unwrap();
        std::os::unix::fs::symlink(&real, td.path().join(".plume")).unwrap();
        let err = read_distill_log(td.path()).expect_err("symlinked .plume must refuse");
        assert!(err.to_string().contains("symlink") || err.to_string().contains(".plume"));
    }
}

/// Wire-shape pin: the `Ok` response crosses the wire as camelCase so
/// the frontend reads `removedEntryCount` / `remainingEntryCount` /
/// `unmatchedGroupIds`.
#[test]
fn distill_apply_ok_serializes_with_camel_case_field_names() {
    let ok = MemoryDistillApplyResponse::Ok(MemoryDistillApplyOk {
        ok: true,
        removed_entry_count: 2,
        remaining_entry_count: 5,
        unmatched_group_ids: vec!["dup_x_2".to_string()],
        conflicted_group_ids: vec!["dup_y_3".to_string()],
        audit_logged: true,
    });
    let json = serde_json::to_value(&ok).expect("serialize");
    assert_eq!(
        json,
        serde_json::json!({
            "ok": true,
            "removedEntryCount": 2,
            "remainingEntryCount": 5,
            "unmatchedGroupIds": ["dup_x_2"],
            "conflictedGroupIds": ["dup_y_3"],
            "auditLogged": true,
        })
    );
}

// ─── D81 (Codex review): audit-logged honesty + final-file symlink ──────

#[test]
fn distill_apply_reports_audit_logged_true_on_normal_apply() {
    let td = TempDir::new("d81-audit-ok");
    let jsonl = concat!(
        r#"{"id":"m_a0000000000000000000000000000000","createdMs":100,"text":"same fact","redactionCount":0}"#,
        "\n",
        r#"{"id":"m_b0000000000000000000000000000000","createdMs":200,"text":"same fact","redactionCount":0}"#,
        "\n",
    );
    write_distill_fixtures(td.path(), jsonl);
    let id = distill_preview(td.path())
        .expect("preview")
        .duplicate_groups[0]
        .id
        .clone();
    let ok = unwrap_distill_apply_ok(distill_apply(td.path(), &[id]));
    assert_eq!(ok.removed_entry_count, 1);
    assert!(ok.audit_logged, "a successful compaction is recorded");
    assert_eq!(read_distill_log(td.path()).expect("log").len(), 1);
}

/// The "never hide memory writes" property: when the entries rewrite
/// commits but the best-effort audit append fails, the deletion is real
/// and `audit_logged` is `false` (not silently swallowed).
#[test]
fn distill_apply_reports_audit_logged_false_when_log_write_fails() {
    let td = TempDir::new("d81-audit-fail");
    let jsonl = concat!(
        r#"{"id":"m_a0000000000000000000000000000000","createdMs":100,"text":"same fact","redactionCount":0}"#,
        "\n",
        r#"{"id":"m_b0000000000000000000000000000000","createdMs":200,"text":"same fact","redactionCount":0}"#,
        "\n",
    );
    write_distill_fixtures(td.path(), jsonl);
    // Make the log path un-writable by planting a DIRECTORY where the
    // log file would go: the append's read/write both fail on it.
    fs::create_dir(
        td.path()
            .join(".plume")
            .join("memory")
            .join("distill-log.jsonl"),
    )
    .unwrap();

    let id = distill_preview(td.path())
        .expect("preview")
        .duplicate_groups[0]
        .id
        .clone();
    let ok = unwrap_distill_apply_ok(distill_apply(td.path(), &[id]));
    // The compaction still happened — the rewrite commits first.
    assert_eq!(ok.removed_entry_count, 1);
    assert_eq!(read_index(td.path()).unwrap().entries.len(), 1);
    // ...but it could not be recorded, and we say so.
    assert!(
        !ok.audit_logged,
        "unrecorded compaction must report audit_logged=false"
    );
}

#[test]
fn read_distill_log_refuses_symlinked_log_file() {
    #[cfg(unix)]
    {
        let td = TempDir::new("d81-log-symlink");
        let memory_dir = td.path().join(".plume").join("memory");
        fs::create_dir_all(&memory_dir).unwrap();
        let outside = td.path().join("outside.jsonl");
        fs::write(&outside, "{}\n").unwrap();
        std::os::unix::fs::symlink(&outside, memory_dir.join("distill-log.jsonl")).unwrap();

        let err = read_distill_log(td.path()).expect_err("symlinked log file must refuse");
        assert!(err.to_string().contains("symlink"));
    }
}

#[test]
fn read_index_refuses_symlinked_entries_file() {
    #[cfg(unix)]
    {
        let td = TempDir::new("d81-entries-symlink");
        let memory_dir = td.path().join(".plume").join("memory");
        fs::create_dir_all(&memory_dir).unwrap();
        let outside = td.path().join("outside.jsonl");
        fs::write(&outside, "{}\n").unwrap();
        std::os::unix::fs::symlink(&outside, memory_dir.join("entries.jsonl")).unwrap();

        let err = read_index(td.path()).expect_err("symlinked entries file must refuse");
        assert!(err.to_string().contains("symlink"));
    }
}

// ─── D71: curated memory topic files ────────────────────────────────────

fn write_memory_file(root: &Path, rel: &str, content: &str) {
    let path = root.join(".plume").join("memory").join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, content).unwrap();
}

#[test]
fn read_topics_empty_when_nothing_created() {
    let td = TempDir::new("d71-empty");
    let topics = read_topics(td.path()).expect("topics");
    // The core trio always appears, in fixed order, marked missing.
    assert_eq!(topics.core.len(), 3);
    assert_eq!(topics.core[0].kind, TopicKind::Index);
    assert_eq!(topics.core[1].kind, TopicKind::User);
    assert_eq!(topics.core[2].kind, TopicKind::Soul);
    assert!(topics.core.iter().all(|f| !f.exists));
    assert!(topics
        .core
        .iter()
        .all(|f| f.content.is_empty() && f.bytes == 0));
    assert!(topics.topics.is_empty());
    assert!(!topics.topics_truncated);
    assert_eq!(topics.limits.max_core_bytes, MAX_CORE_FILE_BYTES as u32);
}

#[test]
fn read_topics_reads_core_files() {
    let td = TempDir::new("d71-core");
    write_memory_file(td.path(), "INDEX.md", "# Index\nsee topics/");
    write_memory_file(td.path(), "USER.md", "prefers terse answers");
    write_memory_file(td.path(), "SOUL.md", "be direct and careful");

    let topics = read_topics(td.path()).expect("topics");
    assert_eq!(topics.core[0].name, "INDEX.md");
    assert!(topics.core[0].exists);
    assert_eq!(topics.core[0].content, "# Index\nsee topics/");
    assert!(topics.core[0].bytes > 0);
    assert!(!topics.core[0].truncated);
    assert_eq!(topics.core[1].content, "prefers terse answers");
    assert_eq!(topics.core[2].content, "be direct and careful");
}

#[test]
fn read_topics_lists_topic_dir_sorted() {
    let td = TempDir::new("d71-topicdir");
    write_memory_file(td.path(), "topics/zeta.md", "z");
    write_memory_file(td.path(), "topics/alpha.md", "a");
    write_memory_file(td.path(), "topics/mid.md", "m");

    let topics = read_topics(td.path()).expect("topics");
    let names: Vec<&str> = topics.topics.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(
        names,
        vec!["topics/alpha.md", "topics/mid.md", "topics/zeta.md"]
    );
    assert!(topics.topics.iter().all(|f| f.kind == TopicKind::Topic));
}

#[test]
fn read_topics_skips_non_md_and_subdirs() {
    let td = TempDir::new("d71-skip");
    write_memory_file(td.path(), "topics/keep.md", "yes");
    write_memory_file(td.path(), "topics/notes.txt", "no");
    write_memory_file(td.path(), "topics/nested/deep.md", "no");

    let topics = read_topics(td.path()).expect("topics");
    let names: Vec<&str> = topics.topics.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(names, vec!["topics/keep.md"]);
}

#[test]
fn read_topics_caps_core_file_content() {
    let td = TempDir::new("d71-cap");
    let big = "x".repeat(MAX_CORE_FILE_BYTES + 500);
    write_memory_file(td.path(), "USER.md", &big);

    let topics = read_topics(td.path()).expect("topics");
    let user = &topics.core[1];
    assert!(user.exists);
    assert!(user.truncated);
    assert_eq!(user.content.len(), MAX_CORE_FILE_BYTES);
    // `bytes` is the full on-disk size, before capping.
    assert_eq!(user.bytes, (MAX_CORE_FILE_BYTES + 500) as u64);
}

#[test]
fn read_topics_caps_topic_count() {
    let td = TempDir::new("d71-count");
    for i in 0..(MAX_TOPIC_FILES + 5) {
        write_memory_file(td.path(), &format!("topics/t{:03}.md", i), "x");
    }
    let topics = read_topics(td.path()).expect("topics");
    assert_eq!(topics.topics.len(), MAX_TOPIC_FILES);
    assert!(topics.topics_truncated);
}

#[test]
fn read_topics_keeps_valid_utf8_prefix_when_capping() {
    let td = TempDir::new("d71-utf8");
    // Fill just under the cap with ASCII, then a multi-byte char that
    // straddles the cap boundary. The cap must drop the partial char,
    // never panic or corrupt.
    let mut s = "a".repeat(MAX_CORE_FILE_BYTES - 1);
    s.push('é'); // 2 bytes: pushes total to cap + 1
    write_memory_file(td.path(), "INDEX.md", &s);

    let topics = read_topics(td.path()).expect("topics");
    let index = &topics.core[0];
    assert!(index.truncated);
    // The trailing 'é' is dropped (its first byte landed at the cap),
    // leaving the ASCII prefix only.
    assert_eq!(index.content.len(), MAX_CORE_FILE_BYTES - 1);
    assert!(index.content.chars().all(|c| c == 'a'));
}

#[test]
fn read_topics_refuses_symlinked_core_file() {
    #[cfg(unix)]
    {
        let td = TempDir::new("d71-symlink-core");
        let memory_dir = td.path().join(".plume").join("memory");
        fs::create_dir_all(&memory_dir).unwrap();
        let outside = td.path().join("secret.md");
        fs::write(&outside, "secret").unwrap();
        std::os::unix::fs::symlink(&outside, memory_dir.join("SOUL.md")).unwrap();

        let err = read_topics(td.path()).expect_err("symlinked core file must refuse");
        assert!(err.to_string().contains("symlink"));
    }
}

#[test]
fn read_topics_skips_symlinked_topic_file() {
    #[cfg(unix)]
    {
        let td = TempDir::new("d71-symlink-topic");
        write_memory_file(td.path(), "topics/real.md", "real");
        let topics_dir = td.path().join(".plume").join("memory").join("topics");
        let outside = td.path().join("secret.md");
        fs::write(&outside, "secret").unwrap();
        std::os::unix::fs::symlink(&outside, topics_dir.join("evil.md")).unwrap();

        let topics = read_topics(td.path()).expect("topics");
        let names: Vec<&str> = topics.topics.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, vec!["topics/real.md"]);
    }
}

#[test]
fn read_topics_refuses_symlinked_plume_dir() {
    #[cfg(unix)]
    {
        let td = TempDir::new("d71-symlink-plume");
        let real = td.path().join("not_plume");
        fs::create_dir_all(&real).unwrap();
        std::os::unix::fs::symlink(&real, td.path().join(".plume")).unwrap();
        let err = read_topics(td.path()).expect_err("symlinked .plume must refuse");
        assert!(err.to_string().contains("symlink") || err.to_string().contains(".plume"));
    }
}

// ─── D75: review-driven test gaps ───────────────────────────────────────

/// Review (topics agent, Low): pin the budget-overflow skip branch of
/// `read_core_for_prompt`, which is unreachable under the production
/// 6 KiB cap (three 2 KiB files always fit). A file that overflows the
/// budget is skipped with `truncated` set, while a later smaller file
/// is still considered and `used_bytes` stays accurate.
#[test]
fn read_core_for_prompt_skips_budget_overflow_keeps_later_fitting_file() {
    let td = TempDir::new("d72-budget");
    write_memory_file(td.path(), "INDEX.md", &"x".repeat(40)); // overflows cap 10
    write_memory_file(td.path(), "SOUL.md", "ok"); // 2 bytes, fits
    let read = read_core_for_prompt(td.path(), 10).expect("read");
    let names: Vec<&str> = read.files.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(names, vec!["SOUL.md"]);
    assert!(
        read.truncated,
        "INDEX should be flagged as skipped for the budget"
    );
    assert_eq!(read.used_bytes, 2);
    assert!(read.used_bytes <= read.byte_cap);
}

/// Review (distill agent, Low): a confirmed group id repeated in the
/// payload removes the group exactly once (the matched-id set and
/// unmatched de-dup both handle it), never twice.
#[test]
fn distill_apply_handles_duplicated_confirmed_id() {
    let td = TempDir::new("d64-dupid");
    let jsonl = concat!(
        r#"{"id":"m_a0000000000000000000000000000000","createdMs":100,"text":"same fact","redactionCount":0}"#,
        "\n",
        r#"{"id":"m_b0000000000000000000000000000000","createdMs":200,"text":"same fact","redactionCount":0}"#,
        "\n",
    );
    write_distill_fixtures(td.path(), jsonl);
    let id = distill_preview(td.path())
        .expect("preview")
        .duplicate_groups[0]
        .id
        .clone();
    let ok = unwrap_distill_apply_ok(distill_apply(td.path(), &[id.clone(), id]));
    assert_eq!(
        ok.removed_entry_count, 1,
        "duplicated id must remove once, not twice"
    );
    assert_eq!(ok.remaining_entry_count, 1);
    assert!(ok.unmatched_group_ids.is_empty());
}

// ─── D80: in-place edit (memory.update) ─────────────────────────────────

fn unwrap_update_ok(resp: MemoryUpdateResponse) -> MemoryUpdateOk {
    match resp {
        MemoryUpdateResponse::Ok(ok) => ok,
        MemoryUpdateResponse::Err(e) => {
            panic!(
                "expected ok, got err: reason={:?} msg={:?}",
                e.reason, e.message
            )
        }
    }
}

#[test]
fn update_changes_text_and_preserves_id_and_created_ms() {
    let td = TempDir::new("d80-edit");
    let root = canon_root(&td);
    let original = unwrap_ok(remember(&root, "verify with cargo test"));

    let ok = unwrap_update_ok(update(
        &root,
        &original.entry.id,
        "verify with `cargo test`",
    ));
    assert_eq!(ok.entry.id, original.entry.id, "id preserved");
    assert_eq!(
        ok.entry.created_ms, original.entry.created_ms,
        "created_ms preserved"
    );
    assert_eq!(ok.entry.text, "verify with `cargo test`");

    // Reflected on disk, still a single entry.
    let index = read_index(&root).unwrap();
    assert_eq!(index.entries.len(), 1);
    assert_eq!(index.entries[0].text, "verify with `cargo test`");
    assert_eq!(index.entries[0].id, original.entry.id);
}

#[test]
fn update_re_redacts_secrets_in_new_text() {
    let td = TempDir::new("d80-redact");
    let root = canon_root(&td);
    let original = unwrap_ok(remember(&root, "no secret here"));

    let ok = unwrap_update_ok(update(
        &root,
        &original.entry.id,
        "staging key is sk-abcdefghij0123456789xyzABCDEFGH",
    ));
    assert!(!ok.entry.text.contains("sk-abcdefg"));
    assert!(ok.entry.text.contains("[REDACTED:api-key]"));
    assert_eq!(ok.entry.redaction_count, 1);
}

#[test]
fn update_not_found_for_wellformed_missing_id() {
    let td = TempDir::new("d80-notfound");
    let root = canon_root(&td);
    unwrap_ok(remember(&root, "present"));

    match update(&root, "m_00000000000000000000000000000000", "x") {
        MemoryUpdateResponse::Err(e) => assert_eq!(e.reason, MemoryUpdateFailure::NotFound),
        MemoryUpdateResponse::Ok(_) => panic!("expected NotFound"),
    }
}

#[test]
fn update_rejects_bad_id_shape() {
    let td = TempDir::new("d80-badid");
    let root = canon_root(&td);
    for bad in [
        "",
        "abc",
        "m_short",
        "m_../escape",
        "x_00000000000000000000000000000000",
    ] {
        match update(&root, bad, "x") {
            MemoryUpdateResponse::Err(e) => {
                assert_eq!(e.reason, MemoryUpdateFailure::BadId, "for {bad:?}")
            }
            MemoryUpdateResponse::Ok(_) => panic!("expected BadId for {bad:?}"),
        }
    }
}

#[test]
fn update_rejects_empty_and_too_long() {
    let td = TempDir::new("d80-validate");
    let root = canon_root(&td);
    let original = unwrap_ok(remember(&root, "seed"));

    match update(&root, &original.entry.id, "   \n\t ") {
        MemoryUpdateResponse::Err(e) => assert_eq!(e.reason, MemoryUpdateFailure::Empty),
        MemoryUpdateResponse::Ok(_) => panic!("expected Empty"),
    }
    let oversize = "x".repeat(MAX_BYTES_PER_ENTRY + 1);
    match update(&root, &original.entry.id, &oversize) {
        MemoryUpdateResponse::Err(e) => assert_eq!(e.reason, MemoryUpdateFailure::TooLong),
        MemoryUpdateResponse::Ok(_) => panic!("expected TooLong"),
    }
    // The entry is unchanged after rejected edits.
    let index = read_index(&root).unwrap();
    assert_eq!(index.entries[0].text, "seed");
}

#[test]
fn update_refuses_symlinked_plume_dir() {
    #[cfg(unix)]
    {
        let td = TempDir::new("d80-symlink");
        let real = td.path().join("not_plume");
        fs::create_dir_all(&real).unwrap();
        std::os::unix::fs::symlink(&real, td.path().join(".plume")).unwrap();
        match update(td.path(), "m_00000000000000000000000000000000", "x") {
            MemoryUpdateResponse::Err(e) => assert_eq!(e.reason, MemoryUpdateFailure::StoreFailed),
            MemoryUpdateResponse::Ok(_) => panic!("symlinked .plume must refuse"),
        }
    }
}

// ─── Distill link-merge into the newest survivor ───────────────────────
//
// When exact-duplicate entries are compacted, topic links held only by a
// removed duplicate must not vanish — they fold (deduplicated, sorted,
// capped) into the newest survivor. Over-cap unions are surfaced as a
// conflict and the group is left completely unchanged. These pin the
// seven behaviors the merge must guarantee.

const ID_A: &str = "m_a0000000000000000000000000000000";
const ID_B: &str = "m_b0000000000000000000000000000000";
const ID_C: &str = "m_c0000000000000000000000000000000";
const ID_D: &str = "m_d0000000000000000000000000000000";

fn merge_entry(id: &str, created_ms: u64, text: &str, links: &[&str]) -> String {
    let links_json = links
        .iter()
        .map(|l| format!("{l:?}"))
        .collect::<Vec<_>>()
        .join(",");
    format!(
        r#"{{"id":"{id}","createdMs":{created_ms},"text":"{text}","redactionCount":0,"links":[{links_json}]}}"#
    )
}

fn seed_merge(root: &Path, entries: &[String]) {
    let jsonl: String = entries.iter().map(|e| format!("{e}\n")).collect();
    write_distill_fixtures(root, &jsonl);
}

fn entries_bytes(root: &Path) -> String {
    fs::read_to_string(root.join(".plume/memory/entries.jsonl")).unwrap()
}

fn find_survivor(root: &Path, id: &str) -> MemoryEntry {
    read_index(root)
        .expect("index")
        .entries
        .into_iter()
        .find(|e| e.id == id)
        .expect("survivor present")
}

// (1) Links held only by the SURVIVOR are kept and, since nothing was
// transferred in, produce no link-merge audit record.
#[test]
fn distill_merge_keeps_survivor_only_links_without_recording_a_transfer() {
    let td = TempDir::new("d-merge-survivor-only");
    // Newest B carries the link; older A has none.
    seed_merge(
        td.path(),
        &[
            merge_entry(ID_A, 100, "same fact", &[]),
            merge_entry(ID_B, 200, "same fact", &["topics/keep.md"]),
        ],
    );

    let group = distill_preview(td.path())
        .expect("preview")
        .duplicate_groups[0]
        .clone();
    assert_eq!(group.merged_links, vec!["topics/keep.md".to_string()]);
    assert!(!group.link_cap_exceeded);

    let ok = unwrap_distill_apply_ok(distill_apply(td.path(), &[group.id]));
    assert_eq!(ok.removed_entry_count, 1);
    assert!(ok.conflicted_group_ids.is_empty());

    assert_eq!(find_survivor(td.path(), ID_B).links, vec!["topics/keep.md"]);
    let log = read_distill_log(td.path()).expect("log");
    assert_eq!(log.len(), 1);
    assert!(
        log[0].link_merges.is_empty(),
        "no inbound transfer, so no merge record"
    );
}

// (2) A link held ONLY by a removed duplicate folds into the newest
// survivor and is recorded in the audit trail.
#[test]
fn distill_merge_folds_removed_only_links_into_survivor_and_records_it() {
    let td = TempDir::new("d-merge-removed-only");
    // Newest B has no links; older A carries the only link.
    seed_merge(
        td.path(),
        &[
            merge_entry(ID_A, 100, "same fact", &["topics/from-removed.md"]),
            merge_entry(ID_B, 200, "same fact", &[]),
        ],
    );

    let group = distill_preview(td.path())
        .expect("preview")
        .duplicate_groups[0]
        .clone();
    assert_eq!(
        group.merged_links,
        vec!["topics/from-removed.md".to_string()]
    );

    let ok = unwrap_distill_apply_ok(distill_apply(td.path(), &[group.id]));
    assert_eq!(ok.removed_entry_count, 1);

    assert_eq!(
        find_survivor(td.path(), ID_B).links,
        vec!["topics/from-removed.md"]
    );
    let log = read_distill_log(td.path()).expect("log");
    assert_eq!(log[0].link_merges.len(), 1);
    assert_eq!(log[0].link_merges[0].survivor_id, ID_B);
    assert_eq!(log[0].link_merges[0].links, vec!["topics/from-removed.md"]);
}

// (3) Overlapping links across the group union without duplication.
#[test]
fn distill_merge_deduplicates_overlapping_links() {
    let td = TempDir::new("d-merge-overlap");
    seed_merge(
        td.path(),
        &[
            merge_entry(ID_A, 100, "same fact", &["topics/b.md", "topics/c.md"]),
            merge_entry(ID_B, 200, "same fact", &["topics/a.md", "topics/b.md"]),
        ],
    );

    let group = distill_preview(td.path())
        .expect("preview")
        .duplicate_groups[0]
        .clone();
    assert_eq!(
        group.merged_links,
        vec![
            "topics/a.md".to_string(),
            "topics/b.md".to_string(),
            "topics/c.md".to_string()
        ],
    );

    let ok = unwrap_distill_apply_ok(distill_apply(td.path(), &[group.id]));
    assert!(ok.conflicted_group_ids.is_empty());
    assert_eq!(
        find_survivor(td.path(), ID_B).links,
        vec!["topics/a.md", "topics/b.md", "topics/c.md"]
    );
}

// (4) The union is deterministically sorted regardless of input order,
// and identical across repeated previews.
#[test]
fn distill_merge_orders_links_deterministically() {
    let td = TempDir::new("d-merge-order");
    // Deliberately unsorted, split across entries.
    seed_merge(
        td.path(),
        &[
            merge_entry(ID_A, 100, "same fact", &["topics/z.md", "topics/m.md"]),
            merge_entry(ID_B, 200, "same fact", &["topics/a.md"]),
        ],
    );

    let first = distill_preview(td.path())
        .expect("preview")
        .duplicate_groups[0]
        .merged_links
        .clone();
    let second = distill_preview(td.path())
        .expect("preview")
        .duplicate_groups[0]
        .merged_links
        .clone();
    assert_eq!(
        first,
        vec![
            "topics/a.md".to_string(),
            "topics/m.md".to_string(),
            "topics/z.md".to_string()
        ],
    );
    assert_eq!(
        first, second,
        "merged link order must be stable across calls"
    );
}

// (5) When the union would exceed the per-entry cap, the group is
// refused: no removal, no link write, byte-identical store, and the id
// surfaces in conflicted_group_ids.
#[test]
fn distill_merge_over_cap_refuses_group_without_mutating_the_store() {
    let td = TempDir::new("d-merge-over-cap");
    // 3 + 3 distinct links = 6 > MAX_LINKS (5).
    seed_merge(
        td.path(),
        &[
            merge_entry(
                ID_A,
                100,
                "same fact",
                &["topics/1.md", "topics/2.md", "topics/3.md"],
            ),
            merge_entry(
                ID_B,
                200,
                "same fact",
                &["topics/4.md", "topics/5.md", "topics/6.md"],
            ),
        ],
    );

    let group = distill_preview(td.path())
        .expect("preview")
        .duplicate_groups[0]
        .clone();
    assert!(group.link_cap_exceeded);
    assert_eq!(group.merged_links.len(), 6);
    assert_eq!(
        distill_preview(td.path()).expect("preview").would_remove,
        0,
        "a conflicted group contributes nothing to wouldRemove"
    );

    let before = entries_bytes(td.path());
    let ok = unwrap_distill_apply_ok(distill_apply(td.path(), std::slice::from_ref(&group.id)));
    assert_eq!(ok.removed_entry_count, 0);
    assert_eq!(ok.remaining_entry_count, 2);
    assert_eq!(ok.conflicted_group_ids, vec![group.id]);
    assert!(ok.unmatched_group_ids.is_empty());
    assert_eq!(
        entries_bytes(td.path()),
        before,
        "a refused group must leave the store byte-identical"
    );
    assert!(
        read_distill_log(td.path()).expect("log").is_empty(),
        "no compaction happened, so no audit record"
    );
}

// (6) Applying one group never disturbs another unselected group or its
// links.
#[test]
fn distill_merge_leaves_unselected_groups_and_their_links_untouched() {
    let td = TempDir::new("d-merge-unselected");
    seed_merge(
        td.path(),
        &[
            merge_entry(ID_A, 100, "alpha", &["topics/a1.md"]),
            merge_entry(ID_B, 200, "alpha", &["topics/a2.md"]),
            merge_entry(ID_C, 100, "beta", &["topics/b1.md"]),
            merge_entry(ID_D, 200, "beta", &["topics/b2.md"]),
        ],
    );

    let preview = distill_preview(td.path()).expect("preview");
    let alpha = preview
        .duplicate_groups
        .iter()
        .find(|g| g.entries[0].text == "alpha")
        .expect("alpha group");

    let ok = unwrap_distill_apply_ok(distill_apply(td.path(), std::slice::from_ref(&alpha.id)));
    assert_eq!(ok.removed_entry_count, 1);

    // Alpha survivor (B) merged its links; A removed.
    assert_eq!(
        find_survivor(td.path(), ID_B).links,
        vec!["topics/a1.md", "topics/a2.md"]
    );
    let index = read_index(td.path()).expect("index");
    assert!(!index.entries.iter().any(|e| e.id == ID_A));
    // Beta group entirely untouched: both entries and their own links.
    assert_eq!(find_survivor(td.path(), ID_C).links, vec!["topics/b1.md"]);
    assert_eq!(find_survivor(td.path(), ID_D).links, vec!["topics/b2.md"]);
}

// (7) The audit record captures removed ids, the kept survivor, and the
// exact merged link set for every group that transferred links.
#[test]
fn distill_merge_audit_record_reports_removed_kept_and_merged_links() {
    let td = TempDir::new("d-merge-audit");
    seed_merge(
        td.path(),
        &[
            merge_entry(ID_A, 100, "same fact", &["topics/x.md"]),
            merge_entry(ID_B, 200, "same fact", &["topics/y.md"]),
        ],
    );

    let group = distill_preview(td.path())
        .expect("preview")
        .duplicate_groups[0]
        .clone();
    let _ = unwrap_distill_apply_ok(distill_apply(td.path(), &[group.id]));

    let log = read_distill_log(td.path()).expect("log");
    assert_eq!(log.len(), 1);
    assert_eq!(log[0].rule, "dedupeExact");
    assert_eq!(log[0].removed_ids, vec![ID_A]);
    assert_eq!(log[0].kept_ids, vec![ID_B]);
    assert_eq!(log[0].link_merges.len(), 1);
    assert_eq!(log[0].link_merges[0].survivor_id, ID_B);
    // Newest survivor's link plus the removed duplicate's link, sorted.
    assert_eq!(
        log[0].link_merges[0].links,
        vec!["topics/x.md".to_string(), "topics/y.md".to_string()]
    );
}

// (9) A link edit that leaves group MEMBERSHIP unchanged still
// invalidates the confirmed group id. Links now steer the apply
// outcome (survivor inheritance, over-cap conflict), so a preview
// taken before the edit must not silently apply against the new link
// state: the stale id is a reported no-op that mutates nothing
// (Codex #138 P2-1).
#[test]
fn distill_merge_link_edit_invalidates_group_id_so_stale_apply_is_a_noop() {
    let td = TempDir::new("d-merge-link-edit");
    seed_merge(
        td.path(),
        &[
            merge_entry(ID_A, 100, "same fact", &["topics/a.md"]),
            merge_entry(ID_B, 200, "same fact", &[]),
        ],
    );

    // Preview the group and capture the id the user would confirm.
    let stale_id = distill_preview(td.path())
        .expect("preview")
        .duplicate_groups[0]
        .id
        .clone();

    // Edit links on a member without touching membership: give the
    // survivor a new link. The two ids and the normalized text are
    // identical to before — only the link set differs.
    seed_merge(
        td.path(),
        &[
            merge_entry(ID_A, 100, "same fact", &["topics/a.md"]),
            merge_entry(ID_B, 200, "same fact", &["topics/b.md"]),
        ],
    );

    // The re-scanned group carries a NEW id...
    let fresh_id = distill_preview(td.path())
        .expect("preview")
        .duplicate_groups[0]
        .id
        .clone();
    assert_ne!(stale_id, fresh_id, "a link edit must change the group id");

    // ...so applying the stale id removes nothing, reports it as
    // unmatched, and leaves the store byte-identical.
    let before = entries_bytes(td.path());
    let ok = unwrap_distill_apply_ok(distill_apply(td.path(), std::slice::from_ref(&stale_id)));
    assert_eq!(ok.removed_entry_count, 0);
    assert_eq!(ok.unmatched_group_ids, vec![stale_id]);
    assert!(ok.conflicted_group_ids.is_empty());
    assert_eq!(
        before,
        entries_bytes(td.path()),
        "a stale-id apply must not rewrite the store"
    );
}
