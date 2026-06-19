//! Tests for the persistent approval ledger (D83). Sibling file via
//! `#[path]` so the production module stays under the decomposition cap.
//!
//! PATH resolution is injected through [`MapResolver`] so every case is
//! deterministic — no real `PATH`, no real binaries. The store itself is
//! exercised against a throwaway temp directory.

use super::*;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Self-cleaning temp directory, mirroring the pattern the provider and
/// memory tests use (no `tempfile` dev-dependency in this crate).
struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(label: &str) -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "plume-ledger-{}-{}-{}",
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

    /// Raw bytes of the on-disk ledger file (panics if absent).
    fn read_ledger_file(&self) -> String {
        fs::read_to_string(self.path.join(".plume").join("approvals.json"))
            .expect("ledger file exists")
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn argv(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|s| s.to_string()).collect()
}

// A fixed clock so expiry math is exact and independent of wall time.
const T0: u64 = 1_700_000_000_000;

// ─── first approval ─────────────────────────────────────────────────────

#[test]
fn first_approval_records_identity_and_lookups_as_approved() {
    let td = TempDir::new("first");
    let resolver = MapResolver::new(&[("npm", "/usr/bin/npm")]);
    let cmd = argv(&["npm", "test"]);

    let rec = approve(td.path(), &cmd, &resolver, T0).expect("approve");
    assert_eq!(rec.argv, cmd);
    assert_eq!(rec.basename, "npm");
    assert_eq!(rec.binary, "/usr/bin/npm");
    assert_eq!(rec.created_ms, T0);
    assert_eq!(rec.updated_ms, T0);
    assert_eq!(rec.expires_ms, Some(T0 + DEFAULT_EXPIRY_MS));
    assert_eq!(rec.approved_by, "user");

    // A lookup a moment later, well before expiry, is Approved.
    assert_eq!(
        lookup(td.path(), &cmd, &resolver, T0 + 1_000).expect("lookup"),
        ApprovalLookup::Approved
    );

    // The store has exactly one record and the file is camelCase JSON.
    let listed = list(td.path()).expect("list");
    assert_eq!(listed.len(), 1);
    let raw = td.read_ledger_file();
    assert!(raw.contains("\"createdMs\""), "camelCase keys: {raw}");
    assert!(raw.contains("\"expiresMs\""), "camelCase keys: {raw}");
}

#[test]
fn unknown_command_is_not_approved() {
    let td = TempDir::new("unknown");
    let resolver = MapResolver::new(&[("npm", "/usr/bin/npm")]);
    assert_eq!(
        lookup(td.path(), &argv(&["cargo", "test"]), &resolver, T0).expect("lookup"),
        ApprovalLookup::NotApproved
    );
}

// ─── reload (persist across a fresh read) ───────────────────────────────

#[test]
fn approval_persists_across_reload() {
    let td = TempDir::new("reload");
    let resolver = MapResolver::new(&[("cargo", "/usr/bin/cargo")]);
    let cmd = argv(&["cargo", "test"]);
    approve(td.path(), &cmd, &resolver, T0).expect("approve");

    // Every public read re-reads the file from scratch — there is no
    // in-process cache — so a plain `list` / `lookup` proves persistence.
    let reloaded = list(td.path()).expect("reload");
    assert_eq!(reloaded.len(), 1);
    assert_eq!(reloaded[0].argv, cmd);
    assert_eq!(
        lookup(td.path(), &cmd, &resolver, T0 + 1).expect("lookup"),
        ApprovalLookup::Approved
    );
}

#[test]
fn reapproval_keeps_created_refreshes_updated_and_expiry() {
    let td = TempDir::new("reapprove");
    let resolver = MapResolver::new(&[("npm", "/usr/bin/npm")]);
    let cmd = argv(&["npm", "test"]);
    approve(td.path(), &cmd, &resolver, T0).expect("first");

    let later = T0 + 5_000;
    let rec = approve(td.path(), &cmd, &resolver, later).expect("second");
    assert_eq!(rec.created_ms, T0, "created stays at first approval");
    assert_eq!(rec.updated_ms, later, "updated moves forward");
    assert_eq!(rec.expires_ms, Some(later + DEFAULT_EXPIRY_MS));

    // Still a single record, not a duplicate.
    assert_eq!(list(td.path()).expect("list").len(), 1);
}

// ─── binary mismatch ────────────────────────────────────────────────────

#[test]
fn binary_mismatch_reprompts() {
    let td = TempDir::new("mismatch");
    let cmd = argv(&["npm", "test"]);
    // Approved when npm lived in /usr/bin.
    let approve_resolver = MapResolver::new(&[("npm", "/usr/bin/npm")]);
    approve(td.path(), &cmd, &approve_resolver, T0).expect("approve");

    // Now the same name resolves to a different absolute path (PATH
    // shadowing / a planted binary) — must re-prompt, not silently allow.
    let shifted_resolver = MapResolver::new(&[("npm", "/tmp/evil/npm")]);
    assert_eq!(
        lookup(td.path(), &cmd, &shifted_resolver, T0 + 1).expect("lookup"),
        ApprovalLookup::BinaryMismatch {
            recorded: "/usr/bin/npm".to_string(),
            current: Some("/tmp/evil/npm".to_string()),
        }
    );
}

#[test]
fn binary_no_longer_resolvable_reprompts() {
    let td = TempDir::new("gone");
    let cmd = argv(&["npm", "test"]);
    let approve_resolver = MapResolver::new(&[("npm", "/usr/bin/npm")]);
    approve(td.path(), &cmd, &approve_resolver, T0).expect("approve");

    // Empty resolver: the program no longer resolves at all.
    let empty_resolver = MapResolver::new(&[]);
    assert_eq!(
        lookup(td.path(), &cmd, &empty_resolver, T0 + 1).expect("lookup"),
        ApprovalLookup::BinaryMismatch {
            recorded: "/usr/bin/npm".to_string(),
            current: None,
        }
    );
}

// ─── revoke ─────────────────────────────────────────────────────────────

#[test]
fn revoke_removes_then_is_noop() {
    let td = TempDir::new("revoke");
    let resolver = MapResolver::new(&[("npm", "/usr/bin/npm")]);
    let cmd = argv(&["npm", "test"]);
    approve(td.path(), &cmd, &resolver, T0).expect("approve");

    assert!(
        revoke(td.path(), &cmd).expect("revoke"),
        "first revoke removes"
    );
    assert_eq!(
        lookup(td.path(), &cmd, &resolver, T0 + 1).expect("lookup"),
        ApprovalLookup::NotApproved
    );
    assert!(
        !revoke(td.path(), &cmd).expect("revoke again"),
        "second revoke is a no-op false"
    );
}

// ─── env-wrapper rejection ──────────────────────────────────────────────

#[test]
fn approve_rejects_env_wrapper() {
    let td = TempDir::new("wrapper");
    let resolver = MapResolver::new(&[("npm", "/usr/bin/npm")]);
    // `env A=1 npm test` can never be approved.
    let err = approve(
        td.path(),
        &argv(&["env", "A=1", "npm", "test"]),
        &resolver,
        T0,
    )
    .expect_err("env wrapper rejected");
    assert!(
        err.0.contains("rejected"),
        "message mentions rejection: {err}"
    );
    // A leading KEY=VAL token is rejected too.
    assert!(approve(td.path(), &argv(&["FOO=1", "npm"]), &resolver, T0).is_err());
    // And nothing was written.
    assert!(list(td.path()).expect("list").is_empty());
}

#[test]
fn lookup_of_env_wrapper_is_rejected_wrapper() {
    let td = TempDir::new("wrapper-lookup");
    let resolver = MapResolver::new(&[("npm", "/usr/bin/npm")]);
    assert_eq!(
        lookup(td.path(), &argv(&["env", "npm"]), &resolver, T0).expect("lookup"),
        ApprovalLookup::RejectedWrapper
    );
}

#[test]
fn approve_rejects_unresolvable_binary() {
    let td = TempDir::new("noresolve");
    let resolver = MapResolver::new(&[]); // resolves nothing
    let err = approve(td.path(), &argv(&["ghost", "run"]), &resolver, T0)
        .expect_err("unresolvable binary rejected");
    assert!(err.0.contains("not found"), "message: {err}");
}

// ─── expiry ─────────────────────────────────────────────────────────────

#[test]
fn lookup_past_expiry_is_expired() {
    let td = TempDir::new("expiry");
    let resolver = MapResolver::new(&[("npm", "/usr/bin/npm")]);
    let cmd = argv(&["npm", "test"]);
    approve(td.path(), &cmd, &resolver, T0).expect("approve");

    // One ms before expiry: still approved.
    assert_eq!(
        lookup(td.path(), &cmd, &resolver, T0 + DEFAULT_EXPIRY_MS - 1).expect("lookup"),
        ApprovalLookup::Approved
    );
    // Exactly at / past expiry: re-prompt.
    assert_eq!(
        lookup(td.path(), &cmd, &resolver, T0 + DEFAULT_EXPIRY_MS).expect("lookup"),
        ApprovalLookup::Expired
    );
    assert_eq!(
        lookup(td.path(), &cmd, &resolver, T0 + DEFAULT_EXPIRY_MS + 10_000).expect("lookup"),
        ApprovalLookup::Expired
    );
}

// ─── corrupted-ledger recovery ──────────────────────────────────────────

#[test]
fn corrupt_ledger_is_treated_as_empty_and_recovers() {
    let td = TempDir::new("corrupt");
    let resolver = MapResolver::new(&[("npm", "/usr/bin/npm")]);
    let cmd = argv(&["npm", "test"]);

    // Plant a malformed file where the ledger lives.
    let plume = td.path().join(".plume");
    fs::create_dir_all(&plume).expect("mkdir .plume");
    fs::write(plume.join("approvals.json"), b"{ this is not json ]").expect("write garbage");

    // Reads must not panic and must grant nothing (fail-safe).
    assert!(list(td.path()).expect("list recovers").is_empty());
    assert_eq!(
        lookup(td.path(), &cmd, &resolver, T0).expect("lookup recovers"),
        ApprovalLookup::NotApproved
    );

    // The next write replaces the unusable file with a valid one.
    approve(td.path(), &cmd, &resolver, T0).expect("approve over corrupt");
    assert_eq!(list(td.path()).expect("list").len(), 1);
    assert_eq!(
        lookup(td.path(), &cmd, &resolver, T0 + 1).expect("lookup"),
        ApprovalLookup::Approved
    );
}

#[test]
fn empty_file_is_treated_as_empty() {
    let td = TempDir::new("empty-file");
    let plume = td.path().join(".plume");
    fs::create_dir_all(&plume).expect("mkdir .plume");
    fs::write(plume.join("approvals.json"), b"   \n").expect("write blank");
    assert!(list(td.path()).expect("list").is_empty());
}

#[test]
fn missing_store_lists_empty() {
    let td = TempDir::new("missing");
    assert!(list(td.path()).expect("list").is_empty());
}

// ─── symlink refusal ────────────────────────────────────────────────────

#[cfg(unix)]
#[test]
fn refuses_symlinked_plume_dir() {
    use std::os::unix::fs::symlink;
    let td = TempDir::new("symlink-dir");
    // Point `.plume` at some other directory; the ledger must refuse to
    // read or write through it rather than escaping the project root.
    let elsewhere = TempDir::new("symlink-target");
    symlink(elsewhere.path(), td.path().join(".plume")).expect("symlink .plume");

    let resolver = MapResolver::new(&[("npm", "/usr/bin/npm")]);
    let cmd = argv(&["npm", "test"]);
    assert!(list(td.path()).is_err(), "list refuses symlinked .plume");
    assert!(
        approve(td.path(), &cmd, &resolver, T0).is_err(),
        "approve refuses symlinked .plume"
    );
}

#[cfg(unix)]
#[test]
fn refuses_symlinked_ledger_file() {
    use std::os::unix::fs::symlink;
    let td = TempDir::new("symlink-file");
    let plume = td.path().join(".plume");
    fs::create_dir_all(&plume).expect("mkdir .plume");
    // approvals.json itself is a symlink to an outside file.
    let outside = TempDir::new("symlink-file-target");
    let target = outside.path().join("stolen.json");
    fs::write(&target, b"[]").expect("write target");
    symlink(&target, plume.join("approvals.json")).expect("symlink file");

    assert!(list(td.path()).is_err(), "list refuses symlinked file");
}

// ─── max records cap ────────────────────────────────────────────────────

#[test]
fn approve_rejects_past_max_records() {
    let td = TempDir::new("cap");
    // Build a resolver that maps cmd0..=cmdN to distinct paths.
    let names: Vec<String> = (0..MAX_RECORDS).map(|i| format!("cmd{i}")).collect();
    let paths: Vec<String> = (0..MAX_RECORDS)
        .map(|i| format!("/usr/bin/cmd{i}"))
        .collect();
    let pairs: Vec<(&str, &str)> = names
        .iter()
        .zip(paths.iter())
        .map(|(n, p)| (n.as_str(), p.as_str()))
        .collect();
    let resolver = MapResolver::new(&pairs);

    for name in &names {
        approve(td.path(), &argv(&[name]), &resolver, T0).expect("fill to cap");
    }
    assert_eq!(list(td.path()).expect("list").len(), MAX_RECORDS);

    // One more distinct command is refused.
    let extra = MapResolver::new(&[("overflow", "/usr/bin/overflow")]);
    let err = approve(td.path(), &argv(&["overflow"]), &extra, T0).expect_err("cap reached");
    assert!(err.0.contains("max is"), "message: {err}");

    // But re-approving an EXISTING record still works (it's an upsert,
    // not a new slot).
    approve(td.path(), &argv(&[&names[0]]), &resolver, T0 + 1).expect("reapprove at cap");
}
