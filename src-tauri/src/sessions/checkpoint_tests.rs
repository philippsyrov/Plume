//! Provenance re-resolution tests.
//!
//! These guard the rule that stops compaction from laundering a forgotten fact
//! back into the model's context. The scenario worth reading first is
//! `forgetting_a_memory_removes_its_fact_from_every_later_generation`.

use std::collections::{HashMap, HashSet};

use super::checkpoint::*;

fn fact(text: &str, turns: &[&str], memory: Option<(&str, u32)>) -> CheckpointFact {
    CheckpointFact {
        kind: FactKind::CriticalFact,
        text: text.to_string(),
        provenance: FactProvenance {
            source_turn_ids: turns.iter().map(|t| t.to_string()).collect(),
            memory_entry: memory.map(|(id, revision)| MemoryProvenance {
                entry_id: id.to_string(),
                revision,
            }),
        },
    }
}

fn context<'a>(
    memory: &'a HashMap<String, u32>,
    turns: &'a HashSet<String>,
) -> ProvenanceContext<'a> {
    ProvenanceContext {
        memory_revisions: memory,
        retained_turn_ids: turns,
    }
}

fn live(pairs: &[(&str, u32)]) -> HashMap<String, u32> {
    pairs
        .iter()
        .map(|(id, rev)| ((*id).to_string(), *rev))
        .collect()
}

fn retained(ids: &[&str]) -> HashSet<String> {
    ids.iter().map(|id| (*id).to_string()).collect()
}

#[test]
fn a_fact_whose_provenance_still_resolves_is_kept() {
    let memory = live(&[("m1", 3)]);
    let turns = retained(&["t1"]);
    let facts = vec![fact("prefers tabs", &["t1"], Some(("m1", 3)))];

    let resolved = resolve_facts(&facts, &context(&memory, &turns));

    assert_eq!(resolved.kept.len(), 1);
    assert!(resolved.refused.is_empty());
    assert!(!resolved.is_stale());
}

#[test]
fn forgetting_a_memory_removes_its_fact_from_every_later_generation() {
    // The laundering path this whole module exists for: compact a fact, forget
    // its memory, and the fact must not survive — not in the next projection,
    // and not by being carried into a later checkpoint either.
    let turns = retained(&["t1"]);
    let facts = vec![fact("lives in Lisbon", &["t1"], Some(("m1", 1)))];

    let forgotten = live(&[]);
    let first = resolve_facts(&facts, &context(&forgotten, &turns));
    assert!(
        first.kept.is_empty(),
        "a forgotten memory's fact is refused"
    );
    assert_eq!(first.refused[0].1, FactRefusal::MemoryForgotten);
    assert!(
        first.is_stale(),
        "the checkpoint must be rebuilt from history, not re-summarized, or the \
         loss is carried forward silently",
    );

    // Re-resolving what the first pass kept proves nothing on its own — that is
    // re-summarizing an already-filtered checkpoint, not the rebuild the stale
    // marking actually triggers. The real path is covered below.
    let second = resolve_facts(&first.kept, &context(&forgotten, &turns));
    assert!(second.kept.is_empty());
}

#[test]
fn a_rebuild_from_history_cannot_resurrect_a_forgotten_fact() {
    // The path the previous test misses, and the one that matters. Losing a
    // fact marks the checkpoint stale, and a rebuild reads retained history —
    // where the turn the fact came from is still sitting, because Plume never
    // deletes history. Rebuilt from that turn the fact returns with no memory
    // link left to refuse it by, so forget would last exactly one projection.
    let turns = retained(&["t1", "t2"]);
    let tombstones = vec![ForgottenMemory {
        entry_id: "m1".into(),
        source_turn_ids: vec!["t1".into()],
        forgotten_at_ms: 42,
    }];

    let rebuildable = rebuildable_turn_ids(&turns, &tombstones);

    assert!(
        !rebuildable.contains("t1"),
        "the turn behind a forgotten memory must not be summarized again",
    );
    assert!(
        rebuildable.contains("t2"),
        "unrelated history is untouched — forget is not a history delete",
    );

    // And a fact rebuilt from that turn, even stripped of its memory link, has
    // no surviving source and is refused.
    let rebuilt = vec![fact("lives in Lisbon", &["t1"], None)];
    let resolved = resolve_facts(&rebuilt, &context(&live(&[]), &rebuildable));
    assert_eq!(resolved.refused[0].1, FactRefusal::SourceTurnsGone);
}

#[test]
fn forgetting_does_not_remove_the_turn_from_history() {
    // Worth stating as a test because the fix is a summarization exclusion, not
    // a deletion: the user asked Plume to stop knowing something, not to erase
    // what they said. The turn stays retained and stays visible.
    let turns = retained(&["t1"]);
    let tombstones = vec![ForgottenMemory {
        entry_id: "m1".into(),
        source_turn_ids: vec!["t1".into()],
        forgotten_at_ms: 1,
    }];

    assert!(turns.contains("t1"), "retained history is unchanged");
    assert!(forgotten_turn_ids(&tombstones).contains("t1"));
}

#[test]
fn revising_a_memory_refuses_the_wording_that_restated_the_old_revision() {
    // The entry still exists, so a presence check would wave this through — but
    // the user changed their mind, and the summary quotes what they replaced.
    let memory = live(&[("m1", 2)]);
    let turns = retained(&["t1"]);
    let facts = vec![fact("deploys on Fridays", &["t1"], Some(("m1", 1)))];

    let resolved = resolve_facts(&facts, &context(&memory, &turns));

    assert_eq!(resolved.refused[0].1, FactRefusal::MemoryRevised);
    assert!(resolved.kept.is_empty());
}

#[test]
fn a_forgotten_memory_is_refused_even_when_its_source_turn_survives() {
    // The turn that discussed a preference is why the fact exists; it is not
    // evidence that the user still wants it remembered.
    let turns = retained(&["t1"]);
    let facts = vec![fact("prefers dark mode", &["t1"], Some(("m1", 1)))];

    let resolved = resolve_facts(&facts, &context(&live(&[]), &turns));

    assert_eq!(resolved.refused[0].1, FactRefusal::MemoryForgotten);
}

#[test]
fn a_fact_survives_while_any_one_of_its_source_turns_remains() {
    let turns = retained(&["t9"]);
    let facts = vec![fact("uses pnpm", &["t1", "t9"], None)];

    let resolved = resolve_facts(&facts, &context(&live(&[]), &turns));

    assert_eq!(
        resolved.kept.len(),
        1,
        "one surviving source still vouches for it"
    );
}

#[test]
fn a_fact_whose_every_source_turn_left_history_is_refused() {
    let facts = vec![fact("uses pnpm", &["t1", "t2"], None)];

    let resolved = resolve_facts(&facts, &context(&live(&[]), &retained(&["t9"])));

    assert_eq!(resolved.refused[0].1, FactRefusal::SourceTurnsGone);
}

#[test]
fn a_fact_naming_no_source_is_never_eligible() {
    // Nothing can vouch for it, so it can never be re-checked against anything.
    let facts = vec![fact("the user is a doctor", &[], None)];

    let resolved = resolve_facts(&facts, &context(&live(&[]), &retained(&["t1"])));

    assert_eq!(resolved.refused[0].1, FactRefusal::Unprovenanced);
}

#[test]
fn refusing_one_fact_does_not_discard_the_rest_of_the_checkpoint() {
    let memory = live(&[("m2", 1)]);
    let turns = retained(&["t1"]);
    let facts = vec![
        fact("forgotten", &["t1"], Some(("m1", 1))),
        fact("still good", &["t1"], Some(("m2", 1))),
        fact("orphaned", &[], None),
    ];

    let resolved = resolve_facts(&facts, &context(&memory, &turns));

    assert_eq!(resolved.kept.len(), 1);
    assert_eq!(resolved.kept[0].text, "still good");
    assert_eq!(resolved.refused.len(), 2);
    assert!(resolved.is_stale());
}

#[test]
fn a_memory_only_fact_is_refused_even_while_its_memory_lives() {
    // A live memory entry is not an anchor in history. Without source turns the
    // fact would keep projecting after every turn behind it was compacted away,
    // which is precisely the anchorless state this module exists to prevent.
    let memory = live(&[("m1", 1)]);
    let facts = vec![fact("prefers tabs", &[], Some(("m1", 1)))];

    let resolved = resolve_facts(&facts, &context(&memory, &retained(&["t1"])));

    assert_eq!(resolved.refused[0].1, FactRefusal::Unprovenanced);
    assert!(resolved.kept.is_empty());
}
