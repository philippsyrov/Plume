use crate::agent::protocol::ProviderFraming;
use crate::browser::evidence::BrowserCaptureKind;
use crate::research::budget::{BudgetRefusal, RecoveryReason, ResearchBudget};
use crate::research::evidence::ResearchEvidenceSource;
use crate::research::model::ModelCapabilities;

use super::context::{
    pack_source_summary, pack_synthesis, reserve_overflow_repack, PackingAttempt, PackingError,
    SummaryForSynthesis, TokenCounter,
};

struct ByteCounter;

impl TokenCounter for ByteCounter {
    fn count(&self, text: &str) -> u64 {
        text.len() as u64
    }
}

fn source(id: &str, content: &str) -> ResearchEvidenceSource {
    ResearchEvidenceSource {
        source_id: id.into(),
        evidence_id: format!("be_{}", "a".repeat(32)),
        capture_kind: BrowserCaptureKind::Page,
        source_url: format!("https://example.com/{id}"),
        title: Some(format!("Source {id}")),
        captured_at_ms: 1,
        content: content.into(),
        sha256: "ab".repeat(32),
        bytes: content.len() as u64,
        redaction_count: 0,
        truncated: false,
    }
}

fn apple() -> ModelCapabilities {
    ModelCapabilities {
        context_tokens: 4096,
        exact_token_count: false,
    }
}

fn qwen() -> ModelCapabilities {
    ModelCapabilities {
        context_tokens: 8192,
        exact_token_count: false,
    }
}

#[test]
fn apple_uses_4096_fallback_and_qwen_retains_more_source_context() {
    let evidence = source("S1", &"x".repeat(20_000));
    let apple_pack = pack_source_summary(
        &evidence,
        apple(),
        ProviderFraming::AppleInstructions,
        &ByteCounter,
        PackingAttempt::Initial,
    )
    .expect("Apple pack");
    let qwen_pack = pack_source_summary(
        &evidence,
        qwen(),
        ProviderFraming::QwenChatMl,
        &ByteCounter,
        PackingAttempt::Initial,
    )
    .expect("Qwen pack");
    assert_eq!(apple_pack.manifest.context_tokens, 4096);
    assert_eq!(qwen_pack.manifest.context_tokens, 8192);
    assert!(qwen_pack.manifest.retained_source_bytes > apple_pack.manifest.retained_source_bytes);
}

#[test]
fn source_trimming_is_utf8_safe_ordered_and_visible() {
    let evidence = source("S1", &"🪶abc".repeat(3_000));
    let packed = pack_source_summary(
        &evidence,
        apple(),
        ProviderFraming::AppleInstructions,
        &ByteCounter,
        PackingAttempt::Initial,
    )
    .expect("pack");
    assert_eq!(packed.messages.len(), 2);
    assert!(packed.messages[1].content.contains("Source S1"));
    assert!(packed.messages[1].content.contains("[truncated by Plume]"));
    assert!(packed.manifest.truncated);
    assert!(std::str::from_utf8(packed.messages[1].content.as_bytes()).is_ok());
}

#[test]
fn synthesis_contains_every_summary_in_source_order_and_no_raw_evidence() {
    let summaries = vec![
        SummaryForSynthesis {
            source_id: "S2".into(),
            summary: "second summary".into(),
        },
        SummaryForSynthesis {
            source_id: "S1".into(),
            summary: "first summary".into(),
        },
    ];
    let packed = pack_synthesis(
        &summaries,
        qwen(),
        ProviderFraming::QwenChatMl,
        &ByteCounter,
        PackingAttempt::Initial,
    )
    .expect("synthesis");
    let prompt = &packed.messages[1].content;
    assert!(prompt.find("Summary S2").unwrap() < prompt.find("Summary S1").unwrap());
    assert!(prompt.contains("second summary"));
    assert!(prompt.contains("first summary"));
    assert!(!prompt.contains("raw-browser-body"));
    assert_eq!(packed.manifest.included_source_ids, ["S2", "S1"]);
}

#[test]
fn recovery_repack_is_smaller_and_competes_with_malformed_reask() {
    let evidence = source("S1", &"x".repeat(20_000));
    let initial = pack_source_summary(
        &evidence,
        apple(),
        ProviderFraming::AppleInstructions,
        &ByteCounter,
        PackingAttempt::Initial,
    )
    .unwrap();
    let recovery = pack_source_summary(
        &evidence,
        apple(),
        ProviderFraming::AppleInstructions,
        &ByteCounter,
        PackingAttempt::Recovery,
    )
    .unwrap();
    assert!(recovery.manifest.retained_source_bytes < initial.manifest.retained_source_bytes);

    let mut budget = ResearchBudget::default();
    budget.begin_logical_turn().unwrap();
    budget.reserve_provider_call().unwrap();
    budget
        .reserve_recovery(RecoveryReason::MalformedFraming)
        .unwrap();
    assert_eq!(
        reserve_overflow_repack(&mut budget),
        Err(BudgetRefusal::RecoveryAlreadyUsed)
    );
}

#[test]
fn synthesis_refuses_instead_of_silently_omitting_a_source() {
    let summaries = (1..=10)
        .map(|index| SummaryForSynthesis {
            source_id: format!("S{index}"),
            summary: "summary".into(),
        })
        .collect::<Vec<_>>();
    assert!(matches!(
        pack_synthesis(
            &summaries,
            ModelCapabilities {
                context_tokens: 64,
                exact_token_count: false,
            },
            ProviderFraming::AppleInstructions,
            &ByteCounter,
            PackingAttempt::Initial,
        ),
        Err(PackingError::ContextTooSmall)
    ));
}
