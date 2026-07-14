//! App-private user memory.
//!
//! This store is physically separate from project memory: Tauri resolves the
//! OS app-data directory once, [`user_memory_dir`] derives its owned child,
//! and IPC callers never supply a path. User entries intentionally have no
//! project-topic links. This module does not perform prompt selection; explicit
//! context integration belongs to a later slice.

use std::collections::HashSet;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::prompts::redact::redact;
use crate::safety::path::{ensure_no_hardlink_alias, PathError};

use super::store::{is_valid_entry_id, mint_entry_id, now_ms};
use super::types::{
    MemoryForgetFailure, MemoryRememberFailure, MemorySearchFailure, MemoryStoreError,
    MemoryUpdateFailure,
};
use super::user_store_lock::acquire_user_memory_process_lock;
use super::{
    MAX_BYTES_PER_ENTRY, MAX_BYTES_TOTAL, MAX_ENTRIES, SEARCH_MAX_LIMIT, SEARCH_MAX_QUERY_BYTES,
};

const ENTRIES_FILE_NAME: &str = "entries.jsonl";

pub fn user_memory_dir(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join("memory")
}

fn user_memory_mutex() -> &'static Mutex<()> {
    static MUTEX: std::sync::OnceLock<Mutex<()>> = std::sync::OnceLock::new();
    MUTEX.get_or_init(|| Mutex::new(()))
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UserMemoryEntry {
    pub id: String,
    pub created_ms: u64,
    pub text: String,
    pub redaction_count: u32,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UserMemoryLimits {
    pub max_entries: u32,
    pub max_bytes_per_entry: u32,
    pub max_bytes_total: u32,
}

impl Default for UserMemoryLimits {
    fn default() -> Self {
        Self {
            max_entries: MAX_ENTRIES as u32,
            max_bytes_per_entry: MAX_BYTES_PER_ENTRY as u32,
            max_bytes_total: MAX_BYTES_TOTAL as u32,
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UserMemoryIndex {
    pub entries: Vec<UserMemoryEntry>,
    pub limits: UserMemoryLimits,
    pub total_bytes: u64,
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum UserMemoryRememberResponse {
    Ok(UserMemoryRememberOk),
    Err(UserMemoryRememberErr),
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UserMemoryRememberOk {
    pub ok: bool,
    pub entry: UserMemoryEntry,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UserMemoryRememberErr {
    pub ok: bool,
    pub reason: MemoryRememberFailure,
    pub message: String,
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum UserMemoryUpdateResponse {
    Ok(UserMemoryUpdateOk),
    Err(UserMemoryUpdateErr),
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UserMemoryUpdateOk {
    pub ok: bool,
    pub entry: UserMemoryEntry,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UserMemoryUpdateErr {
    pub ok: bool,
    pub reason: MemoryUpdateFailure,
    pub message: String,
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum UserMemoryForgetResponse {
    Ok(UserMemoryForgetOk),
    Err(UserMemoryForgetErr),
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UserMemoryForgetOk {
    pub ok: bool,
    pub removed: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UserMemoryForgetErr {
    pub ok: bool,
    pub reason: MemoryForgetFailure,
    pub message: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UserMemorySearchHit {
    pub entry: UserMemoryEntry,
    pub match_count: u32,
    pub first_match_index: u32,
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum UserMemorySearchResponse {
    Ok(UserMemorySearchOk),
    Err(UserMemorySearchErr),
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UserMemorySearchOk {
    pub ok: bool,
    pub hits: Vec<UserMemorySearchHit>,
    pub truncated: bool,
    pub query: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UserMemorySearchErr {
    pub ok: bool,
    pub reason: MemorySearchFailure,
    pub message: String,
}

pub fn read_index(user_memory_dir: &Path) -> Result<UserMemoryIndex, MemoryStoreError> {
    let _guard = user_memory_mutex()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let _process_guard = acquire_user_memory_process_lock(user_memory_dir)?;
    let entries_path = resolve_entries_path(user_memory_dir)?;
    let (entries, total_bytes) = read_entries(&entries_path)?;
    Ok(UserMemoryIndex {
        entries,
        limits: UserMemoryLimits::default(),
        total_bytes,
    })
}

pub fn read_entry_for_prompt(
    user_memory_dir: &Path,
    entry_id: &str,
) -> Result<Option<UserMemoryEntry>, MemoryStoreError> {
    if !is_valid_entry_id(entry_id) {
        return Err(MemoryStoreError("invalid user memory entry id".into()));
    }
    let _guard = user_memory_mutex()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let _process_guard = acquire_user_memory_process_lock(user_memory_dir)?;
    let entries_path = resolve_entries_path(user_memory_dir)?;
    let (entries, _) = read_entries(&entries_path)?;
    Ok(entries.into_iter().find(|entry| entry.id == entry_id))
}

pub fn remember(user_memory_dir: &Path, raw_text: &str) -> UserMemoryRememberResponse {
    let _guard = user_memory_mutex()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let (redacted, redaction_count) = match validate_and_redact(raw_text) {
        Ok(value) => value,
        Err((reason, message)) => return err_remember(reason, message),
    };
    let _process_guard = match acquire_user_memory_process_lock(user_memory_dir) {
        Ok(guard) => guard,
        Err(error) => return err_remember(MemoryRememberFailure::StoreFailed, error.0),
    };
    let entries_path = match ensured_entries_path(user_memory_dir) {
        Ok(path) => path,
        Err(error) => return err_remember(MemoryRememberFailure::StoreFailed, error.0),
    };
    let (mut entries, _) = match read_entries(&entries_path) {
        Ok(value) => value,
        Err(error) => return err_remember(MemoryRememberFailure::StoreFailed, error.0),
    };
    if entries.len() >= MAX_ENTRIES {
        return err_remember(
            MemoryRememberFailure::CapacityReached,
            format!(
                "user memory holds {} entries; max is {MAX_ENTRIES}",
                entries.len()
            ),
        );
    }
    let entry = UserMemoryEntry {
        id: mint_entry_id(),
        created_ms: now_ms(),
        text: redacted,
        redaction_count,
    };
    entries.push(entry.clone());
    let serialized = match serialize_entries(&entries) {
        Ok(value) => value,
        Err(error) => return err_remember(MemoryRememberFailure::StoreFailed, error.0),
    };
    if serialized.len() as u64 > MAX_BYTES_TOTAL {
        return err_remember(
            MemoryRememberFailure::CapacityReached,
            format!(
                "user memory store would be {} bytes; max is {MAX_BYTES_TOTAL}",
                serialized.len()
            ),
        );
    }
    if let Err(error) = write_atomic(&entries_path, serialized.as_bytes()) {
        return err_remember(MemoryRememberFailure::StoreFailed, error.0);
    }
    UserMemoryRememberResponse::Ok(UserMemoryRememberOk { ok: true, entry })
}

pub fn update(user_memory_dir: &Path, entry_id: &str, raw_text: &str) -> UserMemoryUpdateResponse {
    if !is_valid_entry_id(entry_id) {
        return err_update(
            MemoryUpdateFailure::BadId,
            format!("invalid user memory entry id: {entry_id:?}"),
        );
    }
    let _guard = user_memory_mutex()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let _process_guard = match acquire_user_memory_process_lock(user_memory_dir) {
        Ok(guard) => guard,
        Err(error) => return err_update(MemoryUpdateFailure::StoreFailed, error.0),
    };
    let (redacted, redaction_count) = match validate_and_redact(raw_text) {
        Ok(value) => value,
        Err((reason, message)) => {
            return err_update(
                match reason {
                    MemoryRememberFailure::Empty => MemoryUpdateFailure::Empty,
                    MemoryRememberFailure::TooLong => MemoryUpdateFailure::TooLong,
                    MemoryRememberFailure::RedactedToEmpty => MemoryUpdateFailure::RedactedToEmpty,
                    MemoryRememberFailure::CapacityReached => MemoryUpdateFailure::CapacityReached,
                    MemoryRememberFailure::StoreFailed => MemoryUpdateFailure::StoreFailed,
                },
                message,
            );
        }
    };
    let entries_path = match resolve_entries_path(user_memory_dir) {
        Ok(path) => path,
        Err(error) => return err_update(MemoryUpdateFailure::StoreFailed, error.0),
    };
    let (mut entries, _) = match read_entries(&entries_path) {
        Ok(value) => value,
        Err(error) => return err_update(MemoryUpdateFailure::StoreFailed, error.0),
    };
    let Some(position) = entries.iter().position(|entry| entry.id == entry_id) else {
        return err_update(
            MemoryUpdateFailure::NotFound,
            format!("no user memory entry with id {entry_id:?}"),
        );
    };
    entries[position].text = redacted;
    entries[position].redaction_count = redaction_count;
    let updated = entries[position].clone();
    let serialized = match serialize_entries(&entries) {
        Ok(value) => value,
        Err(error) => return err_update(MemoryUpdateFailure::StoreFailed, error.0),
    };
    if serialized.len() as u64 > MAX_BYTES_TOTAL {
        return err_update(
            MemoryUpdateFailure::CapacityReached,
            format!(
                "user memory store would be {} bytes; max is {MAX_BYTES_TOTAL}",
                serialized.len()
            ),
        );
    }
    if let Err(error) = write_atomic(&entries_path, serialized.as_bytes()) {
        return err_update(MemoryUpdateFailure::StoreFailed, error.0);
    }
    UserMemoryUpdateResponse::Ok(UserMemoryUpdateOk {
        ok: true,
        entry: updated,
    })
}

pub fn forget(user_memory_dir: &Path, entry_id: &str) -> UserMemoryForgetResponse {
    if !is_valid_entry_id(entry_id) {
        return UserMemoryForgetResponse::Err(UserMemoryForgetErr {
            ok: false,
            reason: MemoryForgetFailure::BadId,
            message: format!("invalid user memory entry id: {entry_id:?}"),
        });
    }
    let _guard = user_memory_mutex()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let _process_guard = match acquire_user_memory_process_lock(user_memory_dir) {
        Ok(guard) => guard,
        Err(error) => return err_forget(MemoryForgetFailure::StoreFailed, error.0),
    };
    let entries_path = match resolve_entries_path(user_memory_dir) {
        Ok(path) => path,
        Err(error) => return err_forget(MemoryForgetFailure::StoreFailed, error.0),
    };
    let (mut entries, _) = match read_entries(&entries_path) {
        Ok(value) => value,
        Err(error) => return err_forget(MemoryForgetFailure::StoreFailed, error.0),
    };
    let before = entries.len();
    entries.retain(|entry| entry.id != entry_id);
    let removed = entries.len() != before;
    if removed {
        let write_result = if entries.is_empty() {
            match fs::remove_file(&entries_path) {
                Ok(()) => Ok(()),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(error) => Err(MemoryStoreError(format!(
                    "remove {}: {error}",
                    entries_path.display()
                ))),
            }
        } else {
            serialize_entries(&entries)
                .and_then(|serialized| write_atomic(&entries_path, serialized.as_bytes()))
        };
        if let Err(error) = write_result {
            return err_forget(MemoryForgetFailure::StoreFailed, error.0);
        }
    }
    UserMemoryForgetResponse::Ok(UserMemoryForgetOk { ok: true, removed })
}

pub fn search(user_memory_dir: &Path, query: &str, limit: u32) -> UserMemorySearchResponse {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return err_search(
            MemorySearchFailure::EmptyQuery,
            "user memory search query is empty or whitespace-only".to_string(),
        );
    }
    if trimmed.len() > SEARCH_MAX_QUERY_BYTES {
        return err_search(
            MemorySearchFailure::QueryTooLong,
            format!(
                "user memory search query is {} bytes; max is {SEARCH_MAX_QUERY_BYTES}",
                trimmed.len()
            ),
        );
    }
    if limit == 0 || limit > SEARCH_MAX_LIMIT {
        return err_search(
            MemorySearchFailure::BadLimit,
            format!(
                "user memory search limit must be between 1 and {SEARCH_MAX_LIMIT}; got {limit}"
            ),
        );
    }
    let _guard = user_memory_mutex()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let _process_guard = match acquire_user_memory_process_lock(user_memory_dir) {
        Ok(guard) => guard,
        Err(error) => return err_search(MemorySearchFailure::StoreFailed, error.0),
    };
    let entries_path = match resolve_entries_path(user_memory_dir) {
        Ok(path) => path,
        Err(error) => return err_search(MemorySearchFailure::StoreFailed, error.0),
    };
    let entries = match read_entries(&entries_path) {
        Ok((entries, _)) => entries,
        Err(error) => return err_search(MemorySearchFailure::StoreFailed, error.0),
    };
    let needle = trimmed.to_lowercase();
    let mut hits: Vec<UserMemorySearchHit> = entries
        .into_iter()
        .filter_map(|entry| {
            let lower = entry.text.to_lowercase();
            let first_match_index = lower.find(&needle)? as u32;
            let match_count = lower.matches(&needle).count().min(u32::MAX as usize) as u32;
            Some(UserMemorySearchHit {
                entry,
                match_count,
                first_match_index,
            })
        })
        .collect();
    hits.sort_by(|left, right| {
        left.entry
            .text
            .len()
            .cmp(&right.entry.text.len())
            .then_with(|| right.entry.created_ms.cmp(&left.entry.created_ms))
    });
    let truncated = hits.len() > limit as usize;
    hits.truncate(limit as usize);
    UserMemorySearchResponse::Ok(UserMemorySearchOk {
        ok: true,
        hits,
        truncated,
        query: trimmed.to_string(),
    })
}

fn validate_and_redact(raw_text: &str) -> Result<(String, u32), (MemoryRememberFailure, String)> {
    let trimmed = raw_text.trim();
    if trimmed.is_empty() {
        return Err((
            MemoryRememberFailure::Empty,
            "user memory text is empty or whitespace-only".to_string(),
        ));
    }
    if trimmed.len() > MAX_BYTES_PER_ENTRY {
        return Err((
            MemoryRememberFailure::TooLong,
            format!(
                "user memory text is {} bytes; max is {MAX_BYTES_PER_ENTRY}",
                trimmed.len()
            ),
        ));
    }
    let (redacted, spans) = redact(trimmed);
    if redacted.trim().is_empty() {
        return Err((
            MemoryRememberFailure::RedactedToEmpty,
            "user memory text was empty after secret redaction".to_string(),
        ));
    }
    if redacted.len() > MAX_BYTES_PER_ENTRY {
        return Err((
            MemoryRememberFailure::TooLong,
            format!(
                "user memory text was {} bytes after redaction; max is {MAX_BYTES_PER_ENTRY}",
                redacted.len()
            ),
        ));
    }
    Ok((redacted, spans.len() as u32))
}

fn resolve_entries_path(user_memory_dir: &Path) -> Result<PathBuf, MemoryStoreError> {
    refuse_symlink(user_memory_dir, "user memory directory")?;
    let path = user_memory_dir.join(ENTRIES_FILE_NAME);
    refuse_symlink(&path, "user memory entries file")?;
    refuse_hardlink(&path, "user memory entries file")?;
    Ok(path)
}

fn ensured_entries_path(user_memory_dir: &Path) -> Result<PathBuf, MemoryStoreError> {
    refuse_symlink(user_memory_dir, "user memory directory")?;
    fs::create_dir_all(user_memory_dir).map_err(|error| {
        MemoryStoreError(format!(
            "create user memory directory {}: {error}",
            user_memory_dir.display()
        ))
    })?;
    resolve_entries_path(user_memory_dir)
}

fn refuse_symlink(path: &Path, label: &str) -> Result<(), MemoryStoreError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(MemoryStoreError(format!(
            "{label} at {} is a symlink; refusing to touch it",
            path.display()
        ))),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(MemoryStoreError(format!(
            "inspect {label} {}: {error}",
            path.display()
        ))),
    }
}

fn refuse_hardlink(path: &Path, label: &str) -> Result<(), MemoryStoreError> {
    match ensure_no_hardlink_alias(path) {
        Ok(()) => Ok(()),
        Err(PathError::Hardlink(path)) => Err(MemoryStoreError(format!(
            "{label} at {} has multiple hardlink aliases; refusing to touch it",
            path.display()
        ))),
        Err(PathError::Io { source, .. }) if source.kind() == std::io::ErrorKind::NotFound => {
            Ok(())
        }
        Err(error) => Err(MemoryStoreError(format!(
            "inspect {label} {}: {error}",
            path.display()
        ))),
    }
}

fn read_entries(path: &Path) -> Result<(Vec<UserMemoryEntry>, u64), MemoryStoreError> {
    refuse_symlink(path, "user memory entries file")?;
    refuse_hardlink(path, "user memory entries file")?;
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    }
    let file = match options.open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok((Vec::new(), 0));
        }
        Err(error) => {
            return Err(MemoryStoreError(format!(
                "read {}: {error}",
                path.display()
            )));
        }
    };
    let metadata = file
        .metadata()
        .map_err(|error| MemoryStoreError(format!("inspect {}: {error}", path.display())))?;
    if !metadata.is_file() {
        return Err(MemoryStoreError(format!(
            "user memory store {} is not a regular file",
            path.display()
        )));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.nlink() != 1 {
            return Err(MemoryStoreError(format!(
                "user memory store {} has multiple hardlink aliases",
                path.display()
            )));
        }
    }
    if metadata.len() > MAX_BYTES_TOTAL {
        return Err(MemoryStoreError(format!(
            "user memory store {} is {} bytes; max is {MAX_BYTES_TOTAL}",
            path.display(),
            metadata.len()
        )));
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take(MAX_BYTES_TOTAL + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| MemoryStoreError(format!("read {}: {error}", path.display())))?;
    if bytes.len() as u64 > MAX_BYTES_TOTAL {
        return Err(MemoryStoreError(format!(
            "user memory store {} grew beyond {MAX_BYTES_TOTAL} bytes while reading",
            path.display()
        )));
    }
    let raw = String::from_utf8(bytes).map_err(|_| {
        MemoryStoreError(format!(
            "user memory store {} is not valid UTF-8",
            path.display()
        ))
    })?;
    let mut entries = Vec::new();
    for (line_index, line) in raw.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let entry = serde_json::from_str::<UserMemoryEntry>(line).map_err(|_| {
            MemoryStoreError(format!(
                "user memory store {} has malformed JSON on line {}",
                path.display(),
                line_index + 1
            ))
        })?;
        entries.push(entry);
    }
    validate_persisted_entries(&entries)?;
    Ok((entries, raw.len() as u64))
}

fn validate_persisted_entries(entries: &[UserMemoryEntry]) -> Result<(), MemoryStoreError> {
    if entries.len() > MAX_ENTRIES {
        return Err(MemoryStoreError(format!(
            "user memory store has {} entries; max is {MAX_ENTRIES}",
            entries.len()
        )));
    }

    let mut ids = HashSet::with_capacity(entries.len());
    for (index, entry) in entries.iter().enumerate() {
        if !is_valid_entry_id(&entry.id) {
            return Err(invalid_persisted_entry(index, "invalid id"));
        }
        if !ids.insert(entry.id.as_str()) {
            return Err(invalid_persisted_entry(index, "duplicate id"));
        }
        if entry.text.trim().is_empty() {
            return Err(invalid_persisted_entry(index, "empty text"));
        }
        if entry.text.len() > MAX_BYTES_PER_ENTRY {
            return Err(invalid_persisted_entry(index, "text exceeds byte cap"));
        }
        let (_, raw_secret_spans) = redact(&entry.text);
        if !raw_secret_spans.is_empty() {
            return Err(invalid_persisted_entry(
                index,
                "contains unredacted secret-shaped text",
            ));
        }
        if entry.redaction_count as usize > recognized_redaction_markers(&entry.text) {
            return Err(invalid_persisted_entry(
                index,
                "redaction count has no matching markers",
            ));
        }
    }

    let canonical = serialize_entries(entries)?;
    if canonical.len() as u64 > MAX_BYTES_TOTAL {
        return Err(MemoryStoreError(format!(
            "serialized user memory store is {} bytes; max is {MAX_BYTES_TOTAL}",
            canonical.len()
        )));
    }
    Ok(())
}

fn invalid_persisted_entry(index: usize, reason: &str) -> MemoryStoreError {
    MemoryStoreError(format!(
        "user memory store entry {} is invalid: {reason}",
        index + 1
    ))
}

fn recognized_redaction_markers(text: &str) -> usize {
    [
        "[REDACTED:aws-key]",
        "[REDACTED:github-pat]",
        "[REDACTED:api-key]",
        "[REDACTED:jwt]",
        "[REDACTED:bearer]",
    ]
    .into_iter()
    .map(|marker| text.matches(marker).count())
    .sum()
}

fn serialize_entries(entries: &[UserMemoryEntry]) -> Result<String, MemoryStoreError> {
    let mut output = String::new();
    for entry in entries {
        let line = serde_json::to_string(entry)
            .map_err(|error| MemoryStoreError(format!("serialize user memory entry: {error}")))?;
        output.push_str(&line);
        output.push('\n');
    }
    Ok(output)
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), MemoryStoreError> {
    let parent = path.parent().ok_or_else(|| {
        MemoryStoreError(format!("user memory path {} has no parent", path.display()))
    })?;
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    let temporary = parent.join(format!(".entries.plume-user-memory-{nonce}.tmp"));
    let result = (|| {
        let mut file = fs::File::create(&temporary).map_err(|error| {
            MemoryStoreError(format!("create temp {}: {error}", temporary.display()))
        })?;
        file.write_all(bytes).map_err(|error| {
            MemoryStoreError(format!("write temp {}: {error}", temporary.display()))
        })?;
        file.sync_all().map_err(|error| {
            MemoryStoreError(format!("sync temp {}: {error}", temporary.display()))
        })?;
        fs::rename(&temporary, path)
            .map_err(|error| MemoryStoreError(format!("rename -> {}: {error}", path.display())))
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn err_remember(reason: MemoryRememberFailure, message: String) -> UserMemoryRememberResponse {
    UserMemoryRememberResponse::Err(UserMemoryRememberErr {
        ok: false,
        reason,
        message,
    })
}

fn err_update(reason: MemoryUpdateFailure, message: String) -> UserMemoryUpdateResponse {
    UserMemoryUpdateResponse::Err(UserMemoryUpdateErr {
        ok: false,
        reason,
        message,
    })
}

fn err_forget(reason: MemoryForgetFailure, message: String) -> UserMemoryForgetResponse {
    UserMemoryForgetResponse::Err(UserMemoryForgetErr {
        ok: false,
        reason,
        message,
    })
}

fn err_search(reason: MemorySearchFailure, message: String) -> UserMemorySearchResponse {
    UserMemorySearchResponse::Err(UserMemorySearchErr {
        ok: false,
        reason,
        message,
    })
}
