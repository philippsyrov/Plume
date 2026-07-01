//! D108: on-disk storage helpers for the memory MVP (D37), split out of
//! `mod.rs` by behavior boundary — symlink-safe path resolution, JSONL
//! read/write, and id minting. No verb logic lives here; `mod.rs`'s
//! `remember`/`update`/`forget`/`search`/`read_index`/`read_for_prompt`
//! call into these. Every item is `pub(super)` (visible anywhere under
//! `memory::`, including the `distill`/`topics` siblings that also call
//! into this layer) rather than `pub`, since none of it is part of the
//! crate-level `memory::` API external callers use.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use super::types::{MemoryEntry, MemoryStoreError};
use super::MAX_BYTES_TOTAL;

pub(super) const ENTRIES_FILE_NAME: &str = "entries.jsonl";

/// Symlink-safe entries-path resolver shared by every memory verb
/// (Codex D37 HIGH). Refuses if `.plume/` or `.plume/memory/`
/// exists as a symlink — same guard the patch checkpoint uses.
/// Does NOT create the directories; the missing-path case is
/// fine (read_index / forget treat a missing file as "no
/// entries"). `remember` calls `ensured_entries_path` instead,
/// which adds the `create_dir_all` step on top of this check.
pub(super) fn resolve_entries_path(project_root: &Path) -> Result<PathBuf, MemoryStoreError> {
    resolve_memory_file(project_root, ENTRIES_FILE_NAME)
}

/// Resolve `<root>/.plume/memory/<file_name>` with the same symlink
/// refusal the entries store uses. Shared by the entries store and
/// the D69 distill audit log so both honor the planted-`.plume`
/// symlink guard. Does NOT create directories — read paths tolerate
/// a missing tree; writers `create_dir_all` after this check.
pub(super) fn resolve_memory_file(
    project_root: &Path,
    file_name: &str,
) -> Result<PathBuf, MemoryStoreError> {
    let plume_dir = project_root.join(".plume");
    refuse_symlink(&plume_dir, ".plume")?;
    let memory_dir = plume_dir.join("memory");
    refuse_symlink(&memory_dir, ".plume/memory")?;
    Ok(memory_dir.join(file_name))
}

/// Same as `resolve_entries_path` plus ensures the directories
/// exist. Only `remember` needs the create step; read and forget
/// tolerate a missing tree (treated as empty / no-op).
pub(super) fn ensured_entries_path(project_root: &Path) -> Result<PathBuf, MemoryStoreError> {
    let plume_dir = project_root.join(".plume");
    refuse_symlink(&plume_dir, ".plume")?;
    fs::create_dir_all(&plume_dir)
        .map_err(|e| MemoryStoreError(format!("create .plume/: {}", e)))?;

    let memory_dir = plume_dir.join("memory");
    refuse_symlink(&memory_dir, ".plume/memory")?;
    fs::create_dir_all(&memory_dir)
        .map_err(|e| MemoryStoreError(format!("create .plume/memory/: {}", e)))?;

    Ok(memory_dir.join(ENTRIES_FILE_NAME))
}

/// Reject any pre-existing path that's a symlink — `fs::create_dir_all`
/// would follow it and write memory files outside the project
/// root. Missing (NotFound) is fine; we'll create the path as a
/// regular directory. Local copy of the same guard
/// `patch::checkpoint::ensure_not_symlink` uses; kept local so the
/// memory module doesn't depend on a `pub(crate)` from patch.
pub(super) fn refuse_symlink(path: &Path, label: &str) -> Result<(), MemoryStoreError> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => Err(MemoryStoreError(format!(
            "{label} is a symlink; refusing to write memory through it"
        ))),
        Ok(_) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(MemoryStoreError(format!("stat {label}: {e}"))),
    }
}

/// Read every JSONL line in `path` as a `MemoryEntry`. Missing
/// file → empty list. Malformed lines are dropped (the panel
/// stays usable when a user hand-edits the file and fat-fingers
/// one line). Oversize files are rejected: if the on-disk file is
/// past `MAX_BYTES_TOTAL`, we refuse to parse it rather than
/// surface arbitrarily many entries.
///
/// D81 (Codex review, same class as the distill-log finding): refuse a
/// symlinked `entries.jsonl` before reading. The resolver only refuses
/// a symlinked `.plume` / `.plume/memory` directory; a symlink planted
/// at the final file would otherwise be dereferenced. Every reader
/// (index, prompt, search, distill, update) funnels through here, so
/// this closes the gap for all of them at once.
pub(super) fn read_entries(path: &Path) -> Result<(Vec<MemoryEntry>, u64), MemoryStoreError> {
    refuse_symlink(path, ".plume/memory/entries.jsonl")?;
    let raw = match fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok((Vec::new(), 0));
        }
        Err(e) => {
            return Err(MemoryStoreError(format!("read {}: {}", path.display(), e)));
        }
    };
    let total_bytes = raw.len() as u64;
    if total_bytes > MAX_BYTES_TOTAL {
        return Err(MemoryStoreError(format!(
            "memory store {} is {} bytes; max is {} (delete the file or trim it manually)",
            path.display(),
            total_bytes,
            MAX_BYTES_TOTAL
        )));
    }
    let mut entries = Vec::new();
    for line in raw.lines() {
        if line.trim().is_empty() {
            continue;
        }
        if let Ok(entry) = serde_json::from_str::<MemoryEntry>(line) {
            entries.push(entry);
        }
    }
    Ok((entries, total_bytes))
}

pub(super) fn serialize_entries(entries: &[MemoryEntry]) -> Result<String, MemoryStoreError> {
    let mut out = String::new();
    for e in entries {
        let line = serde_json::to_string(e)
            .map_err(|err| MemoryStoreError(format!("serialise entry: {}", err)))?;
        out.push_str(&line);
        out.push('\n');
    }
    Ok(out)
}

/// Sibling-tempfile + atomic rename. Same pattern as
/// `patch::apply::write_atomic`; reimplemented locally so the
/// memory module doesn't depend on a `pub(crate)` from patch.
pub(super) fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), MemoryStoreError> {
    let parent = path
        .parent()
        .ok_or_else(|| MemoryStoreError(format!("memory path {} has no parent", path.display())))?;
    let file_name = path.file_name().and_then(|n| n.to_str()).ok_or_else(|| {
        MemoryStoreError(format!("memory path {} has no filename", path.display()))
    })?;
    let nanos = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let tmp_path = parent.join(format!(".{}.plume-mem-{}.tmp", file_name, nanos));
    {
        let mut f = fs::File::create(&tmp_path)
            .map_err(|e| MemoryStoreError(format!("create temp {}: {}", tmp_path.display(), e)))?;
        if let Err(e) = f.write_all(bytes) {
            let _ = fs::remove_file(&tmp_path);
            return Err(MemoryStoreError(format!(
                "write temp {}: {}",
                tmp_path.display(),
                e
            )));
        }
        if let Err(e) = f.sync_all() {
            let _ = fs::remove_file(&tmp_path);
            return Err(MemoryStoreError(format!(
                "sync temp {}: {}",
                tmp_path.display(),
                e
            )));
        }
    }
    fs::rename(&tmp_path, path).map_err(|e| {
        let _ = fs::remove_file(&tmp_path);
        MemoryStoreError(format!("rename -> {}: {}", path.display(), e))
    })
}

/// Mint a 32-hex-char entry id. Time-sortable so newest entries
/// sort to the bottom of the file naturally. Same shape and
/// generator as `checkpoint::checkpoint_id` — kept independent so
/// the memory module doesn't depend on a `pub(crate)` from
/// checkpoint.
pub(super) fn mint_entry_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let pid = std::process::id() as u128;
    let combined = (nanos << 32) | (pid & 0xFFFFFFFF);
    format!("m_{:032x}", combined)
}

/// Wire-shape gate for entry ids. Production-minted ids match
/// `m_[0-9a-f]{32}`; rejecting anything else stops a tampered
/// payload from sneaking a path-like id past the trust gate.
pub(super) fn is_valid_entry_id(id: &str) -> bool {
    if id.len() != 34 {
        return false;
    }
    if !id.starts_with("m_") {
        return false;
    }
    id[2..].chars().all(|c| c.is_ascii_hexdigit())
}

pub(super) fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
