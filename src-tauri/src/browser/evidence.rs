//! Immutable, project-scoped text snapshots captured from the sandbox Browser.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::prompts::redact::redact;

use super::policy::validate_browser_url;

pub const BROWSER_SELECTION_BYTE_CAP: usize = 16 * 1024;
pub const BROWSER_PAGE_BYTE_CAP: usize = 64 * 1024;
pub const BROWSER_TITLE_BYTE_CAP: usize = 512;
pub const BROWSER_EVIDENCE_MAX_RECORDS: usize = 100;
pub const BROWSER_EVIDENCE_TOTAL_BYTE_CAP: u64 = 4 * 1024 * 1024;
const BROWSER_EVIDENCE_RECORD_BYTE_CAP: u64 = 512 * 1024;
const BROWSER_EVIDENCE_VERSION: u32 = 1;
const EVIDENCE_DIR: &str = "browser-evidence";

static STORE_MUTEX: OnceLock<Mutex<()>> = OnceLock::new();
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum BrowserCaptureKind {
    Selection,
    Page,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CapturedBrowserText {
    pub capture_kind: BrowserCaptureKind,
    pub source_url: String,
    pub title: Option<String>,
    pub content: String,
    pub source_truncated: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BrowserEvidenceRecord {
    pub version: u32,
    pub id: String,
    pub capture_kind: BrowserCaptureKind,
    pub source_url: String,
    pub title: Option<String>,
    pub captured_at_ms: u64,
    pub content: String,
    pub bytes: u64,
    pub redaction_count: u64,
    pub truncated: bool,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BrowserEvidenceSummary {
    pub evidence_id: String,
    pub capture_kind: BrowserCaptureKind,
    pub source_url: String,
    pub title: Option<String>,
    pub captured_at_ms: u64,
    pub bytes: u64,
    pub redaction_count: u64,
    pub truncated: bool,
    pub preview: String,
}

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct BrowserEvidenceError(pub String);

impl BrowserEvidenceError {
    pub fn is_capacity(&self) -> bool {
        self.0 == "browser evidence store capacity reached"
    }
}

pub fn store_text_evidence(
    project_root: &Path,
    capture: CapturedBrowserText,
) -> Result<BrowserEvidenceSummary, BrowserEvidenceError> {
    let (source_url, source_url_redactions) = sanitize_source_url(&capture.source_url)?;
    let content_cap = match capture.capture_kind {
        BrowserCaptureKind::Selection => BROWSER_SELECTION_BYTE_CAP,
        BrowserCaptureKind::Page => BROWSER_PAGE_BYTE_CAP,
    };
    let (redacted_content, content_redactions) = redact(&capture.content);
    let (bounded_content, content_truncated) =
        truncate_redacted_utf8(&redacted_content, content_cap);
    let content = bounded_content.to_string();
    if content.trim().is_empty() {
        return Err(BrowserEvidenceError(
            "browser evidence content is empty".into(),
        ));
    }
    let mut title_redaction_count = 0_u64;
    let title = capture.title.and_then(|title| {
        let (redacted, spans) = redact(&title);
        title_redaction_count = spans.len() as u64;
        let (bounded, _) = truncate_redacted_utf8(&redacted, BROWSER_TITLE_BYTE_CAP);
        (!bounded.trim().is_empty()).then_some(bounded.to_string())
    });
    let record = BrowserEvidenceRecord {
        version: BROWSER_EVIDENCE_VERSION,
        id: mint_evidence_id(),
        capture_kind: capture.capture_kind,
        source_url,
        title,
        captured_at_ms: now_ms(),
        bytes: content.len() as u64,
        content,
        redaction_count: content_redactions.len() as u64
            + title_redaction_count
            + source_url_redactions,
        truncated: capture.source_truncated || content_truncated,
    };
    let serialized = serde_json::to_vec(&record)
        .map_err(|error| BrowserEvidenceError(format!("serialize browser evidence: {error}")))?;
    if serialized.len() as u64 > BROWSER_EVIDENCE_RECORD_BYTE_CAP {
        return Err(BrowserEvidenceError(
            "browser evidence record exceeded its storage cap".into(),
        ));
    }

    let mutex = STORE_MUTEX.get_or_init(|| Mutex::new(()));
    let _guard = mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let evidence_dir = ensure_evidence_dir(project_root)?;
    let (records, bytes) = store_usage(&evidence_dir)?;
    if records >= BROWSER_EVIDENCE_MAX_RECORDS
        || bytes.saturating_add(serialized.len() as u64) > BROWSER_EVIDENCE_TOTAL_BYTE_CAP
    {
        return Err(BrowserEvidenceError(
            "browser evidence store capacity reached".into(),
        ));
    }
    let path = evidence_path(&evidence_dir, &record.id)?;
    refuse_symlink(&path, "browser evidence record")?;
    if path.exists() {
        return Err(BrowserEvidenceError("browser evidence id collision".into()));
    }
    write_atomic(&path, &serialized)?;
    Ok(summary_of(&record))
}

pub fn read_text_evidence(
    project_root: &Path,
    evidence_id: &str,
) -> Result<Option<BrowserEvidenceRecord>, BrowserEvidenceError> {
    validate_evidence_id(evidence_id)?;
    let evidence_dir = resolve_evidence_dir(project_root)?;
    let path = evidence_path(&evidence_dir, evidence_id)?;
    refuse_symlink(&path, "browser evidence record")?;
    let metadata = match fs::metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(BrowserEvidenceError(format!(
                "stat browser evidence {}: {error}",
                path.display()
            )))
        }
    };
    if !metadata.is_file() {
        return Err(BrowserEvidenceError(
            "browser evidence record is not a regular file".into(),
        ));
    }
    ensure_single_link(&metadata)?;
    if metadata.len() > BROWSER_EVIDENCE_RECORD_BYTE_CAP {
        return Err(BrowserEvidenceError(
            "browser evidence record exceeded its storage cap".into(),
        ));
    }
    let raw = fs::read(&path).map_err(|error| {
        BrowserEvidenceError(format!("read browser evidence {}: {error}", path.display()))
    })?;
    let record: BrowserEvidenceRecord = serde_json::from_slice(&raw)
        .map_err(|error| BrowserEvidenceError(format!("parse browser evidence: {error}")))?;
    validate_record(&record, evidence_id)?;
    Ok(Some(record))
}

fn validate_record(
    record: &BrowserEvidenceRecord,
    expected_id: &str,
) -> Result<(), BrowserEvidenceError> {
    if record.version != BROWSER_EVIDENCE_VERSION || record.id != expected_id {
        return Err(BrowserEvidenceError(
            "browser evidence record version or identity mismatch".into(),
        ));
    }
    validate_evidence_id(&record.id)?;
    validate_browser_url(&record.source_url)
        .map_err(|_| BrowserEvidenceError("browser evidence has an invalid source URL".into()))?;
    let (sanitized_source_url, source_redactions) = sanitize_source_url(&record.source_url)?;
    if sanitized_source_url != record.source_url || source_redactions != 0 {
        return Err(BrowserEvidenceError(
            "browser evidence record contains unsafe source provenance".into(),
        ));
    }
    let content_cap = match record.capture_kind {
        BrowserCaptureKind::Selection => BROWSER_SELECTION_BYTE_CAP,
        BrowserCaptureKind::Page => BROWSER_PAGE_BYTE_CAP,
    };
    if record.content.is_empty()
        || record.content.len() > content_cap
        || record.bytes != record.content.len() as u64
        || record
            .title
            .as_ref()
            .is_some_and(|title| title.len() > BROWSER_TITLE_BYTE_CAP)
    {
        return Err(BrowserEvidenceError(
            "browser evidence record failed its bounded field checks".into(),
        ));
    }
    let (redacted_content, spans) = redact(&record.content);
    if redacted_content != record.content || !spans.is_empty() {
        return Err(BrowserEvidenceError(
            "browser evidence record contains unredacted content".into(),
        ));
    }
    if let Some(title) = &record.title {
        let (redacted_title, spans) = redact(title);
        if redacted_title != *title || !spans.is_empty() {
            return Err(BrowserEvidenceError(
                "browser evidence record contains an unredacted title".into(),
            ));
        }
    }
    Ok(())
}

fn summary_of(record: &BrowserEvidenceRecord) -> BrowserEvidenceSummary {
    BrowserEvidenceSummary {
        evidence_id: record.id.clone(),
        capture_kind: record.capture_kind,
        source_url: record.source_url.clone(),
        title: record.title.clone(),
        captured_at_ms: record.captured_at_ms,
        bytes: record.bytes,
        redaction_count: record.redaction_count,
        truncated: record.truncated,
        preview: preview_text(&record.content),
    }
}

fn sanitize_source_url(raw: &str) -> Result<(String, u64), BrowserEvidenceError> {
    let validated = validate_browser_url(raw)
        .map_err(|_| BrowserEvidenceError("browser evidence has an invalid source URL".into()))?;
    let mut provenance = validated.url;
    provenance.set_query(None);
    provenance.set_fragment(None);
    let host = provenance.host_str().unwrap_or_default();
    let (_, host_spans) = redact(host);
    let safe_origin = if host_spans.is_empty() {
        format!("{}/", provenance.origin().ascii_serialization())
    } else {
        format!("{}://redacted.invalid/", provenance.scheme())
    };
    let (_, raw_spans) = redact(provenance.as_str());
    let decoded_spans = encoded_path_redactions(provenance.path());
    let redaction_count = raw_spans.len() as u64 + decoded_spans;
    if redaction_count > 0 {
        return Ok((safe_origin, redaction_count));
    }
    Ok((provenance.as_str().to_string(), 0))
}

fn encoded_path_redactions(path: &str) -> u64 {
    let mut current = path.to_string();
    loop {
        let decoded = percent_decode_lossy(&current);
        if decoded == current {
            return 0;
        }
        let spans = redact(&decoded).1;
        if !spans.is_empty() {
            return spans.len() as u64;
        }
        current = decoded;
    }
}

fn percent_decode_lossy(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            if let (Some(high), Some(low)) =
                (hex_value(bytes[index + 1]), hex_value(bytes[index + 2]))
            {
                decoded.push((high << 4) | low);
                index += 3;
                continue;
            }
        }
        decoded.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&decoded).into_owned()
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn resolve_evidence_dir(project_root: &Path) -> Result<PathBuf, BrowserEvidenceError> {
    let plume = project_root.join(".plume");
    refuse_symlink(&plume, ".plume")?;
    let evidence = plume.join(EVIDENCE_DIR);
    refuse_symlink(&evidence, ".plume/browser-evidence")?;
    Ok(evidence)
}

fn ensure_evidence_dir(project_root: &Path) -> Result<PathBuf, BrowserEvidenceError> {
    let plume = project_root.join(".plume");
    refuse_symlink(&plume, ".plume")?;
    fs::create_dir_all(&plume)
        .map_err(|error| BrowserEvidenceError(format!("create .plume: {error}")))?;
    let evidence = plume.join(EVIDENCE_DIR);
    refuse_symlink(&evidence, ".plume/browser-evidence")?;
    fs::create_dir_all(&evidence).map_err(|error| {
        BrowserEvidenceError(format!("create .plume/browser-evidence: {error}"))
    })?;
    Ok(evidence)
}

fn store_usage(evidence_dir: &Path) -> Result<(usize, u64), BrowserEvidenceError> {
    let mut records = 0usize;
    let mut bytes = 0u64;
    for entry in fs::read_dir(evidence_dir)
        .map_err(|error| BrowserEvidenceError(format!("scan browser evidence: {error}")))?
    {
        let entry = entry
            .map_err(|error| BrowserEvidenceError(format!("scan browser evidence: {error}")))?;
        let file_type = entry
            .file_type()
            .map_err(|error| BrowserEvidenceError(format!("stat browser evidence: {error}")))?;
        if file_type.is_symlink() {
            return Err(BrowserEvidenceError(
                "browser evidence store contains a symlink".into(),
            ));
        }
        if !file_type.is_file()
            || entry.path().extension().and_then(|value| value.to_str()) != Some("json")
        {
            continue;
        }
        records = records.saturating_add(1);
        bytes = bytes.saturating_add(
            entry
                .metadata()
                .map_err(|error| BrowserEvidenceError(format!("stat browser evidence: {error}")))?
                .len(),
        );
    }
    Ok((records, bytes))
}

fn evidence_path(dir: &Path, evidence_id: &str) -> Result<PathBuf, BrowserEvidenceError> {
    validate_evidence_id(evidence_id)?;
    Ok(dir.join(format!("{evidence_id}.json")))
}

fn validate_evidence_id(id: &str) -> Result<(), BrowserEvidenceError> {
    if id.len() == 35
        && id.starts_with("be_")
        && id[3..].bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        Ok(())
    } else {
        Err(BrowserEvidenceError("invalid browser evidence id".into()))
    }
}

fn mint_evidence_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    let counter = NEXT_ID.fetch_add(1, Ordering::Relaxed) as u128;
    let pid = std::process::id() as u128;
    let value = nanos ^ (pid << 64) ^ counter;
    format!("be_{value:032x}")
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

fn truncate_utf8(value: &str, cap: usize) -> (&str, bool) {
    if value.len() <= cap {
        return (value, false);
    }
    let mut end = cap;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    (&value[..end], true)
}

fn truncate_redacted_utf8(value: &str, cap: usize) -> (&str, bool) {
    let (bounded, truncated) = truncate_utf8(value, cap);
    if !truncated {
        return (bounded, false);
    }
    let Some(marker_start) = bounded.rfind("[REDACTED:") else {
        const MARKER_PREFIX: &str = "[REDACTED:";
        for length in (1..MARKER_PREFIX.len()).rev() {
            if bounded.ends_with(&MARKER_PREFIX[..length]) {
                return (&bounded[..bounded.len() - length], true);
            }
        }
        return (bounded, true);
    };
    if bounded[marker_start..].contains(']') {
        (bounded, true)
    } else {
        (&bounded[..marker_start], true)
    }
}

pub(crate) fn preview_text(value: &str) -> String {
    let flat = value.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut chars = flat.chars();
    let preview: String = chars.by_ref().take(160).collect();
    if chars.next().is_some() {
        format!("{preview}…")
    } else {
        preview
    }
}

fn refuse_symlink(path: &Path, label: &str) -> Result<(), BrowserEvidenceError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(BrowserEvidenceError(format!(
            "{label} is a symlink; refusing browser evidence access"
        ))),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(BrowserEvidenceError(format!("stat {label}: {error}"))),
    }
}

#[cfg(unix)]
fn ensure_single_link(metadata: &fs::Metadata) -> Result<(), BrowserEvidenceError> {
    use std::os::unix::fs::MetadataExt;
    if metadata.nlink() == 1 {
        Ok(())
    } else {
        Err(BrowserEvidenceError(
            "browser evidence record is a hardlink alias".into(),
        ))
    }
}

#[cfg(not(unix))]
fn ensure_single_link(_: &fs::Metadata) -> Result<(), BrowserEvidenceError> {
    Ok(())
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), BrowserEvidenceError> {
    let parent = path
        .parent()
        .ok_or_else(|| BrowserEvidenceError("browser evidence path has no parent".into()))?;
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| BrowserEvidenceError("browser evidence path has no filename".into()))?;
    let nonce = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let temporary = parent.join(format!(".{file_name}.{nonce}.tmp"));
    let result = (|| {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|error| BrowserEvidenceError(format!("create evidence temp: {error}")))?;
        file.write_all(bytes)
            .map_err(|error| BrowserEvidenceError(format!("write evidence temp: {error}")))?;
        file.sync_all()
            .map_err(|error| BrowserEvidenceError(format!("sync evidence temp: {error}")))?;
        fs::rename(&temporary, path)
            .map_err(|error| BrowserEvidenceError(format!("commit browser evidence: {error}")))
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}
