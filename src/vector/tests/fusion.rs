//! What the merge promises: the same two lists always fuse to the same answer,
//! agreement outranks a lone first place, and neither list can be silently
//! dropped.
use crate::projection::SearchHit;
use crate::record::StoredRecord;
use crate::vector::fuse;
use serde_json::json;
use uuid::Uuid;

/// Ids are fixed rather than generated: the tie-break is by id, so a test that
/// minted fresh uuids would be asserting against a value it did not choose.
fn hit(id: u128) -> SearchHit {
    SearchHit {
        record: StoredRecord {
            id: Uuid::from_u128(id),
            namespace: "agent.memory".into(),
            type_name: "agent.lesson.v1".into(),
            actor: "owner".into(),
            recorded_at: "2026-01-01T00:00:00Z".into(),
            observed_at: "2026-01-01T00:00:00Z".into(),
            valid_at: "2026-01-01T00:00:00Z".into(),
            payload: json!({"rule": "synthetic"}),
            evidence: Vec::new(),
            tags: Vec::new(),
            supersedes: None,
        },
    }
}

fn ids(hits: &[SearchHit]) -> Vec<u128> {
    hits.iter().map(|hit| hit.record.id.as_u128()).collect()
}

#[test]
fn a_record_both_searches_found_outranks_one_that_only_led_a_single_list() {
    // 2 is second in both; 1 and 3 each lead one list and are absent from the
    // other. Agreement is the whole reason to fuse, so it has to win here — if
    // it did not, the merge would be an expensive way to return one list.
    let fused = fuse(vec![hit(1), hit(2)], vec![hit(3), hit(2)]);

    assert_eq!(ids(&fused)[0], 2, "agreement lost to a single first place");
    assert_eq!(fused.len(), 3, "the merge dropped or duplicated a record");
}

#[test]
fn the_same_two_lists_always_fuse_to_the_same_order() {
    // Ties are not an edge case here: rank 2 in one list scores exactly rank 2
    // in the other, so equal scores are routine and something other than the
    // map's iteration order has to decide them.
    let first = fuse(vec![hit(10), hit(20)], vec![hit(30), hit(40)]);
    let again = fuse(vec![hit(10), hit(20)], vec![hit(30), hit(40)]);

    assert_eq!(ids(&first), ids(&again), "two identical merges disagreed");
    assert_eq!(
        ids(&first),
        vec![10, 30, 20, 40],
        "ties were not settled by id, so the order is whatever the map yielded"
    );
}

#[test]
fn a_record_named_by_both_appears_once() {
    let fused = fuse(vec![hit(7)], vec![hit(7)]);

    assert_eq!(ids(&fused), vec![7], "the same record was returned twice");
}

#[test]
fn either_list_may_be_empty_without_losing_the_other() {
    // The no-vector and no-text cases reach the merge as an empty list rather
    // than as a special case, so this is the guard that keeps them from
    // emptying the answer.
    assert_eq!(ids(&fuse(vec![hit(1)], Vec::new())), vec![1]);
    assert_eq!(ids(&fuse(Vec::new(), vec![hit(2)])), vec![2]);
    assert!(fuse(Vec::new(), Vec::new()).is_empty());
}
