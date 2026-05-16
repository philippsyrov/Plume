//! Tests for `memory`. Split into a sibling file via `#[path]` so
//! the production module stays under the decomposition cap.

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
