//! User-managed links from a flat memory entry to curated topic notes.
//! Links are organization metadata only and never alter prompt selection.

use std::path::Path;

use serde::Serialize;

use super::topics::{read_topics_unlocked, MAX_TOPIC_FILE_BYTES};
use super::{
    is_valid_entry_id, memory_mutex, read_entries, resolve_entries_path, serialize_entries,
    write_atomic, MemoryEntry,
};

pub const MAX_LINKS: usize = 5;

#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum MemorySetLinksResponse {
    Ok(MemorySetLinksOk),
    Err(MemorySetLinksErr),
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemorySetLinksOk {
    pub ok: bool,
    pub entry: MemoryEntry,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemorySetLinksErr {
    pub ok: bool,
    pub reason: MemorySetLinksFailure,
    pub message: String,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum MemorySetLinksFailure {
    BadId,
    NotFound,
    CapacityReached,
    TooMany,
    Duplicate,
    InvalidTopic,
    TopicNotFound,
    StoreFailed,
}

pub fn set_links(project_root: &Path, id: &str, links: &[String]) -> MemorySetLinksResponse {
    if !is_valid_entry_id(id) {
        return err(MemorySetLinksFailure::BadId, "invalid memory entry id");
    }
    if links.len() > MAX_LINKS {
        return err(
            MemorySetLinksFailure::TooMany,
            "at most 5 topic links are allowed",
        );
    }

    let mut canonical = links.to_vec();
    canonical.sort();
    if canonical.windows(2).any(|pair| pair[0] == pair[1]) {
        return err(
            MemorySetLinksFailure::Duplicate,
            "topic links must be unique",
        );
    }
    for link in &canonical {
        if !valid_topic_name(link) {
            return err(
                MemorySetLinksFailure::InvalidTopic,
                format!("invalid curated topic reference: {link:?}"),
            );
        }
    }

    let _guard = memory_mutex().lock().unwrap_or_else(|e| e.into_inner());
    let entries_path = match resolve_entries_path(project_root) {
        Ok(path) => path,
        Err(error) => return err(MemorySetLinksFailure::StoreFailed, error.0),
    };
    let (mut entries, _) = match read_entries(&entries_path) {
        Ok(result) => result,
        Err(error) => return err(MemorySetLinksFailure::StoreFailed, error.0),
    };
    let Some(position) = entries.iter().position(|entry| entry.id == id) else {
        return err(
            MemorySetLinksFailure::NotFound,
            "memory entry was not found",
        );
    };

    if !canonical.is_empty() {
        let surfaced = match read_topics_unlocked(project_root) {
            Ok(topics) => topics.topics,
            Err(error) => return err(MemorySetLinksFailure::InvalidTopic, error.0),
        };
        for link in &canonical {
            let path = project_root.join(".plume").join("memory").join(link);
            if let Err((reason, message)) = validate_topic_file(&path) {
                return err(reason, message);
            }
            if !surfaced
                .iter()
                .any(|topic| topic.name == *link && !topic.truncated)
            {
                return err(
                    MemorySetLinksFailure::TopicNotFound,
                    format!("curated topic is not currently surfaced: {link}"),
                );
            }
        }
    }

    entries[position].links = canonical;
    let updated = entries[position].clone();
    let serialized = match serialize_entries(&entries) {
        Ok(raw) => raw,
        Err(error) => return err(MemorySetLinksFailure::StoreFailed, error.0),
    };
    if serialized.len() as u64 > super::MAX_BYTES_TOTAL {
        return err(
            MemorySetLinksFailure::CapacityReached,
            format!(
                "memory store would be {} bytes; max is {}",
                serialized.len(),
                super::MAX_BYTES_TOTAL
            ),
        );
    }
    if let Err(error) = write_atomic(&entries_path, serialized.as_bytes()) {
        return err(MemorySetLinksFailure::StoreFailed, error.0);
    }
    MemorySetLinksResponse::Ok(MemorySetLinksOk {
        ok: true,
        entry: updated,
    })
}

fn valid_topic_name(link: &str) -> bool {
    let Some(filename) = link.strip_prefix("topics/") else {
        return false;
    };
    !filename.is_empty()
        && !filename.starts_with('.')
        && !filename.contains('/')
        && !filename.contains('\\')
        && filename.ends_with(".md")
        && filename != ".md"
}

fn validate_topic_file(path: &Path) -> Result<(), (MemorySetLinksFailure, String)> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err((
                MemorySetLinksFailure::TopicNotFound,
                "curated topic does not exist".to_string(),
            ));
        }
        Err(error) => {
            return Err((
                MemorySetLinksFailure::InvalidTopic,
                format!("topic is unavailable: {error}"),
            ));
        }
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err((
            MemorySetLinksFailure::InvalidTopic,
            "topic must be a regular non-symlink file".to_string(),
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.nlink() != 1 {
            return Err((
                MemorySetLinksFailure::InvalidTopic,
                "topic hardlinks are not allowed".to_string(),
            ));
        }
    }
    if metadata.len() > MAX_TOPIC_FILE_BYTES as u64 {
        return Err((
            MemorySetLinksFailure::InvalidTopic,
            format!("topic exceeds the {MAX_TOPIC_FILE_BYTES}-byte cap"),
        ));
    }
    Ok(())
}

fn err(reason: MemorySetLinksFailure, message: impl Into<String>) -> MemorySetLinksResponse {
    MemorySetLinksResponse::Err(MemorySetLinksErr {
        ok: false,
        reason,
        message: message.into(),
    })
}
