//! Read-only context preview assembly.

use std::collections::HashSet;
use std::path::Path;

use crate::error::IpcError;

use super::{
    memory_summary, preview_attachment, read_ambient_memory, read_topics_summary,
    AttachmentPreviewOutcome, AttachmentRequest, ContextPreview, InstructionsSummary,
};
use crate::prompts::explicit_context::{
    resolve_explicit_context_for_preview_with_local_owner,
    resolve_explicit_context_for_preview_with_stores, ContextSourceManifestItem,
    ContextSourcePreviewOutcome, ContextSourceRef, ExplicitContextStores,
};
use crate::prompts::instructions::{read_project_instructions, INSTRUCTIONS_FILENAME};

pub fn preview_context(
    project_root: Option<&Path>,
    attachment: Option<AttachmentRequest>,
) -> ContextPreview {
    preview_context_with_sources(project_root, attachment, &[])
}

pub fn preview_context_with_sources(
    project_root: Option<&Path>,
    attachment: Option<AttachmentRequest>,
    context_sources: &[ContextSourceRef],
) -> ContextPreview {
    preview_context_with_sources_owned(project_root, None, None, attachment, context_sources)
}

pub fn preview_context_with_sources_and_stores(
    stores: ExplicitContextStores<'_>,
    attachment: Option<AttachmentRequest>,
    context_sources: &[ContextSourceRef],
) -> ContextPreview {
    preview_context_with_sources_owned(
        stores.project_root,
        stores.local_browser_owner,
        Some(stores.user_memory_dir),
        attachment,
        context_sources,
    )
}

fn preview_context_with_sources_owned(
    project_root: Option<&Path>,
    local_owner: Option<(&Path, &str)>,
    user_memory_dir: Option<&Path>,
    attachment: Option<AttachmentRequest>,
    context_sources: &[ContextSourceRef],
) -> ContextPreview {
    let instructions = project_root
        .and_then(read_project_instructions)
        .and_then(|content| {
            if content.content.trim().is_empty() {
                return None;
            }
            Some(InstructionsSummary {
                source: INSTRUCTIONS_FILENAME.to_string(),
                original_bytes: content.original_bytes,
                redaction_count: content.redactions.len(),
            })
        });

    let attachment_outcome = match (attachment, project_root) {
        (None, _) => None,
        (Some(req), Some(root)) => Some(preview_attachment(root, req)),
        (Some(AttachmentRequest::ProjectFile { rel_path, .. }), None) => {
            Some(AttachmentPreviewOutcome::Blocked {
                rel_path,
                error: IpcError::NeedsApproval,
            })
        }
    };

    let explicit_context = match user_memory_dir {
        Some(user_memory_dir) => resolve_explicit_context_for_preview_with_stores(
            ExplicitContextStores {
                project_root,
                user_memory_dir,
                local_browser_owner: local_owner,
            },
            context_sources,
        ),
        None => resolve_explicit_context_for_preview_with_local_owner(
            project_root,
            local_owner,
            context_sources,
        ),
    };
    let explicit_memory_ids = explicit_context
        .iter()
        .filter_map(|outcome| match outcome {
            ContextSourcePreviewOutcome::Ready(ContextSourceManifestItem::MemoryEntry {
                entry_id,
                ..
            }) => Some(entry_id.clone()),
            _ => None,
        })
        .collect::<HashSet<_>>();
    let memory = project_root
        .and_then(|root| read_ambient_memory(root, &explicit_memory_ids))
        .map(|read| memory_summary(&read));
    let topics = project_root.and_then(read_topics_summary);
    ContextPreview {
        instructions,
        attachment: attachment_outcome,
        memory,
        topics,
        explicit_context,
    }
}
