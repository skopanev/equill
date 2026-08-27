use super::super::assemble;
use super::support::{append_ranked, registry_with_rank, request, store};
use std::fs;

#[test]
fn numeric_rank_precedes_strategy_score_and_observed_at() {
    let root = store("rank");
    registry_with_rank(&root, 4_000, 1_000, &["exact", "recency"], "/confidence");
    let higher = append_ranked(&root, "General policy", 0.95, "2026-01-01T00:00:00Z");
    let lower = append_ranked(&root, "Needle-specific policy", 0.7, "2026-01-03T00:00:00Z");

    let bundle = assemble(&root, "worker.v1", request("needle"), "test-owner").expect("context");

    assert_eq!(bundle.selected_record_ids, vec![higher, lower]);
    fs::remove_dir_all(root).expect("remove store");
}
