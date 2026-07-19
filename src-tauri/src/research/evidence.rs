//! Exact, session-owned Browser text evidence for Stage A research.

#![allow(dead_code)] // Task 8 wires the resolver into the run harness.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::browser::evidence::{self, BrowserCaptureKind, BrowserEvidenceRecord};
use crate::browser::local_evidence::{read_local_text_evidence, LocalEvidenceOwner};
use crate::project::OpenProject;
use crate::prompts::ContextSourceRef;
use crate::sessions::owner::{ResolvedSessionOwner, SessionOwnerScope};
use crate::sessions::{self, SessionRecord};

pub(crate) const MAX_RESEARCH_SOURCES: usize = 10;
pub(crate) const MAX_RESEARCH_SOURCE_BYTES: usize = 64 * 1024;
pub(crate) const MAX_RESEARCH_TOTAL_BYTES: usize = 4 * 1024 * 1024;

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ResearchEvidenceSource {
    pub source_id: String,
    pub evidence_id: String,
    pub capture_kind: BrowserCaptureKind,
    pub source_url: String,
    pub title: Option<String>,
    pub captured_at_ms: u64,
    pub content: String,
    pub sha256: String,
    pub bytes: u64,
    pub redaction_count: u64,
    pub truncated: bool,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub(crate) enum ResearchEvidenceError {
    #[error("research requires between one and ten Browser text sources")]
    SourceCount,
    #[error("research accepts Browser text evidence only")]
    UnsupportedSourceKind,
    #[error("research source ids must be unique")]
    DuplicateSource,
    #[error("a requested source is not on the owning session's current shelf")]
    NotOnOwnerShelf,
    #[error("the owning project changed before evidence resolution finished")]
    StaleProjectGeneration,
    #[error("research evidence is missing or no longer readable")]
    EvidenceUnavailable,
    #[error("a research source exceeds the 64 KiB cap")]
    SourceTooLarge,
    #[error("the research evidence set exceeds the 4 MiB cap")]
    TotalTooLarge,
    #[error("the owning session changed or could not be read")]
    OwnerUnavailable,
}

pub(crate) fn resolve_browser_evidence(
    owner: &ResolvedSessionOwner,
    sources: &[ContextSourceRef],
    mut current_trusted_project: impl FnMut() -> Option<OpenProject>,
) -> Result<Vec<ResearchEvidenceSource>, ResearchEvidenceError> {
    if sources.is_empty() || sources.len() > MAX_RESEARCH_SOURCES {
        return Err(ResearchEvidenceError::SourceCount);
    }
    let evidence_ids = exact_text_evidence_ids(sources)?;
    if evidence_ids.iter().collect::<HashSet<_>>().len() != evidence_ids.len() {
        return Err(ResearchEvidenceError::DuplicateSource);
    }
    verify_project_generation(owner, current_trusted_project())?;
    let session = load_owner_session(owner)?;
    verify_shelf_membership(&session, &evidence_ids)?;

    let mut total_bytes = 0_usize;
    let mut resolved = Vec::with_capacity(evidence_ids.len());
    for (index, evidence_id) in evidence_ids.iter().enumerate() {
        let record = read_owned_record(owner, evidence_id)?;
        let bytes = record.content.len();
        if bytes > MAX_RESEARCH_SOURCE_BYTES {
            return Err(ResearchEvidenceError::SourceTooLarge);
        }
        total_bytes = total_bytes
            .checked_add(bytes)
            .ok_or(ResearchEvidenceError::TotalTooLarge)?;
        if total_bytes > MAX_RESEARCH_TOTAL_BYTES {
            return Err(ResearchEvidenceError::TotalTooLarge);
        }
        resolved.push(project_record(index, record));
    }

    // The project and shelf are mutable while disk reads occur. Re-check both
    // at the last possible boundary so the returned vector belongs to the
    // same project generation and explicit source selection it started with.
    verify_project_generation(owner, current_trusted_project())?;
    let current_session = load_owner_session(owner)?;
    verify_shelf_membership(&current_session, &evidence_ids)?;
    Ok(resolved)
}

fn exact_text_evidence_ids(
    sources: &[ContextSourceRef],
) -> Result<Vec<String>, ResearchEvidenceError> {
    sources
        .iter()
        .map(|source| match source {
            ContextSourceRef::BrowserTextEvidence { evidence_id } => Ok(evidence_id.clone()),
            _ => Err(ResearchEvidenceError::UnsupportedSourceKind),
        })
        .collect()
}

fn verify_project_generation(
    owner: &ResolvedSessionOwner,
    current: Option<OpenProject>,
) -> Result<(), ResearchEvidenceError> {
    match (&owner.project, current) {
        (None, _) if owner.scope == SessionOwnerScope::Local => Ok(()),
        (Some(expected), Some(current))
            if expected.id == current.id && expected.root == current.root =>
        {
            Ok(())
        }
        _ => Err(ResearchEvidenceError::StaleProjectGeneration),
    }
}

fn load_owner_session(
    owner: &ResolvedSessionOwner,
) -> Result<SessionRecord, ResearchEvidenceError> {
    sessions::load(&owner.sessions_dir, &owner.session_id)
        .map_err(|_| ResearchEvidenceError::OwnerUnavailable)
}

fn verify_shelf_membership(
    session: &SessionRecord,
    evidence_ids: &[String],
) -> Result<(), ResearchEvidenceError> {
    let shelf_ids = session
        .context_sources
        .iter()
        .filter_map(|source| match source {
            ContextSourceRef::BrowserTextEvidence { evidence_id } => Some(evidence_id.as_str()),
            _ => None,
        })
        .collect::<HashSet<_>>();
    if evidence_ids
        .iter()
        .all(|evidence_id| shelf_ids.contains(evidence_id.as_str()))
    {
        Ok(())
    } else {
        Err(ResearchEvidenceError::NotOnOwnerShelf)
    }
}

fn read_owned_record(
    owner: &ResolvedSessionOwner,
    evidence_id: &str,
) -> Result<BrowserEvidenceRecord, ResearchEvidenceError> {
    match owner.scope {
        SessionOwnerScope::Local => read_local_text_evidence(
            &owner.sessions_dir,
            &LocalEvidenceOwner {
                session_id: owner.session_id.clone(),
            },
            evidence_id,
        )
        .map_err(|_| ResearchEvidenceError::EvidenceUnavailable)?
        .ok_or(ResearchEvidenceError::EvidenceUnavailable),
        SessionOwnerScope::Project => {
            let root = &owner
                .project
                .as_ref()
                .ok_or(ResearchEvidenceError::StaleProjectGeneration)?
                .root;
            evidence::read_text_evidence(root, evidence_id)
                .map_err(|_| ResearchEvidenceError::EvidenceUnavailable)?
                .ok_or(ResearchEvidenceError::EvidenceUnavailable)
        }
    }
}

fn project_record(index: usize, record: BrowserEvidenceRecord) -> ResearchEvidenceSource {
    let sha256 = format!("{:x}", Sha256::digest(record.content.as_bytes()));
    ResearchEvidenceSource {
        source_id: format!("S{}", index + 1),
        evidence_id: record.id,
        capture_kind: record.capture_kind,
        source_url: record.source_url,
        title: record.title,
        captured_at_ms: record.captured_at_ms,
        content: record.content,
        sha256,
        bytes: record.bytes,
        redaction_count: record.redaction_count,
        truncated: record.truncated,
    }
}
