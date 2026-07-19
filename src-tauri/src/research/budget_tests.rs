//! Tests for logical-turn, recovery, and physical provider-call ceilings.

use super::{
    BudgetRefusal, RecoveryReason, ResearchBudget, MAX_LOGICAL_TURNS, MAX_PROVIDER_CALLS,
    MAX_RECOVERY_CALLS,
};

#[test]
fn allows_thirteen_logical_turns_and_refuses_the_fourteenth() {
    let mut budget = ResearchBudget::default();

    for _ in 0..MAX_LOGICAL_TURNS {
        budget
            .begin_logical_turn()
            .expect("each planned logical turn should fit");
        budget
            .reserve_provider_call()
            .expect("each logical turn gets its base provider call");
    }

    assert_eq!(
        budget.begin_logical_turn(),
        Err(BudgetRefusal::LogicalTurnLimit),
    );
    assert_eq!(budget.snapshot().logical_turns, MAX_LOGICAL_TURNS);
}

#[test]
fn malformed_and_overflow_recoveries_compete_for_one_turn_allowance() {
    let mut budget = ResearchBudget::default();
    budget.begin_logical_turn().unwrap();
    budget.reserve_provider_call().unwrap();
    budget
        .reserve_recovery(RecoveryReason::MalformedFraming)
        .expect("the first recovery should fit");

    assert_eq!(
        budget.reserve_recovery(RecoveryReason::ContextOverflow),
        Err(BudgetRefusal::RecoveryAlreadyUsed),
    );
    assert_eq!(budget.snapshot().recovery_calls, 1);
}

#[test]
fn reaches_the_exact_thirteen_recovery_and_twenty_six_provider_call_caps() {
    let mut budget = ResearchBudget::default();

    for turn in 0..MAX_LOGICAL_TURNS {
        budget.begin_logical_turn().unwrap();
        budget.reserve_provider_call().unwrap();
        let reason = if turn % 2 == 0 {
            RecoveryReason::MalformedFraming
        } else {
            RecoveryReason::ContextOverflow
        };
        budget.reserve_recovery(reason).unwrap();
        budget.reserve_provider_call().unwrap();
    }

    let snapshot = budget.snapshot();
    assert_eq!(snapshot.logical_turns, MAX_LOGICAL_TURNS);
    assert_eq!(snapshot.recovery_calls, MAX_RECOVERY_CALLS);
    assert_eq!(snapshot.provider_calls, MAX_PROVIDER_CALLS);
    assert_eq!(
        budget.reserve_provider_call(),
        Err(BudgetRefusal::ProviderCallLimit),
    );
}

#[test]
fn rejected_recovery_does_not_consume_a_provider_call() {
    let mut budget = ResearchBudget::default();
    budget.begin_logical_turn().unwrap();
    budget.reserve_provider_call().unwrap();
    budget
        .reserve_recovery(RecoveryReason::MalformedFraming)
        .unwrap();
    budget.reserve_provider_call().unwrap();
    let calls_before_rejection = budget.snapshot().provider_calls;

    assert_eq!(
        budget.reserve_recovery(RecoveryReason::ContextOverflow),
        Err(BudgetRefusal::RecoveryAlreadyUsed),
    );
    assert_eq!(budget.snapshot().provider_calls, calls_before_rejection);
}

#[test]
fn recovery_requires_an_active_logical_turn() {
    let mut budget = ResearchBudget::default();

    assert_eq!(
        budget.reserve_recovery(RecoveryReason::MalformedFraming),
        Err(BudgetRefusal::NoActiveLogicalTurn),
    );
}

#[test]
fn provider_calls_require_a_turn_and_a_reserved_recovery() {
    let mut budget = ResearchBudget::default();
    assert_eq!(
        budget.reserve_provider_call(),
        Err(BudgetRefusal::NoActiveLogicalTurn),
    );

    budget.begin_logical_turn().unwrap();
    budget.reserve_provider_call().unwrap();
    assert_eq!(
        budget.reserve_provider_call(),
        Err(BudgetRefusal::ProviderCallNotAuthorized),
    );

    budget
        .reserve_recovery(RecoveryReason::MalformedFraming)
        .unwrap();
    budget.reserve_provider_call().unwrap();
    assert_eq!(
        budget.reserve_provider_call(),
        Err(BudgetRefusal::ProviderCallNotAuthorized),
    );
}

#[test]
fn recovery_cannot_be_reserved_before_the_base_call() {
    let mut budget = ResearchBudget::default();
    budget.begin_logical_turn().unwrap();

    assert_eq!(
        budget.reserve_recovery(RecoveryReason::ContextOverflow),
        Err(BudgetRefusal::RecoveryBeforeBaseCall),
    );
    assert_eq!(budget.snapshot().recovery_calls, 0);
}

#[test]
fn snapshot_serializes_exact_camel_case_counters() {
    let mut budget = ResearchBudget::default();
    budget.begin_logical_turn().unwrap();
    budget.reserve_provider_call().unwrap();
    budget
        .reserve_recovery(RecoveryReason::ContextOverflow)
        .unwrap();

    assert_eq!(
        serde_json::to_value(budget.snapshot()).unwrap(),
        serde_json::json!({
            "logicalTurns": 1,
            "recoveryCalls": 1,
            "providerCalls": 1,
        }),
    );
}
