//! D71: curated memory topic files.
//!
//! Beyond the flat `entries.jsonl` store (D37), the North Star
//! (`docs/LOCAL_AGENT_NORTH_STAR.md § Plume Memory Design Direction`)
//! describes a richer, human-authored layer of Markdown under
//! `.plume/memory/`:
//!
//! ```text
//! .plume/memory/
//!   INDEX.md   — pointers to durable project facts and topic files
//!   USER.md    — user preferences relevant to Plume
//!   SOUL.md    — the agent's durable voice/personality baseline
//!   topics/
//!     architecture.md, commands.md, testing.md, ...
//! ```
//!
//! These files are authored by the user in their own editor (Plume
//! does not write them in D71); this module reads and surfaces them,
//! capped and symlink-safe, behind the same trust gate as the rest of
//! the memory verbs. The three core files are "always-loaded" prompt
//! fuel — kept to a tight per-file cap — while `topics/*.md` are
//! larger reference docs read on demand.
//!
//! Wiring the always-loaded trio into the chat prompt context (like
//! `read_for_prompt` does for entries) is the D72 follow-up; D71 ships
//! the read + UI floor only.

use std::io::Read;
use std::path::Path;

use serde::Serialize;

use super::{memory_mutex, refuse_symlink, resolve_memory_file, MemoryStoreError};

/// Per-file cap for the always-loaded core files (INDEX/USER/SOUL).
/// They are prompt fuel, so every byte costs context tokens — keep
/// them small.
pub(crate) const MAX_CORE_FILE_BYTES: usize = 2 * 1024;

/// Per-file cap for `topics/*.md`. Topics are reference docs loaded on
/// demand, so they can be larger than the always-loaded trio.
pub(crate) const MAX_TOPIC_FILE_BYTES: usize = 8 * 1024;

/// How many `topics/*.md` files we surface. A project with more than
/// this many topic docs is past what the panel should list inline.
pub(crate) const MAX_TOPIC_FILES: usize = 32;

/// Which curated file a `TopicFile` is. Drives the panel's ordering
/// and labels; the core trio always appears (even when missing) so the
/// user sees the convention.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum TopicKind {
    Index,
    User,
    Soul,
    Topic,
}

/// One curated memory file. `exists: false` carries empty `content`
/// and `bytes: 0` — the panel renders the core trio as "not created
/// yet" rather than hiding them.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TopicFile {
    /// Path relative to `.plume/memory/`, e.g. `"INDEX.md"` or
    /// `"topics/architecture.md"`.
    pub name: String,
    pub kind: TopicKind,
    pub exists: bool,
    /// On-disk byte size (full file, before capping). `0` if missing.
    pub bytes: u64,
    /// The content was longer than its cap and was trimmed.
    pub truncated: bool,
    /// Capped, UTF-8-safe content. Empty when missing.
    pub content: String,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TopicLimits {
    pub max_core_bytes: u32,
    pub max_topic_bytes: u32,
    pub max_topics: u32,
}

impl Default for TopicLimits {
    fn default() -> Self {
        Self {
            max_core_bytes: MAX_CORE_FILE_BYTES as u32,
            max_topic_bytes: MAX_TOPIC_FILE_BYTES as u32,
            max_topics: MAX_TOPIC_FILES as u32,
        }
    }
}

/// Read result for `memory.topics`. `core` is always the three curated
/// files in fixed order (INDEX, USER, SOUL); `topics` is the
/// `topics/*.md` set sorted by name and capped to `MAX_TOPIC_FILES`.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MemoryTopics {
    pub core: Vec<TopicFile>,
    pub topics: Vec<TopicFile>,
    /// `true` when more than `MAX_TOPIC_FILES` `*.md` files were found
    /// and the surplus was dropped.
    pub topics_truncated: bool,
    pub limits: TopicLimits,
}

/// D71: read the curated memory topic files. Trust is enforced at the
/// IPC layer; this function holds the process-wide memory mutex (so a
/// concurrent write to the entries store can't interleave) and refuses
/// planted symlinks the same way the entries store does.
pub fn read_topics(project_root: &Path) -> Result<MemoryTopics, MemoryStoreError> {
    let _guard = memory_mutex().lock().unwrap_or_else(|e| e.into_inner());

    let core = [
        (TopicKind::Index, "INDEX.md"),
        (TopicKind::User, "USER.md"),
        (TopicKind::Soul, "SOUL.md"),
    ]
    .into_iter()
    .map(|(kind, name)| {
        let path = resolve_memory_file(project_root, name)?;
        build_topic_file(name, kind, &path, MAX_CORE_FILE_BYTES)
    })
    .collect::<Result<Vec<_>, MemoryStoreError>>()?;

    let (topics, topics_truncated) = read_topic_dir(project_root)?;

    Ok(MemoryTopics {
        core,
        topics,
        topics_truncated,
        limits: TopicLimits::default(),
    })
}

/// D72: the always-loaded core files projected for the chat prompt.
/// Carries only the existing, non-empty core files that fit the byte
/// budget, in fixed order (INDEX, USER, SOUL). `topics/*.md` are NOT
/// included — they are reference docs read on demand, not always-loaded
/// prompt fuel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TopicsPromptRead {
    pub files: Vec<TopicPromptFile>,
    pub used_bytes: usize,
    pub byte_cap: usize,
    /// A core file was skipped or trimmed to stay within the budget /
    /// per-file cap.
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TopicPromptFile {
    pub name: String,
    pub content: String,
}

/// D72: read the always-loaded core topic files (INDEX/USER/SOUL) for
/// folding into the chat prompt, within `byte_cap`. Missing,
/// whitespace-only, or symlinked-refused files are skipped; a file that
/// would overflow the budget is skipped (and `truncated` set) while
/// smaller later files are still considered. Same symlink-safe resolver
/// and process-wide memory mutex as `read_topics`.
///
/// Mirrors `memory::read_for_prompt` (entries) so `prompts::assemble`
/// folds the curated trio the same way it folds remembered entries.
pub fn read_core_for_prompt(
    project_root: &Path,
    byte_cap: usize,
) -> Result<TopicsPromptRead, MemoryStoreError> {
    let _guard = memory_mutex().lock().unwrap_or_else(|e| e.into_inner());

    let mut files = Vec::new();
    let mut used_bytes = 0usize;
    let mut truncated = false;
    for name in ["INDEX.md", "USER.md", "SOUL.md"] {
        let path = resolve_memory_file(project_root, name)?;
        let Some(read) = read_capped_file(&path, MAX_CORE_FILE_BYTES)? else {
            continue;
        };
        if read.truncated {
            // The file itself was larger than its per-file cap; the
            // prompt only ever sees the capped prefix.
            truncated = true;
        }
        let content = read.content.trim().to_string();
        if content.is_empty() {
            continue;
        }
        let bytes = content.len();
        if used_bytes.saturating_add(bytes) > byte_cap {
            truncated = true;
            continue;
        }
        used_bytes += bytes;
        files.push(TopicPromptFile {
            name: name.to_string(),
            content,
        });
    }

    Ok(TopicsPromptRead {
        files,
        used_bytes,
        byte_cap,
        truncated,
    })
}

/// List and read `.plume/memory/topics/*.md`, sorted by name, capped.
/// A missing `topics/` directory is fine (empty list). Symlinked
/// entries — and the `topics/` dir itself if symlinked — are refused
/// so a planted link can't redirect a read outside the project.
fn read_topic_dir(project_root: &Path) -> Result<(Vec<TopicFile>, bool), MemoryStoreError> {
    let topics_dir = resolve_memory_file(project_root, "topics")?;
    // `resolve_memory_file` already refused symlinked `.plume` /
    // `.plume/memory`; also refuse a symlinked `topics/` itself.
    refuse_symlink(&topics_dir, ".plume/memory/topics")?;

    let read_dir = match std::fs::read_dir(&topics_dir) {
        Ok(rd) => rd,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok((Vec::new(), false)),
        Err(e) => {
            return Err(MemoryStoreError(format!(
                "read {}: {}",
                topics_dir.display(),
                e
            )));
        }
    };

    // Collect candidate `*.md` file names first so the result is
    // deterministic (sorted) regardless of directory iteration order.
    let mut names: Vec<String> = Vec::new();
    for entry in read_dir {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        let file_type = match entry.file_type() {
            Ok(t) => t,
            Err(_) => continue,
        };
        // Skip symlinks (could point outside the project) and
        // sub-directories — topics are flat `*.md` files.
        if file_type.is_symlink() || !file_type.is_file() {
            continue;
        }
        let file_name = entry.file_name();
        let Some(name) = file_name.to_str() else {
            continue;
        };
        if !name.to_ascii_lowercase().ends_with(".md") {
            continue;
        }
        names.push(name.to_string());
    }
    names.sort();

    let truncated = names.len() > MAX_TOPIC_FILES;
    names.truncate(MAX_TOPIC_FILES);

    let mut topics = Vec::with_capacity(names.len());
    for name in names {
        let rel = format!("topics/{name}");
        let path = topics_dir.join(&name);
        topics.push(build_topic_file(
            &rel,
            TopicKind::Topic,
            &path,
            MAX_TOPIC_FILE_BYTES,
        )?);
    }
    Ok((topics, truncated))
}

/// Build a `TopicFile` for `path`, reading at most `cap` bytes. Missing
/// → `exists: false`. A symlinked target is refused.
fn build_topic_file(
    name: &str,
    kind: TopicKind,
    path: &Path,
    cap: usize,
) -> Result<TopicFile, MemoryStoreError> {
    match read_capped_file(path, cap)? {
        Some(read) => Ok(TopicFile {
            name: name.to_string(),
            kind,
            exists: true,
            bytes: read.on_disk_bytes,
            truncated: read.truncated,
            content: read.content,
        }),
        None => Ok(TopicFile {
            name: name.to_string(),
            kind,
            exists: false,
            bytes: 0,
            truncated: false,
            content: String::new(),
        }),
    }
}

struct CappedRead {
    content: String,
    on_disk_bytes: u64,
    truncated: bool,
}

/// Read up to `cap` bytes of a regular file, keeping the valid UTF-8
/// prefix (a cap that lands mid-character drops the partial tail rather
/// than panicking or erroring). Refuses symlinks; missing → `None`.
fn read_capped_file(path: &Path, cap: usize) -> Result<Option<CappedRead>, MemoryStoreError> {
    let meta = match std::fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(MemoryStoreError(format!("stat {}: {}", path.display(), e))),
    };
    if meta.file_type().is_symlink() {
        return Err(MemoryStoreError(format!(
            "{} is a symlink; refusing to read memory through it",
            path.display()
        )));
    }
    if !meta.is_file() {
        return Ok(None);
    }
    let on_disk_bytes = meta.len();

    // Read at most cap + 1 bytes so we can detect truncation without
    // slurping a pathologically large file into memory.
    let mut file = std::fs::File::open(path)
        .map_err(|e| MemoryStoreError(format!("open {}: {}", path.display(), e)))?;
    let mut bytes = Vec::new();
    file.by_ref()
        .take(cap as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| MemoryStoreError(format!("read {}: {}", path.display(), e)))?;

    let truncated = bytes.len() > cap || on_disk_bytes > bytes.len() as u64;
    bytes.truncate(cap);
    let content = match std::str::from_utf8(&bytes) {
        Ok(s) => s.to_string(),
        Err(e) => String::from_utf8_lossy(&bytes[..e.valid_up_to()]).to_string(),
    };

    Ok(Some(CappedRead {
        content,
        on_disk_bytes,
        truncated,
    }))
}
