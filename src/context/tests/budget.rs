use super::super::assemble;
use super::super::model::ExclusionReason;
use super::support::{append, registry, request, store};
use crate::command::doctor;
use std::fs;

#[test]
fn required_overflow_is_degraded_and_fails_doctor() {
    let root = store("overflow");
    registry(&root, 60, 10, &["exact", "tag"], "agent.memory");
    append(
        &root,
        "Mandatory policy too large for its cap",
        &["must"],
        None,
        "2026-01-01T00:00:00Z",
    );
    let bundle = assemble(&root, "worker.v1", request(""), "test-owner").expect("degraded bundle");
    let report = doctor::report(Some(&root), true, false).expect("doctor");

    assert!(bundle.receipt.degraded);
    assert!(bundle.selected_record_ids.is_empty());
    assert!(!report.ok);
    assert_eq!(report.context_profile_faults, 1);
    fs::remove_dir_all(root).expect("remove store");
}

#[test]
fn relevant_floor_preserves_request_evidence_before_core() {
    let root = store("relevant-floor");
    registry(&root, 500, 100, &["exact", "tag"], "agent.memory");
    let core = append(
        &root,
        &"C".repeat(260),
        &["core"],
        None,
        "2026-01-01T00:00:00Z",
    );
    let relevant = append(&root, "Needle evidence", &[], None, "2026-01-01T00:00:00Z");
    let bundle = assemble(&root, "worker.v1", request("needle"), "test-owner").expect("context");

    assert_eq!(bundle.selected_record_ids, vec![relevant]);
    assert!(
        bundle
            .receipt
            .excluded
            .iter()
            .any(|item| { item.id == core && item.reason == ExclusionReason::CoreCap })
    );
    fs::remove_dir_all(root).expect("remove store");
}
