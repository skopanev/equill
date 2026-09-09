//! What the ledger numbers mean, including where they deliberately overlap.
use super::counts::of;
use crate::record::StoredRecord;
use serde_json::json;
use uuid::Uuid;

pub(super) fn plain(id: Uuid, supersedes: Option<Uuid>, revoked: bool) -> StoredRecord {
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
        tags: if revoked {
            vec![crate::record::REVOKED_TAG.to_string()]
        } else {
            Vec::new()
        },
        supersedes,
    }
}

#[test]
fn an_empty_ledger_counts_zero_of_everything() {
    let counts = of(&[]);

    assert_eq!(counts.ledger_records, 0);
    assert_eq!(counts.ledger_live, 0);
    assert_eq!(counts.ledger_dead, 0);
}

/// A chain leaves one living record however long it is.
#[test]
fn a_chain_leaves_its_living_end() {
    let (first, second, third) = (Uuid::now_v7(), Uuid::now_v7(), Uuid::now_v7());
    let counts = of(&[
        plain(first, None, false),
        plain(second, Some(first), false),
        plain(third, Some(second), false),
    ]);

    assert_eq!((counts.ledger_records, counts.ledger_live), (3, 1));
    assert_eq!(counts.ledger_dead, 2);
    assert_eq!(counts.ledger_superseded, 2);
    assert_eq!(counts.ledger_revoked, 0);
}

/// The two dead numbers can sum to more than the total, and that is not a bug
/// to hide: a tombstone that is itself superseded is both, so the report says
/// how many are both rather than leaving the arithmetic impossible.
#[test]
fn a_record_that_is_both_is_counted_once_and_the_overlap_is_named() {
    let (claim, tombstone, later) = (Uuid::now_v7(), Uuid::now_v7(), Uuid::now_v7());
    let counts = of(&[
        plain(claim, None, false),
        plain(tombstone, Some(claim), true),
        plain(later, Some(tombstone), false),
    ]);

    assert_eq!(counts.ledger_superseded, 2, "claim and tombstone");
    assert_eq!(counts.ledger_revoked, 1, "the tombstone");
    assert_eq!(counts.ledger_dead_overlap, 1, "the tombstone is both");
    assert_eq!(
        counts.ledger_dead, 2,
        "the tombstone was counted twice in the total"
    );
    assert_eq!(counts.ledger_live, 1);
    assert_eq!(
        counts.ledger_superseded + counts.ledger_revoked - counts.ledger_dead_overlap,
        counts.ledger_dead,
        "the numbers a reader is given do not reconcile"
    );
}
