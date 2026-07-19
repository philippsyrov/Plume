use crate::browser::evidence::BrowserCaptureKind;
use crate::research::evidence::ResearchEvidenceSource;

use super::citations::{verify_citations, CitationError, CitationStatus};

fn source(id: &str, content: &str) -> ResearchEvidenceSource {
    ResearchEvidenceSource {
        source_id: id.into(),
        evidence_id: format!("be_{}", "a".repeat(32)),
        capture_kind: BrowserCaptureKind::Page,
        source_url: format!("https://example.com/{id}"),
        title: Some(format!("Source {id}")),
        captured_at_ms: 1,
        content: content.into(),
        sha256: format!("{:x}", sha2::Sha256::digest(content.as_bytes())),
        bytes: content.len() as u64,
        redaction_count: 0,
        truncated: false,
    }
}

use sha2::Digest;

#[test]
fn every_prose_paragraph_and_list_item_requires_a_known_source() {
    let sources = vec![source("S1", "one"), source("S2", "two")];
    let draft = "# Heading\n\nFirst claim. [[S1]]\ncontinued.\n\n- Item one [[S2]]\n- Item two [[S1]][[S2]]\n\n```rust\nlet uncited = true;\n```";
    let verified = verify_citations(draft, &sources).expect("verified");
    assert_eq!(verified.status, CitationStatus::Verified);
    assert_eq!(verified.paragraph_sources.len(), 3);

    assert!(matches!(
        verify_citations("Claim without evidence.", &sources),
        Err(CitationError::MissingCitation { .. })
    ));
}

#[test]
fn malformed_unknown_and_unaccepted_markers_fail_closed() {
    let sources = vec![source("S1", "one")];
    for malformed in ["Claim [[S1].", "Claim [[S01]].", "Claim [[Sx]]."] {
        assert!(matches!(
            verify_citations(malformed, &sources),
            Err(CitationError::MalformedCitation)
        ));
    }
    assert!(matches!(
        verify_citations("Claim [[S2]].", &sources),
        Err(CitationError::UnknownSource(source)) if source == "S2"
    ));
}

#[test]
fn duplicate_accepted_ids_and_stale_hashes_are_rejected() {
    let first = source("S1", "one");
    assert!(matches!(
        verify_citations("Claim [[S1]].", &[first.clone(), first.clone()]),
        Err(CitationError::DuplicateAcceptedSource(source)) if source == "S1"
    ));

    let mut stale = first;
    stale.content.push_str(" changed");
    assert!(matches!(
        verify_citations("Claim [[S1]].", &[stale]),
        Err(CitationError::StaleSourceHash(source)) if source == "S1"
    ));
}

#[test]
fn verifier_claims_provenance_not_relevance_or_truth() {
    let sources = vec![source("S1", "The sky is blue")];
    let verified = verify_citations("The moon is cheese. [[S1]]", &sources).unwrap();
    assert_eq!(verified.status, CitationStatus::Verified);
    assert_eq!(verified.claim, "provenance-only");
}

#[test]
fn unclosed_code_fences_cannot_hide_uncited_prose() {
    let sources = vec![source("S1", "one")];
    assert!(matches!(
        verify_citations("Claim [[S1]]\n\n```text\nhidden", &sources),
        Err(CitationError::UnclosedCodeFence)
    ));
    assert!(matches!(
        verify_citations("#not-a-heading", &sources),
        Err(CitationError::MissingCitation { .. })
    ));
}
