use std::fs;

use crate::browser::evidence::{store_text_evidence, BrowserCaptureKind, CapturedBrowserText};
use crate::browser::local_evidence::{store_local_text_evidence, LocalEvidenceOwner};
use crate::project::OpenProject;
use crate::prompts::{ContextSourceRef, EXPLICIT_CONTEXT_BYTE_CAP};
use crate::sessions::owner::{
    resolve_session_owner, ResolvedSessionOwner, SessionOwnerRef, SessionOwnerScope,
};
use crate::sessions::{self, save_transcript_with_context};

use super::evidence::{resolve_browser_evidence, ResearchEvidenceError};

fn capture(label: &str, bytes: usize) -> CapturedBrowserText {
    CapturedBrowserText {
        capture_kind: BrowserCaptureKind::Page,
        source_url: format!("https://example.com/{label}?secret=removed"),
        title: Some(format!("Source {label}")),
        content: label.repeat(bytes.div_ceil(label.len())),
        source_truncated: false,
    }
}

fn local_owner(temp: &tempfile::TempDir) -> (ResolvedSessionOwner, LocalEvidenceOwner) {
    let sessions_dir = temp.path().join("sessions");
    let session = sessions::create(&sessions_dir, Some("research")).expect("session");
    let owner = resolve_session_owner(
        &SessionOwnerRef {
            scope: SessionOwnerScope::Local,
            session_id: session.id.clone(),
        },
        SessionOwnerScope::Local,
        &sessions_dir,
        None,
    )
    .expect("owner");
    (
        owner,
        LocalEvidenceOwner {
            session_id: session.id,
        },
    )
}

fn save_shelf(owner: &ResolvedSessionOwner, sources: &[ContextSourceRef]) {
    save_transcript_with_context(
        &owner.sessions_dir,
        &owner.session_id,
        &[],
        sources,
        owner.scope == SessionOwnerScope::Project,
    )
    .expect("save shelf");
}

#[test]
fn local_sources_preserve_order_and_mint_stable_run_ids() {
    let temp = tempfile::tempdir().expect("tempdir");
    let (owner, local) = local_owner(&temp);
    let first = store_local_text_evidence(&owner.sessions_dir, &local, capture("first", 100))
        .expect("first evidence");
    let second = store_local_text_evidence(&owner.sessions_dir, &local, capture("second", 120))
        .expect("second evidence");
    let refs = vec![
        ContextSourceRef::BrowserTextEvidence {
            evidence_id: second.evidence_id.clone(),
        },
        ContextSourceRef::BrowserTextEvidence {
            evidence_id: first.evidence_id.clone(),
        },
    ];
    save_shelf(&owner, &refs);

    let resolved = resolve_browser_evidence(&owner, &refs, || None).expect("resolve");
    assert_eq!(resolved[0].source_id, "S1");
    assert_eq!(resolved[0].evidence_id, second.evidence_id);
    assert_eq!(resolved[1].source_id, "S2");
    assert_eq!(resolved[1].evidence_id, first.evidence_id);
    assert_eq!(resolved[0].sha256.len(), 64);
    assert_eq!(resolved[0].bytes as usize, resolved[0].content.len());
    assert!(resolved[0].source_url.starts_with("https://example.com/"));
}

#[test]
fn only_browser_text_evidence_is_an_eligible_source_kind() {
    let temp = tempfile::tempdir().expect("tempdir");
    let (owner, _) = local_owner(&temp);
    for source in [
        ContextSourceRef::UserMemoryEntry {
            entry_id: format!("m_{}", "a".repeat(32)),
        },
        ContextSourceRef::BrowserScreenshotEvidence {
            evidence_id: format!("bs_{}", "a".repeat(32)),
        },
    ] {
        assert!(matches!(
            resolve_browser_evidence(&owner, &[source], || None),
            Err(ResearchEvidenceError::UnsupportedSourceKind)
        ));
    }
}

#[test]
fn project_evidence_requires_shelf_membership_and_current_generation() {
    let temp = tempfile::tempdir().expect("tempdir");
    let project_root = temp.path().join("project");
    fs::create_dir_all(&project_root).expect("project");
    let project_root = fs::canonicalize(project_root).expect("canonical project");
    let project = OpenProject {
        id: "generation-a".into(),
        root: project_root.clone(),
    };
    let project_sessions = sessions::project_sessions_dir(&project_root).expect("sessions dir");
    let session = sessions::create(&project_sessions, Some("research")).expect("session");
    let owner = resolve_session_owner(
        &SessionOwnerRef {
            scope: SessionOwnerScope::Project,
            session_id: session.id,
        },
        SessionOwnerScope::Project,
        &temp.path().join("local"),
        Some(&project),
    )
    .expect("owner");
    let evidence = store_text_evidence(&project_root, capture("project", 100)).expect("evidence");
    let refs = vec![ContextSourceRef::BrowserTextEvidence {
        evidence_id: evidence.evidence_id,
    }];

    assert!(matches!(
        resolve_browser_evidence(&owner, &refs, || Some(project.clone())),
        Err(ResearchEvidenceError::NotOnOwnerShelf)
    ));
    save_shelf(&owner, &refs);
    assert!(resolve_browser_evidence(&owner, &refs, || Some(project.clone())).is_ok());
    let switched = OpenProject {
        id: "generation-b".into(),
        root: project.root.clone(),
    };
    assert!(matches!(
        resolve_browser_evidence(&owner, &refs, || Some(switched.clone())),
        Err(ResearchEvidenceError::StaleProjectGeneration)
    ));
}

#[test]
fn research_resolution_does_not_reuse_the_chat_aggregate_cap() {
    let temp = tempfile::tempdir().expect("tempdir");
    let (owner, local) = local_owner(&temp);
    let mut refs = Vec::new();
    for index in 0..5 {
        let summary = store_local_text_evidence(
            &owner.sessions_dir,
            &local,
            capture(&format!("large-{index}"), 60 * 1024),
        )
        .expect("large evidence");
        refs.push(ContextSourceRef::BrowserTextEvidence {
            evidence_id: summary.evidence_id,
        });
    }
    save_shelf(&owner, &refs);
    let resolved = resolve_browser_evidence(&owner, &refs, || None).expect("resolve large set");
    let bytes: usize = resolved.iter().map(|source| source.content.len()).sum();
    assert!(bytes > EXPLICIT_CONTEXT_BYTE_CAP);
}

#[test]
fn source_count_and_duplicate_identities_are_bounded() {
    let temp = tempfile::tempdir().expect("tempdir");
    let (owner, _) = local_owner(&temp);
    let id = format!("be_{}", "a".repeat(32));
    let duplicate = ContextSourceRef::BrowserTextEvidence {
        evidence_id: id.clone(),
    };
    assert!(matches!(
        resolve_browser_evidence(&owner, &[duplicate.clone(), duplicate], || None),
        Err(ResearchEvidenceError::DuplicateSource)
    ));
    let too_many = (0..11)
        .map(|index| ContextSourceRef::BrowserTextEvidence {
            evidence_id: format!("be_{index:032x}"),
        })
        .collect::<Vec<_>>();
    assert!(matches!(
        resolve_browser_evidence(&owner, &too_many, || None),
        Err(ResearchEvidenceError::SourceCount)
    ));
}
