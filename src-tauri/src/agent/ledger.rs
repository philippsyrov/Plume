//! Persistent approval ledger (D83) — agent-loop slice 2b.
//!
//! The on-disk follow-up to the in-memory decision core (`agent::approval`,
//! D78). Records the command identities the user has approved so a
//! later `ask-on-write` / `ask-on-fail` run can skip the prompt — and,
//! critically, **re-prompts when the resolved binary changed** under an
//! approved name (`docs/SAFETY.md § Argv normalization` / `§ Approval
//! ledger`).
//!
//! Format choice: stored as JSON at `<project>/.plume/approvals.json`
//! via `serde_json` (already a dependency) rather than the TOML the
//! SAFETY doc illustrates, to avoid pulling in a `toml` / date crate
//! (no new downloads). The record schema is preserved; timestamps are
//! Unix epoch milliseconds, matching the memory store. The file is
//! human-readable and hand-editable.
//!
//! Safety properties:
//! - **Symlink-safe.** Refuses a symlinked `.plume` directory or
//!   `approvals.json` file before reading or writing — same guard the
//!   memory store uses.
//! - **Env-wrapper rejection.** Reuses `approval::normalize_command`, so
//!   `env A=1 cmd` and `KEY=VAL`-leading argv can never be approved.
//! - **Binary-match.** An approval records the resolved absolute binary
//!   path; a lookup re-resolves and reports `BinaryMismatch` if it moved.
//! - **Expiry.** Each record carries `expires_ms` (90-day default;
//!   `None` = no expiry); a lookup past it reports `Expired`.
//! - **Corrupt-ledger recovery.** A malformed file is treated as empty
//!   (fail-safe: nothing is approved, everything re-prompts) and never
//!   panics; the next write replaces it.
//!
//! Pure-ish + testable: PATH resolution is abstracted behind
//! [`BinaryResolver`] so tests inject a deterministic map. No IPC, no
//! execution, no consumer yet — the loop controller (slice 4) and a
//! future `approvals.*` verb wire it up, hence `allow(dead_code)`.

#![allow(dead_code)]

use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use super::approval::normalize_command;

/// Default approval lifetime: 90 days, per `docs/SAFETY.md § Approval
/// ledger`. The user (later, via the approvals UI) can override or clear
/// per record; `expires_ms: None` means "no expiry".
pub const DEFAULT_EXPIRY_MS: u64 = 90 * 24 * 60 * 60 * 1000;

/// Cap on stored approvals. A project with more approved commands than
/// this is well past a normal coding workflow.
pub const MAX_RECORDS: usize = 256;

const APPROVALS_FILE_NAME: &str = "approvals.json";

/// One approved command identity. Serialized as camelCase JSON to match
/// the rest of the codebase's serde convention.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApprovalRecord {
    /// Normalized argv (verbatim, trailing args kept). The match key.
    pub argv: Vec<String>,
    /// Basename of the resolved binary (e.g. `"npm"`). Carried for
    /// display and as a secondary identity per the SAFETY doc.
    pub basename: String,
    /// Absolute path the program token resolved to at approval time. A
    /// later run that resolves to a different path re-prompts.
    pub binary: String,
    /// Unix epoch ms when first approved.
    pub created_ms: u64,
    /// Unix epoch ms of the most recent (re-)approval.
    pub updated_ms: u64,
    /// Unix epoch ms after which the approval is stale. `None` = never
    /// expires. Schema room is intentional: the approvals UI sets it.
    pub expires_ms: Option<u64>,
    /// Who approved it. `"user"` today; `"agent"` is reserved for a
    /// future self-approval path and is NOT honored yet.
    pub approved_by: String,
}

/// The result of looking a command up in the ledger.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApprovalLookup {
    /// In the ledger, not expired, and the binary still matches.
    Approved,
    /// No record for this argv.
    NotApproved,
    /// A record exists but is past `expires_ms` — re-prompt.
    Expired,
    /// A record exists but the program now resolves to a different
    /// binary (or no longer resolves) — re-prompt. Carries both paths
    /// for an honest diagnostic.
    BinaryMismatch {
        recorded: String,
        current: Option<String>,
    },
    /// The argv could not even be normalized (env wrapper / empty /
    /// blank program) — it is not approvable.
    RejectedWrapper,
}

#[derive(Debug)]
pub struct LedgerError(pub String);

impl std::fmt::Display for LedgerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for LedgerError {}

/// Resolves a program token (argv[0]) to its absolute binary path.
/// Abstracted so the persistent ledger is testable without a real
/// `PATH` / filesystem.
pub trait BinaryResolver {
    fn resolve(&self, program: &str) -> Option<PathBuf>;
}

/// Production resolver: an explicit path is canonicalized directly;
/// otherwise the program is searched along `PATH`. Returns the
/// canonical absolute path (symlinks resolved), or `None`.
pub struct PathResolver;

impl BinaryResolver for PathResolver {
    fn resolve(&self, program: &str) -> Option<PathBuf> {
        if program.is_empty() {
            return None;
        }
        if program.contains('/') || program.contains('\\') {
            return std::fs::canonicalize(program).ok().filter(|p| p.is_file());
        }
        let path_var = std::env::var_os("PATH")?;
        for dir in std::env::split_paths(&path_var) {
            let candidate = dir.join(program);
            if candidate.is_file() {
                if let Ok(canon) = std::fs::canonicalize(&candidate) {
                    return Some(canon);
                }
            }
        }
        None
    }
}

/// Current wall-clock in epoch ms. Pure ledger functions take `now_ms`
/// explicitly so expiry is testable; production callers pass this.
pub fn current_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

// ─── Public API ─────────────────────────────────────────────────────────

/// Approve a command. Normalizes + reject-wraps the argv, resolves the
/// binary (error if not found), and upserts the record (a re-approval
/// refreshes `updated_ms` + `expires_ms`, keeping `created_ms`). Returns
/// the stored record.
pub fn approve(
    project_root: &Path,
    argv: &[String],
    resolver: &dyn BinaryResolver,
    now_ms: u64,
) -> Result<ApprovalRecord, LedgerError> {
    let normalized = normalize_command(argv)
        .map_err(|e| LedgerError(format!("cannot approve: argv rejected ({e:?})")))?;
    let binary = resolver.resolve(&normalized.argv[0]).ok_or_else(|| {
        LedgerError(format!(
            "binary not found on PATH: {:?}",
            normalized.argv[0]
        ))
    })?;
    let binary_str = binary.to_string_lossy().into_owned();
    let basename = binary
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();

    let mut records = read_store(project_root)?;
    if let Some(pos) = records.iter().position(|r| r.argv == normalized.argv) {
        let rec = &mut records[pos];
        rec.binary = binary_str;
        rec.basename = basename;
        rec.updated_ms = now_ms;
        rec.expires_ms = Some(now_ms.saturating_add(DEFAULT_EXPIRY_MS));
        let out = rec.clone();
        write_store(project_root, &records)?;
        return Ok(out);
    }

    if records.len() >= MAX_RECORDS {
        return Err(LedgerError(format!(
            "approval ledger holds {} records; max is {MAX_RECORDS}",
            records.len()
        )));
    }
    let record = ApprovalRecord {
        argv: normalized.argv,
        basename,
        binary: binary_str,
        created_ms: now_ms,
        updated_ms: now_ms,
        expires_ms: Some(now_ms.saturating_add(DEFAULT_EXPIRY_MS)),
        approved_by: "user".to_string(),
    };
    records.push(record.clone());
    write_store(project_root, &records)?;
    Ok(record)
}

/// Look a command up. Returns the typed [`ApprovalLookup`]; only
/// `Approved` should let a run skip the prompt. Re-resolves the binary
/// to catch a moved/replaced program.
pub fn lookup(
    project_root: &Path,
    argv: &[String],
    resolver: &dyn BinaryResolver,
    now_ms: u64,
) -> Result<ApprovalLookup, LedgerError> {
    let normalized = match normalize_command(argv) {
        Ok(n) => n,
        Err(_) => return Ok(ApprovalLookup::RejectedWrapper),
    };
    let records = read_store(project_root)?;
    let Some(record) = records.iter().find(|r| r.argv == normalized.argv) else {
        return Ok(ApprovalLookup::NotApproved);
    };
    if let Some(exp) = record.expires_ms {
        if now_ms >= exp {
            return Ok(ApprovalLookup::Expired);
        }
    }
    match resolver.resolve(&normalized.argv[0]) {
        Some(current) => {
            let current_str = current.to_string_lossy().into_owned();
            if current_str == record.binary {
                Ok(ApprovalLookup::Approved)
            } else {
                Ok(ApprovalLookup::BinaryMismatch {
                    recorded: record.binary.clone(),
                    current: Some(current_str),
                })
            }
        }
        None => Ok(ApprovalLookup::BinaryMismatch {
            recorded: record.binary.clone(),
            current: None,
        }),
    }
}

/// Remove the approval for `argv`. Returns whether a record was present.
/// A non-normalizable argv (env wrapper) is a no-op `false`.
pub fn revoke(project_root: &Path, argv: &[String]) -> Result<bool, LedgerError> {
    let normalized = match normalize_command(argv) {
        Ok(n) => n,
        Err(_) => return Ok(false),
    };
    let mut records = read_store(project_root)?;
    let before = records.len();
    records.retain(|r| r.argv != normalized.argv);
    let removed = records.len() != before;
    if removed {
        write_store(project_root, &records)?;
    }
    Ok(removed)
}

/// Read all approvals (for an inspection / approvals UI later).
pub fn list(project_root: &Path) -> Result<Vec<ApprovalRecord>, LedgerError> {
    read_store(project_root)
}

// ─── Internals ──────────────────────────────────────────────────────────

fn read_store(project_root: &Path) -> Result<Vec<ApprovalRecord>, LedgerError> {
    let path = resolve_approvals_path(project_root)?;
    let raw = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(LedgerError(format!("read {}: {}", path.display(), e))),
    };
    if raw.trim().is_empty() {
        return Ok(Vec::new());
    }
    match serde_json::from_str::<Vec<ApprovalRecord>>(&raw) {
        Ok(records) => Ok(records),
        Err(e) => {
            // Corrupt-ledger recovery: a malformed file never grants
            // approval. Treat as empty (fail-safe) and warn; the next
            // write replaces the unusable file. We do NOT delete it here
            // so a human can still recover the bytes manually.
            tracing::warn!(error = %e, path = %path.display(), "approvals ledger is corrupt; treating as empty");
            Ok(Vec::new())
        }
    }
}

fn write_store(project_root: &Path, records: &[ApprovalRecord]) -> Result<(), LedgerError> {
    let plume_dir = project_root.join(".plume");
    refuse_symlink(&plume_dir, ".plume")?;
    std::fs::create_dir_all(&plume_dir).map_err(|e| LedgerError(format!("create .plume/: {e}")))?;
    let path = plume_dir.join(APPROVALS_FILE_NAME);
    refuse_symlink(&path, ".plume/approvals.json")?;
    let json = serde_json::to_string_pretty(records)
        .map_err(|e| LedgerError(format!("serialise approvals: {e}")))?;
    write_atomic(&path, json.as_bytes())
}

/// Symlink-safe path to `<root>/.plume/approvals.json`. Refuses a
/// symlinked `.plume` directory or `approvals.json` file (a planted
/// symlink could otherwise redirect a read/write outside the project).
fn resolve_approvals_path(project_root: &Path) -> Result<PathBuf, LedgerError> {
    let plume_dir = project_root.join(".plume");
    refuse_symlink(&plume_dir, ".plume")?;
    let path = plume_dir.join(APPROVALS_FILE_NAME);
    refuse_symlink(&path, ".plume/approvals.json")?;
    Ok(path)
}

/// Refuse a pre-existing symlink at `path`. Missing is fine. Local copy
/// of the same guard the memory store uses, kept here so the agent
/// module doesn't depend on a `pub(crate)` from memory.
fn refuse_symlink(path: &Path, label: &str) -> Result<(), LedgerError> {
    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => Err(LedgerError(format!(
            "{label} is a symlink; refusing to touch the approval ledger through it"
        ))),
        Ok(_) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(LedgerError(format!("stat {label}: {e}"))),
    }
}

/// Sibling-tempfile + atomic rename. Mirrors the memory store's writer.
fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), LedgerError> {
    let parent = path
        .parent()
        .ok_or_else(|| LedgerError(format!("approvals path {} has no parent", path.display())))?;
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| LedgerError(format!("approvals path {} has no filename", path.display())))?;
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let tmp = parent.join(format!(".{file_name}.plume-approvals-{nanos}.tmp"));
    {
        let mut f = std::fs::File::create(&tmp)
            .map_err(|e| LedgerError(format!("create temp {}: {e}", tmp.display())))?;
        if let Err(e) = f.write_all(bytes) {
            let _ = std::fs::remove_file(&tmp);
            return Err(LedgerError(format!("write temp {}: {e}", tmp.display())));
        }
        if let Err(e) = f.sync_all() {
            let _ = std::fs::remove_file(&tmp);
            return Err(LedgerError(format!("sync temp {}: {e}", tmp.display())));
        }
    }
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        LedgerError(format!("rename -> {}: {e}", path.display()))
    })
}

/// Test-only deterministic resolver, mapping a program name to a path.
/// Public for the sibling test file; harmless in the binary.
pub struct MapResolver {
    pub map: HashMap<String, PathBuf>,
}

impl MapResolver {
    pub fn new(pairs: &[(&str, &str)]) -> Self {
        Self {
            map: pairs
                .iter()
                .map(|(k, v)| (k.to_string(), PathBuf::from(v)))
                .collect(),
        }
    }
}

impl BinaryResolver for MapResolver {
    fn resolve(&self, program: &str) -> Option<PathBuf> {
        self.map.get(program).cloned()
    }
}

#[cfg(test)]
#[path = "ledger_tests.rs"]
mod tests;
