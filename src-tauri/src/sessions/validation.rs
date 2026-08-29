//! D63A: validation for session ids, titles, and transcript entries,
//! plus the pure entry ↔ database-row mappers.
//!
//! Everything here is pure (no SQLite, no filesystem) so the rules are
//! unit-testable without a database and reusable on both the save path
//! (wire → row) and the load path (row → wire). Load-side violations
//! are `Corrupt` — a persisted row we refuse to coerce — while wire-side
//! violations are `Invalid`.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use super::{
    EntryMessage, EntryRole, EntryStats, LineRange, SentMode, SessionStoreError,
    TranscriptArtifactOwner, TranscriptArtifactScope, TranscriptEntry,
};
use crate::prompts::{
    validate_context_manifest, validate_context_source_refs, ContextSourceManifestItem,
    ContextSourceRef,
};

/// Hard caps from the D63 design. Deliberately generous for real chats
/// and tight enough that a runaway caller cannot balloon the database.
pub(super) const MAX_SESSIONS: i64 = 200;

/// Bytes an entry writes into the `content` column.
///
/// Shared by the per-entry cap and the durable store cap so the two cannot
/// disagree about an entry's size. It must keep matching `row_from_entry`:
/// research entries put their payload in `artifact_json`, not `content`, which
/// is why they measure zero here.
pub(super) fn entry_content_len(entry: &TranscriptEntry) -> usize {
    match entry {
        TranscriptEntry::Message { message, .. } => message.content.len(),
        TranscriptEntry::Cancelled { partial, .. } => partial.len(),
        TranscriptEntry::Error { message } => message.len(),
        TranscriptEntry::ResearchArtifact { .. } | TranscriptEntry::ResearchExport { .. } => 0,
    }
}

/// Bytes an entry writes across every text column of its row.
///
/// The store cap has to weigh the whole row, not just `content`. Stats,
/// per-entry context manifests, and research payloads all live in their own
/// columns, so a save that keeps the same prose but carries heavier manifests
/// would otherwise measure as unchanged and grow a full store.
pub(super) fn entry_row_len(entry: &TranscriptEntry) -> usize {
    let row = match row_from_entry(entry) {
        Ok(row) => row,
        // Unrepresentable entries are rejected by validation before any write,
        // so measuring one as zero cannot admit it.
        Err(_) => return 0,
    };
    row.content.len()
        + row.stats_json.as_deref().map_or(0, str::len)
        + row.context_manifest_json.as_deref().map_or(0, str::len)
        + row.artifact_json.as_deref().map_or(0, str::len)
}
pub(super) const MAX_TRANSCRIPT_ENTRIES: usize = 500;
pub(super) const MAX_ENTRY_CONTENT_BYTES: usize = 256 * 1024;
pub(super) const MAX_TRANSCRIPT_BYTES: usize = 8 * 1024 * 1024;
pub(super) const MAX_TITLE_CHARS: usize = 120;
pub(super) const MAX_ID_LEN: usize = 64;
/// Mirrors `commands::chat::validate::MAX_ATTACHMENT_REL_PATH_LEN`; the
/// attachment shape persisted here is the one `chat.send` accepted.
pub(super) const MAX_ATTACHMENT_REL_PATH_LEN: usize = 1024;

pub(super) const DEFAULT_TITLE: &str = "New chat";

/// Mint an opaque session id: `s` + 64-bit nanos + pid + process-local
/// counter, all hex. Sortable by mint time, collision-free within a
/// process (counter) and across processes (pid), and always passes
/// [`validate_id`]. Ids are never paths.
pub(super) fn mint_session_id() -> String {
    mint_id('s')
}

/// Message ids share the session-id form with an `m` prefix. They are
/// re-minted on every transcript replacement; nothing references them
/// across saves.
pub(super) fn mint_message_id() -> String {
    mint_id('m')
}

fn mint_id(prefix: char) -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let pid = std::process::id();
    format!("{prefix}{nanos:016x}{pid:08x}{n:08x}")
}

/// Ids are validated before any lookup so a user-controlled string can
/// never smuggle path fragments or exotic content into SQL diagnostics.
/// (Values are always bound parameters regardless.)
pub(super) fn validate_id(id: &str) -> Result<(), SessionStoreError> {
    let len_ok = !id.is_empty() && id.len() <= MAX_ID_LEN;
    let chars_ok = id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
    if len_ok && chars_ok {
        Ok(())
    } else {
        Err(SessionStoreError::Invalid(
            "malformed session id".to_string(),
        ))
    }
}

/// D66: cap for a search query, in Unicode scalar values (measured
/// after trimming). Far beyond any real search; a bound so a runaway
/// caller cannot feed megabytes into the FTS tokenizer.
pub(super) const MAX_QUERY_CHARS: usize = 200;

/// D66: turn a user's search text into an FTS5 MATCH expression that
/// treats it as LITERAL terms — never as query syntax. Each
/// whitespace-separated term is double-quoted (embedded `"` doubled,
/// the FTS5 string escape) and suffixed `*` for prefix matching, so
/// incremental typing finds tokens as they are being written and
/// operators like `OR`, `NEAR(`, `-`, `*`, or an unbalanced quote are
/// searched for, not interpreted.
pub(super) fn build_fts_match(raw: &str) -> Result<String, SessionStoreError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(SessionStoreError::Invalid(
            "search query is empty after trimming".to_string(),
        ));
    }
    if trimmed.chars().count() > MAX_QUERY_CHARS {
        return Err(SessionStoreError::Invalid(format!(
            "search query exceeds {MAX_QUERY_CHARS} characters"
        )));
    }
    // Punctuation-only terms tokenize to nothing inside FTS5 (an empty
    // quoted phrase can even be a syntax error); keep only terms with
    // at least one alphanumeric scalar and reject a query with none —
    // a typed Invalid instead of an FTS5 mystery.
    let terms: Vec<String> = trimmed
        .split_whitespace()
        .filter(|term| term.chars().any(char::is_alphanumeric))
        .map(|term| format!("\"{}\"*", term.replace('"', "\"\"")))
        .collect();
    if terms.is_empty() {
        return Err(SessionStoreError::Invalid(
            "search query has no searchable characters".to_string(),
        ));
    }
    Ok(terms.join(" "))
}

/// Trim, then bound to 1–120 Unicode scalar values. Returns the trimmed
/// title — the trimmed form is what gets stored and echoed back.
pub(super) fn validate_title(raw: &str) -> Result<String, SessionStoreError> {
    let trimmed = raw.trim();
    let chars = trimmed.chars().count();
    if chars == 0 {
        return Err(SessionStoreError::Invalid(
            "session title is empty after trimming".to_string(),
        ));
    }
    if chars > MAX_TITLE_CHARS {
        return Err(SessionStoreError::Invalid(format!(
            "session title exceeds {MAX_TITLE_CHARS} characters"
        )));
    }
    Ok(trimmed.to_string())
}

/// Parse raw wire values into typed entries. The enum has no
/// `streaming` variant and no `system`/`tool` roles, so those arrive
/// here as serde unknown-variant errors and surface as a typed
/// `Invalid` naming the offending entry index — never as an opaque
/// deserialization failure at the Tauri boundary.
pub fn parse_entries(
    values: &[serde_json::Value],
) -> Result<Vec<TranscriptEntry>, SessionStoreError> {
    values
        .iter()
        .enumerate()
        .map(|(i, v)| {
            serde_json::from_value::<TranscriptEntry>(v.clone())
                .map_err(|e| SessionStoreError::Invalid(format!("entry {i}: {e}")))
        })
        .collect()
}

/// Semantic validation of an already-typed snapshot. Runs at the store
/// boundary (not only in the command layer) so the caps hold no matter
/// who calls the store.
pub(super) fn validate_entries(
    entries: &[TranscriptEntry],
    allow_attachments: bool,
) -> Result<(), SessionStoreError> {
    if entries.len() > MAX_TRANSCRIPT_ENTRIES {
        return Err(SessionStoreError::Invalid(format!(
            "transcript has {} entries; the cap is {MAX_TRANSCRIPT_ENTRIES}",
            entries.len()
        )));
    }
    for (i, entry) in entries.iter().enumerate() {
        let content_len = entry_content_len(entry);
        if content_len > MAX_ENTRY_CONTENT_BYTES {
            return Err(SessionStoreError::Invalid(format!(
                "entry {i}: content is {content_len} bytes; the per-entry cap is {MAX_ENTRY_CONTENT_BYTES}"
            )));
        }
        if let TranscriptEntry::Message {
            attachment_rel_path,
            attachment_line_range,
            duration_ms,
            context_sources,
            ..
        } = entry
        {
            validate_attachment(
                i,
                allow_attachments,
                attachment_rel_path.as_deref(),
                *attachment_line_range,
            )?;
            validate_duration(i, *duration_ms)?;
            if let Some(manifest) = context_sources {
                if !allow_attachments {
                    let local_browser_only = manifest.iter().all(|item| {
                        matches!(
                            item,
                            ContextSourceManifestItem::UserMemoryEntry { .. }
                                | ContextSourceManifestItem::BrowserTextEvidence { .. }
                                | ContextSourceManifestItem::BrowserScreenshotEvidence { .. }
                        )
                    });
                    if !local_browser_only {
                        return Err(SessionStoreError::Invalid(format!(
                            "entry {i}: local sessions may carry only user memory and local Browser evidence manifests"
                        )));
                    }
                }
                validate_context_manifest(manifest)
                    .map_err(|error| SessionStoreError::Invalid(format!("entry {i}: {error}")))?;
            }
        }
        if let TranscriptEntry::Cancelled { duration_ms, .. } = entry {
            validate_duration(i, *duration_ms)?;
        }
        match entry {
            TranscriptEntry::ResearchArtifact {
                owner,
                artifact_id,
                version,
            } => validate_artifact_ref(i, owner, artifact_id, *version, None, allow_attachments)?,
            TranscriptEntry::ResearchExport {
                owner,
                artifact_id,
                version,
                file_name,
            } => validate_artifact_ref(
                i,
                owner,
                artifact_id,
                *version,
                Some(file_name),
                allow_attachments,
            )?,
            _ => {}
        }
    }
    // Total-size cap on the same serialized form the wire carries.
    let total = serde_json::to_string(entries)
        .map_err(|e| SessionStoreError::Storage(format!("measure transcript size: {e}")))?
        .len();
    if total > MAX_TRANSCRIPT_BYTES {
        return Err(SessionStoreError::Invalid(format!(
            "transcript serializes to {total} bytes; the cap is {MAX_TRANSCRIPT_BYTES}"
        )));
    }
    Ok(())
}

fn validate_artifact_ref(
    index: usize,
    owner: &TranscriptArtifactOwner,
    artifact_id: &str,
    version: u32,
    file_name: Option<&String>,
    allow_project_scope: bool,
) -> Result<(), SessionStoreError> {
    validate_artifact_shape(owner, artifact_id, version, file_name)
        .map_err(|message| SessionStoreError::Invalid(format!("entry {index}: {message}")))?;
    let scope_matches =
        matches!(owner.scope, TranscriptArtifactScope::Project) == allow_project_scope;
    if !scope_matches {
        return Err(SessionStoreError::Invalid(format!(
            "entry {index}: artifact owner scope does not match this session store"
        )));
    }
    Ok(())
}

fn validate_artifact_shape(
    owner: &TranscriptArtifactOwner,
    artifact_id: &str,
    version: u32,
    file_name: Option<&String>,
) -> Result<(), &'static str> {
    validate_id(&owner.session_id).map_err(|_| "malformed artifact owner")?;
    let artifact_id_valid = !artifact_id.is_empty()
        && artifact_id.len() <= MAX_ID_LEN
        && artifact_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if !artifact_id_valid {
        return Err("malformed artifact id");
    }
    if version == 0 {
        return Err("artifact version must be positive");
    }
    if let Some(name) = file_name {
        let safe = !name.is_empty()
            && name.len() <= 255
            && name.ends_with(".md")
            && !name.contains('/')
            && !name.contains('\\')
            && name != "."
            && name != "..";
        if !safe {
            return Err("unsafe Markdown filename");
        }
    }
    Ok(())
}

fn validate_attachment(
    index: usize,
    allow_attachments: bool,
    rel_path: Option<&str>,
    range: Option<LineRange>,
) -> Result<(), SessionStoreError> {
    if !allow_attachments && (rel_path.is_some() || range.is_some()) {
        return Err(SessionStoreError::Invalid(format!(
            "entry {index}: local sessions cannot carry project-file attachments"
        )));
    }
    if range.is_some() && rel_path.is_none() {
        return Err(SessionStoreError::Invalid(format!(
            "entry {index}: attachment line range without an attachment path"
        )));
    }
    if let Some(rel) = rel_path {
        // Same shape rules `chat.send` enforced when the attachment was
        // originally accepted (commands::chat::validate): non-empty,
        // capped, relative, no `..` segments, no NUL. Shape-only — the
        // file is history metadata and is never re-read from disk here.
        if rel.trim().is_empty() {
            return Err(SessionStoreError::Invalid(format!(
                "entry {index}: attachment relPath is empty"
            )));
        }
        if rel.len() > MAX_ATTACHMENT_REL_PATH_LEN {
            return Err(SessionStoreError::Invalid(format!(
                "entry {index}: attachment relPath exceeds {MAX_ATTACHMENT_REL_PATH_LEN} chars"
            )));
        }
        if rel.starts_with('/') || rel.starts_with('\\') {
            return Err(SessionStoreError::Invalid(format!(
                "entry {index}: attachment relPath must be project-relative, not absolute"
            )));
        }
        if rel.split(['/', '\\']).any(|segment| segment == "..") {
            return Err(SessionStoreError::Invalid(format!(
                "entry {index}: attachment relPath must not contain '..' segments"
            )));
        }
        if rel.contains('\0') {
            return Err(SessionStoreError::Invalid(format!(
                "entry {index}: attachment relPath contains NUL byte"
            )));
        }
    }
    if let Some(r) = range {
        if r.start_line == 0 || r.start_line > r.end_line {
            return Err(SessionStoreError::Invalid(format!(
                "entry {index}: attachment line range must satisfy 1 <= startLine <= endLine"
            )));
        }
    }
    Ok(())
}

fn validate_duration(index: usize, duration_ms: Option<u64>) -> Result<(), SessionStoreError> {
    match duration_ms {
        Some(d) if i64::try_from(d).is_err() => Err(SessionStoreError::Invalid(format!(
            "entry {index}: durationMs out of range"
        ))),
        _ => Ok(()),
    }
}

/// One `chat_messages` row, database-typed. Used in both directions so
/// the save and load mappings cannot drift apart.
pub(super) struct RawMessageRow {
    pub kind: String,
    pub role: Option<String>,
    pub content: String,
    pub model_used: Option<String>,
    pub duration_ms: Option<i64>,
    pub attachment_rel_path: Option<String>,
    pub attachment_start_line: Option<i64>,
    pub attachment_end_line: Option<i64>,
    pub stats_json: Option<String>,
    pub sent_in_mode: Option<String>,
    pub context_manifest_json: Option<String>,
    pub artifact_json: Option<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ArtifactRow {
    owner: TranscriptArtifactOwner,
    artifact_id: String,
    version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    file_name: Option<String>,
}

pub(super) fn row_from_entry(entry: &TranscriptEntry) -> Result<RawMessageRow, SessionStoreError> {
    match entry {
        TranscriptEntry::Message {
            message,
            model_used,
            duration_ms,
            attachment_rel_path,
            attachment_line_range,
            stats,
            sent_in_mode,
            context_sources,
        } => {
            let stats_json = match stats {
                Some(s) => Some(serde_json::to_string(s).map_err(|e| {
                    SessionStoreError::Storage(format!("serialize entry stats: {e}"))
                })?),
                None => None,
            };
            let context_manifest_json = match context_sources {
                Some(items) if !items.is_empty() => {
                    Some(serde_json::to_string(items).map_err(|e| {
                        SessionStoreError::Storage(format!("serialize context manifest: {e}"))
                    })?)
                }
                _ => None,
            };
            Ok(RawMessageRow {
                kind: "message".to_string(),
                role: Some(role_as_str(message.role).to_string()),
                content: message.content.clone(),
                model_used: model_used.clone(),
                duration_ms: to_db_duration(*duration_ms)?,
                attachment_rel_path: attachment_rel_path.clone(),
                attachment_start_line: attachment_line_range.map(|r| i64::from(r.start_line)),
                attachment_end_line: attachment_line_range.map(|r| i64::from(r.end_line)),
                stats_json,
                sent_in_mode: sent_in_mode.map(|m| mode_as_str(m).to_string()),
                context_manifest_json,
                artifact_json: None,
            })
        }
        TranscriptEntry::Cancelled {
            partial,
            model_used,
            duration_ms,
        } => Ok(RawMessageRow {
            kind: "cancelled".to_string(),
            role: None,
            content: partial.clone(),
            model_used: model_used.clone(),
            duration_ms: to_db_duration(*duration_ms)?,
            attachment_rel_path: None,
            attachment_start_line: None,
            attachment_end_line: None,
            stats_json: None,
            sent_in_mode: None,
            context_manifest_json: None,
            artifact_json: None,
        }),
        TranscriptEntry::Error { message } => Ok(RawMessageRow {
            kind: "error".to_string(),
            role: None,
            content: message.clone(),
            model_used: None,
            duration_ms: None,
            attachment_rel_path: None,
            attachment_start_line: None,
            attachment_end_line: None,
            stats_json: None,
            sent_in_mode: None,
            context_manifest_json: None,
            artifact_json: None,
        }),
        TranscriptEntry::ResearchArtifact {
            owner,
            artifact_id,
            version,
        } => Ok(RawMessageRow {
            kind: "researchArtifact".to_string(),
            role: None,
            content: String::new(),
            model_used: None,
            duration_ms: None,
            attachment_rel_path: None,
            attachment_start_line: None,
            attachment_end_line: None,
            stats_json: None,
            sent_in_mode: None,
            context_manifest_json: None,
            artifact_json: Some(
                serde_json::to_string(&ArtifactRow {
                    owner: owner.clone(),
                    artifact_id: artifact_id.clone(),
                    version: *version,
                    file_name: None,
                })
                .map_err(|e| SessionStoreError::Storage(format!("serialize artifact ref: {e}")))?,
            ),
        }),
        TranscriptEntry::ResearchExport {
            owner,
            artifact_id,
            version,
            file_name,
        } => Ok(RawMessageRow {
            kind: "researchExport".to_string(),
            role: None,
            content: String::new(),
            model_used: None,
            duration_ms: None,
            attachment_rel_path: None,
            attachment_start_line: None,
            attachment_end_line: None,
            stats_json: None,
            sent_in_mode: None,
            context_manifest_json: None,
            artifact_json: Some(
                serde_json::to_string(&ArtifactRow {
                    owner: owner.clone(),
                    artifact_id: artifact_id.clone(),
                    version: *version,
                    file_name: Some(file_name.clone()),
                })
                .map_err(|e| SessionStoreError::Storage(format!("serialize export ref: {e}")))?,
            ),
        }),
    }
}

/// Load-side mapping. Any shape violation is `Corrupt` — the design
/// says malformed persisted rows are rejected, not silently coerced.
pub(super) fn entry_from_row(row: RawMessageRow) -> Result<TranscriptEntry, SessionStoreError> {
    match row.kind.as_str() {
        "message" => {
            let role = match row.role.as_deref() {
                Some("user") => EntryRole::User,
                Some("assistant") => EntryRole::Assistant,
                other => {
                    return Err(corrupt(format!(
                        "message row has role {other:?}; expected user or assistant"
                    )));
                }
            };
            let attachment_line_range = match (row.attachment_start_line, row.attachment_end_line) {
                (None, None) => None,
                (Some(start), Some(end)) => Some(LineRange {
                    start_line: line_from_db(start)?,
                    end_line: line_from_db(end)?,
                }),
                _ => {
                    return Err(corrupt(
                        "message row has half an attachment line range".to_string(),
                    ));
                }
            };
            if attachment_line_range.is_some() && row.attachment_rel_path.is_none() {
                return Err(corrupt(
                    "message row has a line range but no attachment path".to_string(),
                ));
            }
            let stats =
                match row.stats_json.as_deref() {
                    None => None,
                    Some(json) => Some(serde_json::from_str::<EntryStats>(json).map_err(|e| {
                        corrupt(format!("message row has malformed stats json: {e}"))
                    })?),
                };
            let sent_in_mode = match row.sent_in_mode.as_deref() {
                None => None,
                Some("chat") => Some(SentMode::Chat),
                Some("proposeDiff") => Some(SentMode::ProposeDiff),
                Some(other) => {
                    return Err(corrupt(format!("message row has unknown mode {other:?}")));
                }
            };
            let context_sources = match row.context_manifest_json.as_deref() {
                None => None,
                Some(json) => {
                    let items = serde_json::from_str::<Vec<ContextSourceManifestItem>>(json)
                        .map_err(|error| {
                            corrupt(format!(
                                "message row has malformed context manifest: {error}"
                            ))
                        })?;
                    validate_context_manifest(&items).map_err(|error| {
                        corrupt(format!("message row has invalid context manifest: {error}"))
                    })?;
                    (!items.is_empty()).then_some(items)
                }
            };
            if role != EntryRole::User && context_sources.is_some() {
                return Err(corrupt(
                    "assistant message row carries a context manifest".to_string(),
                ));
            }
            Ok(TranscriptEntry::Message {
                message: EntryMessage {
                    role,
                    content: row.content,
                },
                model_used: row.model_used,
                duration_ms: duration_from_db(row.duration_ms)?,
                attachment_rel_path: row.attachment_rel_path,
                attachment_line_range,
                stats,
                sent_in_mode,
                context_sources,
            })
        }
        "cancelled" => {
            if row.role.is_some() {
                return Err(corrupt("cancelled row carries a role".to_string()));
            }
            Ok(TranscriptEntry::Cancelled {
                partial: row.content,
                model_used: row.model_used,
                duration_ms: duration_from_db(row.duration_ms)?,
            })
        }
        "error" => {
            if row.role.is_some() {
                return Err(corrupt("error row carries a role".to_string()));
            }
            Ok(TranscriptEntry::Error {
                message: row.content,
            })
        }
        "researchArtifact" | "researchExport" => {
            if row.role.is_some() || !row.content.is_empty() {
                return Err(corrupt(
                    "artifact row carries message content or role".to_string(),
                ));
            }
            let json = row
                .artifact_json
                .as_deref()
                .ok_or_else(|| corrupt("artifact row is missing typed metadata".to_string()))?;
            let meta: ArtifactRow = serde_json::from_str(json)
                .map_err(|e| corrupt(format!("artifact row has malformed metadata: {e}")))?;
            validate_artifact_shape(
                &meta.owner,
                &meta.artifact_id,
                meta.version,
                meta.file_name.as_ref(),
            )
            .map_err(|message| corrupt(format!("artifact row has invalid metadata: {message}")))?;
            if row.kind == "researchArtifact" {
                if meta.file_name.is_some() {
                    return Err(corrupt(
                        "research artifact row carries a filename".to_string(),
                    ));
                }
                Ok(TranscriptEntry::ResearchArtifact {
                    owner: meta.owner,
                    artifact_id: meta.artifact_id,
                    version: meta.version,
                })
            } else {
                Ok(TranscriptEntry::ResearchExport {
                    owner: meta.owner,
                    artifact_id: meta.artifact_id,
                    version: meta.version,
                    file_name: meta.file_name.ok_or_else(|| {
                        corrupt("research export row has no filename".to_string())
                    })?,
                })
            }
        }
        other => Err(corrupt(format!("unknown persisted entry kind {other:?}"))),
    }
}

pub(super) fn validate_context_sources(
    sources: &[ContextSourceRef],
    allow_project_context: bool,
) -> Result<(), SessionStoreError> {
    if !allow_project_context
        && sources.iter().any(|source| {
            !matches!(
                source,
                ContextSourceRef::UserMemoryEntry { .. }
                    | ContextSourceRef::BrowserTextEvidence { .. }
                    | ContextSourceRef::BrowserScreenshotEvidence { .. }
            )
        })
    {
        return Err(SessionStoreError::Invalid(
            "local sessions may carry only user memory and local Browser evidence on the context shelf".into(),
        ));
    }
    let deduped = validate_context_source_refs(sources)
        .map_err(|error| SessionStoreError::Invalid(error.to_string()))?;
    if deduped.len() != sources.len() {
        return Err(SessionStoreError::Invalid(
            "context shelf contains duplicate source identities".into(),
        ));
    }
    Ok(())
}

pub(super) fn serialize_context_sources(
    sources: &[ContextSourceRef],
) -> Result<Option<String>, SessionStoreError> {
    if sources.is_empty() {
        return Ok(None);
    }
    serde_json::to_string(sources)
        .map(Some)
        .map_err(|error| SessionStoreError::Storage(format!("serialize context shelf: {error}")))
}

pub(super) fn parse_context_sources(
    json: Option<&str>,
) -> Result<Vec<ContextSourceRef>, SessionStoreError> {
    let Some(json) = json else {
        return Ok(Vec::new());
    };
    let sources = serde_json::from_str::<Vec<ContextSourceRef>>(json)
        .map_err(|error| corrupt(format!("malformed context shelf json: {error}")))?;
    validate_context_sources(&sources, true)
        .map_err(|error| corrupt(format!("invalid context shelf: {error}")))?;
    Ok(sources)
}

fn corrupt(detail: String) -> SessionStoreError {
    SessionStoreError::Corrupt(detail)
}

fn role_as_str(role: EntryRole) -> &'static str {
    match role {
        EntryRole::User => "user",
        EntryRole::Assistant => "assistant",
    }
}

fn mode_as_str(mode: SentMode) -> &'static str {
    match mode {
        SentMode::Chat => "chat",
        SentMode::ProposeDiff => "proposeDiff",
    }
}

fn to_db_duration(duration_ms: Option<u64>) -> Result<Option<i64>, SessionStoreError> {
    duration_ms
        .map(|d| {
            i64::try_from(d)
                .map_err(|_| SessionStoreError::Invalid("durationMs out of range".to_string()))
        })
        .transpose()
}

fn duration_from_db(raw: Option<i64>) -> Result<Option<u64>, SessionStoreError> {
    raw.map(|d| u64::try_from(d).map_err(|_| corrupt(format!("row has negative durationMs {d}"))))
        .transpose()
}

fn line_from_db(raw: i64) -> Result<u32, SessionStoreError> {
    u32::try_from(raw).map_err(|_| corrupt(format!("row has out-of-range line number {raw}")))
}
