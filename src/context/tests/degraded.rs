use super::super::assemble;
use super::super::model::{ExclusionReason, Strategy};
use super::records::append;
use super::registries::registry;
use super::support::{request, store};
use crate::{projection, record};
use std::fs;

#[test]
fn degraded_fts_is_visible_and_no_match_is_explicit() {
    let root = store("degraded-fts");
    registry(&root, 4_000, 1_000, &["fts"], "agent.memory");
    append(
        &root,
        "Searchable needle",
        &[],
        None,
        "2026-01-01T00:00:00Z",
    );
    let record = record::read_all(&root).expect("records").remove(0);
    projection::mark_degraded(&root, &record, "synthetic projection fault");
    let bundle =
        assemble(&root, "worker.v1", request("needle"), "test-owner").expect("degraded context");

    assert!(bundle.receipt.degraded);
    assert!(bundle.receipt.empty);
    assert_eq!(bundle.receipt.degraded_strategies, vec![Strategy::Fts]);
    assert!(bundle.content.is_empty());
    fs::remove_dir_all(root).expect("remove store");
}

#[test]
fn unauthorized_records_are_named_in_the_receipt() {
    let root = store("unauthorized");
    registry(&root, 4_000, 1_000, &["exact"], "agent.other");
    let id = append(
        &root,
        "Private synthetic rule",
        &[],
        None,
        "2026-01-01T00:00:00Z",
    );
    let bundle = assemble(&root, "worker.v1", request("private"), "test-owner").expect("context");

    assert!(
        bundle
            .receipt
            .excluded
            .iter()
            .any(|item| { item.id == id && item.reason == ExclusionReason::Unauthorized })
    );
    fs::remove_dir_all(root).expect("remove store");
}
