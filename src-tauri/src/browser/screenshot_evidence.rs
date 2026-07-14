//! Immutable, project-scoped PNG snapshots captured from the sandbox Browser.

use std::fs;
use std::io::Cursor;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::prompts::redact::redact;

use super::evidence::sanitize_source_url;

pub const BROWSER_SCREENSHOT_BYTE_CAP: usize = 4 * 1024 * 1024;
pub const BROWSER_SCREENSHOT_DIMENSION_CAP: u32 = 4096;
pub const BROWSER_SCREENSHOT_MAX_RECORDS: usize = 25;
pub const BROWSER_SCREENSHOT_TOTAL_BYTE_CAP: u64 = 32 * 1024 * 1024;
const BROWSER_SCREENSHOT_METADATA_BYTE_CAP: u64 = 64 * 1024;
const BROWSER_SCREENSHOT_TITLE_BYTE_CAP: usize = 512;
const BROWSER_SCREENSHOT_VERSION: u32 = 1;
const SCREENSHOT_DIR: &str = "screenshots";
const PNG_SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";

static STORE_MUTEX: OnceLock<Mutex<()>> = OnceLock::new();
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CapturedBrowserScreenshot {
    pub source_url: String,
    pub title: Option<String>,
    pub png_bytes: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BrowserScreenshotMetadata {
    pub version: u32,
    pub id: String,
    pub source_url: String,
    pub title: Option<String>,
    pub captured_at_ms: u64,
    pub width: u32,
    pub height: u32,
    pub bytes: u64,
    pub sha256: String,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BrowserScreenshotSummary {
    pub evidence_id: String,
    pub source_url: String,
    pub title: Option<String>,
    pub captured_at_ms: u64,
    pub width: u32,
    pub height: u32,
    pub bytes: u64,
    pub sha256: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoredBrowserScreenshot {
    pub metadata: BrowserScreenshotMetadata,
    pub png_bytes: Vec<u8>,
}

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct BrowserScreenshotError(pub String);

impl BrowserScreenshotError {
    pub fn is_capacity(&self) -> bool {
        self.0 == "browser screenshot store capacity reached"
    }
}

pub fn store_screenshot_evidence(
    project_root: &Path,
    capture: CapturedBrowserScreenshot,
) -> Result<BrowserScreenshotSummary, BrowserScreenshotError> {
    validate_png(&capture.png_bytes, capture.width, capture.height)?;
    let (source_url, _) = sanitize_source_url(&capture.source_url)
        .map_err(|error| BrowserScreenshotError(error.0))?;
    let title = sanitize_title(capture.title);
    let metadata = BrowserScreenshotMetadata {
        version: BROWSER_SCREENSHOT_VERSION,
        id: mint_id(),
        source_url,
        title,
        captured_at_ms: now_ms(),
        width: capture.width,
        height: capture.height,
        bytes: capture.png_bytes.len() as u64,
        sha256: sha256_hex(&capture.png_bytes),
    };
    let metadata_bytes = serde_json::to_vec(&metadata).map_err(|error| {
        BrowserScreenshotError(format!("serialize screenshot metadata: {error}"))
    })?;
    if metadata_bytes.len() as u64 > BROWSER_SCREENSHOT_METADATA_BYTE_CAP {
        return Err(BrowserScreenshotError(
            "browser screenshot metadata exceeded its storage cap".into(),
        ));
    }

    let mutex = STORE_MUTEX.get_or_init(|| Mutex::new(()));
    let _guard = mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let dir = ensure_screenshot_dir(project_root)?;
    let (count, bytes) = store_usage(&dir)?;
    if count >= BROWSER_SCREENSHOT_MAX_RECORDS
        || bytes.saturating_add(capture.png_bytes.len() as u64) > BROWSER_SCREENSHOT_TOTAL_BYTE_CAP
    {
        return Err(BrowserScreenshotError(
            "browser screenshot store capacity reached".into(),
        ));
    }
    let metadata_path = record_path(&dir, &metadata.id, "json")?;
    let png_path = record_path(&dir, &metadata.id, "png")?;
    refuse_link_or_existing(&metadata_path)?;
    refuse_link_or_existing(&png_path)?;
    write_atomic(&png_path, &capture.png_bytes)?;
    if let Err(error) = write_atomic(&metadata_path, &metadata_bytes) {
        let _ = fs::remove_file(&png_path);
        return Err(error);
    }
    Ok(summary_of(&metadata))
}

pub fn read_screenshot_evidence(
    project_root: &Path,
    evidence_id: &str,
) -> Result<Option<StoredBrowserScreenshot>, BrowserScreenshotError> {
    validate_id(evidence_id)?;
    let dir = resolve_screenshot_dir(project_root)?;
    let metadata_path = record_path(&dir, evidence_id, "json")?;
    let png_path = record_path(&dir, evidence_id, "png")?;
    let metadata_meta = match safe_file_metadata(&metadata_path)? {
        Some(metadata) => metadata,
        None => return Ok(None),
    };
    if metadata_meta.len() > BROWSER_SCREENSHOT_METADATA_BYTE_CAP {
        return Err(BrowserScreenshotError(
            "screenshot metadata is oversized".into(),
        ));
    }
    let raw = fs::read(&metadata_path).map_err(io_error("read screenshot metadata"))?;
    let metadata: BrowserScreenshotMetadata = serde_json::from_slice(&raw)
        .map_err(|error| BrowserScreenshotError(format!("parse screenshot metadata: {error}")))?;
    validate_metadata(&metadata, evidence_id)?;
    let png_meta = safe_file_metadata(&png_path)?
        .ok_or_else(|| BrowserScreenshotError("screenshot PNG is missing".into()))?;
    if png_meta.len() != metadata.bytes || png_meta.len() > BROWSER_SCREENSHOT_BYTE_CAP as u64 {
        return Err(BrowserScreenshotError(
            "screenshot PNG size mismatch".into(),
        ));
    }
    let png_bytes = fs::read(&png_path).map_err(io_error("read screenshot PNG"))?;
    validate_png(&png_bytes, metadata.width, metadata.height)?;
    if sha256_hex(&png_bytes) != metadata.sha256 {
        return Err(BrowserScreenshotError(
            "screenshot PNG digest mismatch".into(),
        ));
    }
    Ok(Some(StoredBrowserScreenshot {
        metadata,
        png_bytes,
    }))
}

fn validate_metadata(
    metadata: &BrowserScreenshotMetadata,
    expected_id: &str,
) -> Result<(), BrowserScreenshotError> {
    if metadata.version != BROWSER_SCREENSHOT_VERSION || metadata.id != expected_id {
        return Err(BrowserScreenshotError(
            "screenshot metadata version or identity mismatch".into(),
        ));
    }
    validate_id(&metadata.id)?;
    if metadata.sha256.len() != 64
        || !metadata
            .sha256
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(BrowserScreenshotError(
            "screenshot digest is malformed".into(),
        ));
    }
    let (safe_url, redactions) = sanitize_source_url(&metadata.source_url)
        .map_err(|error| BrowserScreenshotError(error.0))?;
    if safe_url != metadata.source_url || redactions != 0 {
        return Err(BrowserScreenshotError(
            "screenshot URL provenance is unsafe".into(),
        ));
    }
    if let Some(title) = &metadata.title {
        let (safe, spans) = redact(title);
        if safe != *title || !spans.is_empty() || title.len() > BROWSER_SCREENSHOT_TITLE_BYTE_CAP {
            return Err(BrowserScreenshotError("screenshot title is unsafe".into()));
        }
    }
    validate_dimensions(metadata.width, metadata.height)
}

fn validate_png(bytes: &[u8], width: u32, height: u32) -> Result<(), BrowserScreenshotError> {
    if bytes.len() < 24
        || bytes.len() > BROWSER_SCREENSHOT_BYTE_CAP
        || bytes.get(..8) != Some(PNG_SIGNATURE)
        || bytes.get(12..16) != Some(b"IHDR")
    {
        return Err(BrowserScreenshotError(
            "invalid or oversized screenshot PNG".into(),
        ));
    }
    validate_dimensions(width, height)?;
    let decoder = png::Decoder::new(Cursor::new(bytes));
    let mut reader = decoder
        .read_info()
        .map_err(|_| BrowserScreenshotError("screenshot PNG could not be decoded".into()))?;
    let mut decoded = vec![0; reader.output_buffer_size()];
    let info = reader
        .next_frame(&mut decoded)
        .map_err(|_| BrowserScreenshotError("screenshot PNG could not be decoded".into()))?;
    if (info.width, info.height) != (width, height) {
        return Err(BrowserScreenshotError(
            "screenshot dimensions do not match PNG".into(),
        ));
    }
    Ok(())
}

fn validate_dimensions(width: u32, height: u32) -> Result<(), BrowserScreenshotError> {
    if width == 0
        || height == 0
        || width > BROWSER_SCREENSHOT_DIMENSION_CAP
        || height > BROWSER_SCREENSHOT_DIMENSION_CAP
    {
        return Err(BrowserScreenshotError(
            "screenshot dimensions are out of bounds".into(),
        ));
    }
    Ok(())
}

fn sanitize_title(title: Option<String>) -> Option<String> {
    title.and_then(|title| {
        let (redacted, _) = redact(&title);
        let bounded = truncate_utf8(&redacted, BROWSER_SCREENSHOT_TITLE_BYTE_CAP);
        (!bounded.trim().is_empty()).then_some(bounded.to_string())
    })
}

fn truncate_utf8(value: &str, cap: usize) -> &str {
    if value.len() <= cap {
        return value;
    }
    let mut end = cap;
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    &value[..end]
}

fn summary_of(metadata: &BrowserScreenshotMetadata) -> BrowserScreenshotSummary {
    BrowserScreenshotSummary {
        evidence_id: metadata.id.clone(),
        source_url: metadata.source_url.clone(),
        title: metadata.title.clone(),
        captured_at_ms: metadata.captured_at_ms,
        width: metadata.width,
        height: metadata.height,
        bytes: metadata.bytes,
        sha256: metadata.sha256.clone(),
    }
}

fn ensure_screenshot_dir(project_root: &Path) -> Result<PathBuf, BrowserScreenshotError> {
    let plume = ensure_plain_dir(&project_root.join(".plume"), "project metadata")?;
    let evidence = ensure_plain_dir(&plume.join("browser-evidence"), "browser evidence")?;
    ensure_plain_dir(&evidence.join(SCREENSHOT_DIR), "browser screenshots")
}

fn resolve_screenshot_dir(project_root: &Path) -> Result<PathBuf, BrowserScreenshotError> {
    let mut current = project_root.to_path_buf();
    for (part, label) in [
        (".plume", "project metadata"),
        ("browser-evidence", "browser evidence"),
        (SCREENSHOT_DIR, "browser screenshots"),
    ] {
        current.push(part);
        let metadata = fs::symlink_metadata(&current)
            .map_err(|error| BrowserScreenshotError(format!("stat {label}: {error}")))?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(BrowserScreenshotError(format!(
                "{label} is not a plain directory"
            )));
        }
    }
    Ok(current)
}

fn ensure_plain_dir(path: &Path, label: &str) -> Result<PathBuf, BrowserScreenshotError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => Err(
            BrowserScreenshotError(format!("{label} is not a plain directory")),
        ),
        Ok(_) => Ok(path.to_path_buf()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir(path).map_err(|error| {
                BrowserScreenshotError(format!("create {label} {}: {error}", path.display()))
            })?;
            Ok(path.to_path_buf())
        }
        Err(error) => Err(BrowserScreenshotError(format!("stat {label}: {error}"))),
    }
}

fn safe_file_metadata(path: &Path) -> Result<Option<fs::Metadata>, BrowserScreenshotError> {
    let link = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(BrowserScreenshotError(format!(
                "stat screenshot file: {error}"
            )))
        }
    };
    if link.file_type().is_symlink() || !link.is_file() {
        return Err(BrowserScreenshotError(
            "screenshot file is not a regular file".into(),
        ));
    }
    ensure_single_link(&link)?;
    Ok(Some(link))
}

#[cfg(unix)]
fn ensure_single_link(metadata: &fs::Metadata) -> Result<(), BrowserScreenshotError> {
    use std::os::unix::fs::MetadataExt;
    if metadata.nlink() != 1 {
        return Err(BrowserScreenshotError(
            "screenshot file has multiple hard links".into(),
        ));
    }
    Ok(())
}

#[cfg(not(unix))]
fn ensure_single_link(_: &fs::Metadata) -> Result<(), BrowserScreenshotError> {
    Ok(())
}

fn refuse_link_or_existing(path: &Path) -> Result<(), BrowserScreenshotError> {
    match fs::symlink_metadata(path) {
        Ok(_) => Err(BrowserScreenshotError("screenshot id collision".into())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(BrowserScreenshotError(format!(
            "stat screenshot path: {error}"
        ))),
    }
}

fn store_usage(dir: &Path) -> Result<(usize, u64), BrowserScreenshotError> {
    let mut count = 0_usize;
    let mut bytes = 0_u64;
    for entry in fs::read_dir(dir).map_err(io_error("read screenshot directory"))? {
        let entry = entry.map_err(io_error("read screenshot entry"))?;
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some("png") {
            continue;
        }
        let metadata = safe_file_metadata(&path)?
            .ok_or_else(|| BrowserScreenshotError("screenshot disappeared during scan".into()))?;
        count += 1;
        bytes = bytes.saturating_add(metadata.len());
    }
    Ok((count, bytes))
}

fn record_path(dir: &Path, id: &str, extension: &str) -> Result<PathBuf, BrowserScreenshotError> {
    validate_id(id)?;
    Ok(dir.join(format!("{id}.{extension}")))
}

fn validate_id(id: &str) -> Result<(), BrowserScreenshotError> {
    if id.len() == 35
        && id.starts_with("bs_")
        && id[3..].bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        Ok(())
    } else {
        Err(BrowserScreenshotError(
            "invalid screenshot evidence id".into(),
        ))
    }
}

fn mint_id() -> String {
    let counter = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let mut hasher = Sha256::new();
    hasher.update(now_ms().to_le_bytes());
    hasher.update(std::process::id().to_le_bytes());
    hasher.update(counter.to_le_bytes());
    let digest = hasher.finalize();
    format!("bs_{}", hex_prefix(&digest, 16))
}

fn hex_prefix(bytes: &[u8], count: usize) -> String {
    let mut out = String::with_capacity(count * 2);
    for byte in bytes.iter().take(count) {
        use std::fmt::Write as _;
        let _ = write!(out, "{byte:02x}");
    }
    out
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    hex_prefix(&digest, digest.len())
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), BrowserScreenshotError> {
    let temp = path.with_extension(format!("{}.tmp", NEXT_ID.fetch_add(1, Ordering::Relaxed)));
    let result = (|| {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)
            .map_err(io_error("create screenshot temp file"))?;
        file.write_all(bytes)
            .map_err(io_error("write screenshot temp file"))?;
        file.sync_all()
            .map_err(io_error("sync screenshot temp file"))?;
        fs::rename(&temp, path).map_err(io_error("commit screenshot file"))
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

fn io_error(context: &'static str) -> impl FnOnce(std::io::Error) -> BrowserScreenshotError {
    move |error| BrowserScreenshotError(format!("{context}: {error}"))
}
