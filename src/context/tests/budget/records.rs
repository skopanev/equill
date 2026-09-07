use super::super::super::assemble_with_limits;
use super::super::super::model::{ExclusionReason, RuntimeBudget};
use super::super::fixtures::records::append;
use super::super::fixtures::registries::registry_unbounded;
use super::super::fixtures::support::{request, store};
use crate::filter::Filter;
use std::fs;

#[test]
fn record_budget_keeps_ranked_prefix_and_reports_the_tail() {
    let root = store("record-cap");
    registry_unbounded(&root, &["tag"], "agent.memory");
    let first = append(
        &root,
        "First core rule",
        &["core"],
        None,
        "2026-01-01T00:00:00Z",
    );
    let second = append(
        &root,
        "Second core rule",
        &["core"],
        None,
        "2026-01-01T00:00:00Z",
    );
    let third = append(
        &root,
        "Third core rule",
        &["core"],
        None,
        "2026-01-01T00:00:00Z",
    );
    let bundle = assemble_with_limits(
        &root,
        "worker.v1",
        request(""),
        "test-owner",
        &Filter::default(),
        RuntimeBudget {
            tokens: None,
            records: Some(2),
        },
    )
    .expect("record budget");

    assert_eq!(bundle.selected_record_ids, vec![first, second]);
    assert_eq!(bundle.receipt.runtime_budget_records, Some(2));
    assert!(
        bundle
            .receipt
            .excluded
            .iter()
            .any(|item| item.id == third && item.reason == ExclusionReason::RecordBudget)
    );
    assert!(bundle.receipt.degraded);
    fs::remove_dir_all(root).expect("remove store");
}

#[test]
fn required_record_ceiling_fails_without_a_partial_bundle() {
    let root = store("required-record-cap");
    registry_unbounded(&root, &["tag"], "agent.memory");
    for rule in ["Required one", "Required two"] {
        append(&root, rule, &["must"], None, "2026-01-01T00:00:00Z");
    }
    let error = assemble_with_limits(
        &root,
        "worker.v1",
        request(""),
        "test-owner",
        &Filter::default(),
        RuntimeBudget {
            tokens: None,
            records: Some(1),
        },
    )
    .expect_err("required records cannot be truncated");

    assert!(error.to_string().contains("CONTEXT_REQUIRED_OVERFLOW"));
    assert!(error.to_string().contains("needs 2 records"));
    assert!(error.to_string().contains("record limit is 1"));
    assert!(!root.join("receipts/context").exists());
    fs::remove_dir_all(root).expect("remove store");
}
