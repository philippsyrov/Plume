//! Rust-owned projection from internal citations to export Markdown.

use super::citations::{verify_citations, CitationError};
use super::evidence::ResearchEvidenceSource;

pub(crate) const MAX_ARTIFACT_BYTES: usize = 512 * 1024;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub(crate) enum MarkdownProjectionError {
    #[error("the model draft must not supply its own Sources section")]
    ModelSuppliedSources,
    #[error("the projected Markdown artifact exceeds its byte cap")]
    ArtifactTooLarge,
    #[error(transparent)]
    Citation(#[from] CitationError),
}

pub(crate) fn project_markdown(
    draft: &str,
    sources: &[ResearchEvidenceSource],
) -> Result<String, MarkdownProjectionError> {
    if contains_sources_heading(draft) {
        return Err(MarkdownProjectionError::ModelSuppliedSources);
    }
    verify_citations(draft, sources)?;
    project_without_verification(draft, sources)
}

pub(crate) fn project_markdown_for_review(
    draft: &str,
    sources: &[ResearchEvidenceSource],
) -> Result<String, MarkdownProjectionError> {
    project_without_verification(draft, sources)
}

fn project_without_verification(
    draft: &str,
    sources: &[ResearchEvidenceSource],
) -> Result<String, MarkdownProjectionError> {
    if contains_sources_heading(draft) {
        return Err(MarkdownProjectionError::ModelSuppliedSources);
    }

    let mut projected = draft.trim_end().to_string();
    for source in sources {
        projected = projected.replace(
            &format!("[[{}]]", source.source_id),
            &format!("[^{}]", source.source_id),
        );
    }
    projected.push_str("\n\n## Sources\n\n");
    for source in sources {
        let title = escape_link_label(
            source
                .title
                .as_deref()
                .unwrap_or(source.source_url.as_str()),
        );
        let url = escape_angle_url(&source.source_url);
        projected.push_str(&format!(
            "[^{}]: [{}](<{}>)\n",
            source.source_id, title, url
        ));
    }
    if projected.len() > MAX_ARTIFACT_BYTES {
        return Err(MarkdownProjectionError::ArtifactTooLarge);
    }
    Ok(projected)
}

fn contains_sources_heading(draft: &str) -> bool {
    let mut in_fence = false;
    draft.lines().any(|line| {
        let trimmed = line.trim();
        if trimmed.starts_with("```") {
            in_fence = !in_fence;
            return false;
        }
        if in_fence {
            return false;
        }
        let heading = trimmed.trim_start_matches('#').trim();
        trimmed.starts_with('#') && heading.eq_ignore_ascii_case("sources")
    })
}

fn escape_link_label(value: &str) -> String {
    value
        .replace(['\r', '\n'], " ")
        .replace('\\', "\\\\")
        .replace('[', "\\[")
        .replace(']', "\\]")
}

fn escape_angle_url(value: &str) -> String {
    value.replace('<', "%3C").replace('>', "%3E")
}
