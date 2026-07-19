//! Deterministic provenance checks for internal `[[S1]]` citations.

#![allow(dead_code)] // Task 8 wires verification into the run harness.

use std::collections::{HashMap, HashSet};

use sha2::{Digest, Sha256};

use super::evidence::ResearchEvidenceSource;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CitationStatus {
    Verified,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ParagraphCitations {
    pub line: usize,
    pub source_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CitationVerification {
    pub status: CitationStatus,
    /// Honest boundary: ids prove provenance membership, not relevance/truth.
    pub claim: &'static str,
    pub paragraph_sources: Vec<ParagraphCitations>,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub(crate) enum CitationError {
    #[error("the draft contains malformed citation syntax")]
    MalformedCitation,
    #[error("the draft contains an unclosed fenced code block")]
    UnclosedCodeFence,
    #[error("the draft cites unknown source {0}")]
    UnknownSource(String),
    #[error("accepted source id {0} appears more than once")]
    DuplicateAcceptedSource(String),
    #[error("accepted source {0} no longer matches its immutable extract hash")]
    StaleSourceHash(String),
    #[error("prose beginning on line {line} has no accepted citation")]
    MissingCitation { line: usize },
}

pub(crate) fn verify_citations(
    draft: &str,
    sources: &[ResearchEvidenceSource],
) -> Result<CitationVerification, CitationError> {
    let accepted = validate_sources(sources)?;
    let mut paragraphs = Vec::new();
    let mut prose = String::new();
    let mut prose_line = 0_usize;
    let mut in_fence = false;

    for (index, line) in draft.lines().enumerate() {
        let line_number = index + 1;
        let trimmed = line.trim();
        if trimmed.starts_with("```") {
            flush_prose(&mut paragraphs, &mut prose, prose_line, &accepted)?;
            in_fence = !in_fence;
            continue;
        }
        if in_fence || is_heading(trimmed) {
            flush_prose(&mut paragraphs, &mut prose, prose_line, &accepted)?;
            continue;
        }
        if trimmed.is_empty() {
            flush_prose(&mut paragraphs, &mut prose, prose_line, &accepted)?;
            continue;
        }
        if is_list_item(trimmed) {
            flush_prose(&mut paragraphs, &mut prose, prose_line, &accepted)?;
            paragraphs.push(verify_prose(trimmed, line_number, &accepted)?);
            continue;
        }
        if prose.is_empty() {
            prose_line = line_number;
        } else {
            prose.push('\n');
        }
        prose.push_str(trimmed);
    }
    if in_fence {
        return Err(CitationError::UnclosedCodeFence);
    }
    flush_prose(&mut paragraphs, &mut prose, prose_line, &accepted)?;

    Ok(CitationVerification {
        status: CitationStatus::Verified,
        claim: "provenance-only",
        paragraph_sources: paragraphs,
    })
}

fn validate_sources(
    sources: &[ResearchEvidenceSource],
) -> Result<HashMap<&str, &ResearchEvidenceSource>, CitationError> {
    let mut accepted = HashMap::with_capacity(sources.len());
    for source in sources {
        if accepted.insert(source.source_id.as_str(), source).is_some() {
            return Err(CitationError::DuplicateAcceptedSource(
                source.source_id.clone(),
            ));
        }
        let current = format!("{:x}", Sha256::digest(source.content.as_bytes()));
        if current != source.sha256 {
            return Err(CitationError::StaleSourceHash(source.source_id.clone()));
        }
    }
    Ok(accepted)
}

fn flush_prose(
    paragraphs: &mut Vec<ParagraphCitations>,
    prose: &mut String,
    line: usize,
    accepted: &HashMap<&str, &ResearchEvidenceSource>,
) -> Result<(), CitationError> {
    if !prose.is_empty() {
        paragraphs.push(verify_prose(prose, line, accepted)?);
        prose.clear();
    }
    Ok(())
}

fn verify_prose(
    prose: &str,
    line: usize,
    accepted: &HashMap<&str, &ResearchEvidenceSource>,
) -> Result<ParagraphCitations, CitationError> {
    let markers = extract_markers(prose)?;
    if markers.is_empty() {
        return Err(CitationError::MissingCitation { line });
    }
    let mut unique = HashSet::new();
    let mut source_ids = Vec::new();
    for marker in markers {
        if !accepted.contains_key(marker.as_str()) {
            return Err(CitationError::UnknownSource(marker));
        }
        if unique.insert(marker.clone()) {
            source_ids.push(marker);
        }
    }
    Ok(ParagraphCitations { line, source_ids })
}

fn extract_markers(prose: &str) -> Result<Vec<String>, CitationError> {
    let mut markers = Vec::new();
    let mut cursor = 0_usize;
    while cursor < prose.len() {
        let remaining = &prose[cursor..];
        let next_open = remaining.find("[[");
        let next_close = remaining.find("]]");
        if next_close.is_some_and(|close| next_open.map_or(true, |open| close < open)) {
            return Err(CitationError::MalformedCitation);
        }
        let Some(open) = next_open else {
            break;
        };
        let body_start = cursor + open + 2;
        let close_offset = prose[body_start..]
            .find("]]")
            .ok_or(CitationError::MalformedCitation)?;
        let body_end = body_start + close_offset;
        let body = &prose[body_start..body_end];
        if !valid_source_marker(body) {
            return Err(CitationError::MalformedCitation);
        }
        markers.push(body.to_string());
        cursor = body_end + 2;
    }
    if prose[cursor..].contains("[[") || prose[cursor..].contains("]]") {
        return Err(CitationError::MalformedCitation);
    }
    Ok(markers)
}

fn valid_source_marker(marker: &str) -> bool {
    let Some(number) = marker.strip_prefix('S') else {
        return false;
    };
    if number.is_empty()
        || number.starts_with('0')
        || !number.bytes().all(|byte| byte.is_ascii_digit())
    {
        return false;
    }
    number
        .parse::<u8>()
        .is_ok_and(|value| (1..=10).contains(&value))
}

fn is_list_item(line: &str) -> bool {
    line.starts_with("- ")
        || line.starts_with("* ")
        || line.starts_with("+ ")
        || line.split_once(". ").is_some_and(|(prefix, _)| {
            !prefix.is_empty() && prefix.bytes().all(|b| b.is_ascii_digit())
        })
}

fn is_heading(line: &str) -> bool {
    let hashes = line.bytes().take_while(|byte| *byte == b'#').count();
    (1..=6).contains(&hashes)
        && line
            .as_bytes()
            .get(hashes)
            .map_or(true, |byte| byte.is_ascii_whitespace())
}
