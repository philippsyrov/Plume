//! Exact, content-safe source manifests for bounded project context.

use crate::memory;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemorySummary {
    pub entry_count: usize,
    pub used_bytes: usize,
    pub byte_cap: usize,
    pub truncated: bool,
    pub entries: Vec<MemoryContextEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryContextEntry {
    pub id: String,
    pub created_at_ms: u64,
    pub text_bytes: usize,
    pub preview: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TopicsSummary {
    pub file_count: usize,
    pub used_bytes: usize,
    pub byte_cap: usize,
    pub truncated: bool,
    pub files: Vec<TopicContextFile>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TopicContextFile {
    pub name: String,
    pub bytes: usize,
}

pub(super) fn memory_context_entries(read: &memory::MemoryPromptRead) -> Vec<MemoryContextEntry> {
    read.entries
        .iter()
        .map(|entry| MemoryContextEntry {
            id: entry.id.clone(),
            created_at_ms: entry.created_ms,
            text_bytes: entry.text.len(),
            preview: entry
                .text
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
                .chars()
                .take(120)
                .collect(),
        })
        .collect()
}

pub(super) fn topic_context_files(read: &memory::TopicsPromptRead) -> Vec<TopicContextFile> {
    read.files
        .iter()
        .map(|file| TopicContextFile {
            name: file.name.clone(),
            bytes: file.content.len(),
        })
        .collect()
}
