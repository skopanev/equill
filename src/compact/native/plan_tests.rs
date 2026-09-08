//! What the plan says goes, what stays, and what stays changed.
use super::{Removal, build};
use crate::record::StoredRecord;
use serde_json::json;
use uuid::Uuid;

fn record(id: Uuid, supersedes: Option<Uuid>, tags: Vec<String>) -> StoredRecord {
    StoredRecord {
        id,
        namespace: "agent.memory".into(),
        type_name: "agent.lesson.v1".into(),
        actor: "owner".into(),
        recorded_at: "2026-01-01T00:00:00Z".into(),
        observed_at: "2026-01-01T00:00:00Z".into(),
        valid_at: "2026-01-01T00:00:00Z".into(),
        payload: json!({ "rule": "a rule" }),
        evidence: Vec::new(),
        tags,
        supersedes,
    }
}

/// A chain keeps only its living end, and the link that reached into the
/// removed past is reported as cut — not quietly left dangling, and not
/// described as an untouched record.
#[test]
fn a_chain_collapses_to_its_living_end_and_the_link_is_cut() {
    let (first, second, third) = (Uuid::now_v7(), Uuid::now_v7(), Uuid::now_v7());
    let plan = build(&[
        record(first, None, Vec::new()),
        record(second, Some(first), Vec::new()),
        record(third, Some(second), Vec::new()),
    ])
    .expect("plan");

    assert_eq!(plan.retained, 1);
    assert_eq!(plan.removed.len(), 2);
    assert!(
        plan.removed
            .iter()
            .all(|item| item.reason == Removal::Superseded)
    );
    assert_eq!(plan.severed.len(), 1, "the surviving link was not reported");
    assert_eq!(plan.severed[0].id, third);
    assert_eq!(plan.severed[0].was, second);
}

/// A withdrawal takes both the claim and the tombstone: keeping either would
/// leave the retraction half-done.
#[test]
fn a_revocation_removes_the_claim_and_its_tombstone() {
    let (claim, tombstone) = (Uuid::now_v7(), Uuid::now_v7());
    let plan = build(&[
        record(claim, None, Vec::new()),
        record(tombstone, Some(claim), vec!["equill:revoked".into()]),
    ])
    .expect("plan");

    assert_eq!(plan.retained, 0, "a withdrawn claim survived");
    assert_eq!(plan.removed.len(), 2);
    assert!(
        plan.removed
            .iter()
            .any(|item| item.reason == Removal::Revoked)
    );
    assert!(plan.severed.is_empty(), "nothing survives to be severed");
}

/// A store with nothing dead in it plans no work, which is what makes a second
/// run after an apply a no-op.
#[test]
fn a_store_of_living_records_plans_nothing() {
    let plan = build(&[
        record(Uuid::now_v7(), None, Vec::new()),
        record(Uuid::now_v7(), None, Vec::new()),
    ])
    .expect("plan");

    assert_eq!(plan.retained, 2);
    assert!(plan.removed.is_empty() && plan.severed.is_empty());
}

/// Every surviving link is cut, and that is not an accident of this fixture.
///
/// A record named by `supersedes` is superseded by definition, so it is always
/// among the removed — there is no such thing as a live record whose ancestor
/// survives. After compaction no retained record carries a `supersedes` at all,
/// and the count of cuts is exactly the count of retained records that had one.
#[test]
fn every_retained_link_is_cut_because_no_ancestor_can_survive() {
    let (first, second, lone) = (Uuid::now_v7(), Uuid::now_v7(), Uuid::now_v7());
    let records = [
        record(first, None, Vec::new()),
        record(second, Some(first), Vec::new()),
        record(lone, None, Vec::new()),
    ];
    let plan = build(&records).expect("plan");

    let removed: std::collections::HashSet<_> = plan.removed.iter().map(|item| item.id).collect();
    let retained_with_edge = records
        .iter()
        .filter(|item| !removed.contains(&item.id))
        .filter(|item| item.supersedes.is_some())
        .count();

    assert_eq!(
        plan.severed.len(),
        retained_with_edge,
        "the plan and the ledger disagree on how many links survive to be cut"
    );
    assert!(
        !plan.severed.iter().any(|item| item.id == lone),
        "a record that never had a link was reported as cut"
    );
}
