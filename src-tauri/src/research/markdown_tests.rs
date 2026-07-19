use sha2::{Digest, Sha256};

use crate::browser::evidence::BrowserCaptureKind;
use crate::research::evidence::ResearchEvidenceSource;

use super::markdown::{
    project_markdown, project_markdown_for_review, MarkdownProjectionError, MAX_ARTIFACT_BYTES,
};

fn source(id: &str, title: &str, url: &str) -> ResearchEvidenceSource {
    let content = format!("content for {id}");
    ResearchEvidenceSource {
        source_id: id.into(),
        evidence_id: format!("be_{}", "a".repeat(32)),
        capture_kind: BrowserCaptureKind::Page,
        source_url: url.into(),
        title: Some(title.into()),
        captured_at_ms: 1,
        sha256: format!("{:x}", Sha256::digest(content.as_bytes())),
        bytes: content.len() as u64,
        content,
        redaction_count: 0,
        truncated: false,
    }
}

#[test]
fn projection_converts_internal_ids_and_appends_rust_owned_footnotes() {
    let sources = vec![
        source("S1", "First [source]", "https://example.com/one"),
        source("S2", "Second\nsource", "https://example.com/two_(x)"),
    ];
    let projected = project_markdown("# Note\n\nClaim. [[S2]][[S1]]", &sources).unwrap();
    assert_eq!(
        projected,
        "# Note\n\nClaim. [^S2][^S1]\n\n## Sources\n\n[^S1]: [First \\[source\\]](<https://example.com/one>)\n[^S2]: [Second source](<https://example.com/two_(x)>)\n"
    );
}

#[test]
fn model_supplied_sources_section_is_rejected() {
    let sources = vec![source("S1", "One", "https://example.com")];
    assert!(matches!(
        project_markdown("Claim. [[S1]]\n\n## Sources\nFake", &sources),
        Err(MarkdownProjectionError::ModelSuppliedSources)
    ));
}

#[test]
fn projection_keeps_exact_export_bytes_and_enforces_the_artifact_cap() {
    let sources = vec![source("S1", "One", "https://example.com")];
    let first = project_markdown("Claim. [[S1]]", &sources).unwrap();
    let second = project_markdown("Claim. [[S1]]\n", &sources).unwrap();
    assert_eq!(first, second);
    assert!(first.ends_with('\n'));

    let oversized = format!("{} [[S1]]", "x".repeat(MAX_ARTIFACT_BYTES));
    assert!(matches!(
        project_markdown(&oversized, &sources),
        Err(MarkdownProjectionError::ArtifactTooLarge)
    ));
}

#[test]
fn review_needed_projection_stays_exportable_without_claiming_verification() {
    let sources = vec![source("S1", "One", "https://example.com")];
    let draft = "Cited claim [[S1]].\n\nUncited claim.";
    let projected =
        project_markdown_for_review(draft, &sources).expect("review-needed draft remains usable");
    assert!(projected.contains("[^S1]"));
    assert!(projected.contains("## Sources"));
    assert!(matches!(
        project_markdown(draft, &sources),
        Err(MarkdownProjectionError::Citation(_))
    ));
}
