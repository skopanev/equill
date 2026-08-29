use super::super::assemble;
use super::super::model::ExclusionReason;
use super::records::append;
use super::registries::{registry, registry_unbounded};
use super::support::{request, store};
use crate::command::doctor;
use crate::filter::Filter;
use std::fs;

#[test]
fn required_overflow_fails_context_and_doctor() {
    let root = store("overflow");
    registry(&root, 60, 10, &["exact", "tag"], "agent.memory");
    append(
        &root,
        "Mandatory policy too large for its cap",
        &["must"],
        None,
        "2026-01-01T00:00:00Z",
    );
    let error = assemble(
        &root,
        "worker.v1",
        request(""),
        "test-owner",
        &Filter::default(),
    )
    .expect_err("required overflow must fail context");
    let report = doctor::report(Some(&root), true, false).expect("doctor");

    assert!(
        error
            .to_string()
            .contains("required context exceeds the 10 unit limit")
    );
    assert!(!report.ok);
    assert_eq!(report.context_profile_faults, 1);
    fs::remove_dir_all(root).expect("remove store");
}

#[test]
fn context_budget_counts_and_emits_payload_only() {
    let root = store("payload-only");
    registry(&root, 500, 400, &["tag"], "agent.memory");
    append(
        &root,
        "Only payload enters context",
        &["must", "service-tag"],
        None,
        "2026-01-01T00:00:00Z",
    );
    let bundle = assemble(
        &root,
        "worker.v1",
        request(""),
        "test-owner",
        &Filter::default(),
    )
    .expect("context");

    assert_eq!(bundle.content, r#"{"rule":"Only payload enters context"}"#);
    assert_eq!(bundle.receipt.used, bundle.content.chars().count());
    fs::remove_dir_all(root).expect("remove store");
}

#[test]
fn relevant_floor_preserves_request_evidence_before_core() {
    let root = store("relevant-floor");
    registry(&root, 500, 100, &["exact", "tag"], "agent.memory");
    let core = append(
        &root,
        &"C".repeat(450),
        &["core"],
        None,
        "2026-01-01T00:00:00Z",
    );
    let relevant = append(&root, "Needle evidence", &[], None, "2026-01-01T00:00:00Z");
    let bundle = assemble(
        &root,
        "worker.v1",
        request("needle"),
        "test-owner",
        &Filter::default(),
    )
    .expect("context");

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

#[test]
fn absent_budget_returns_everything_and_never_overflows() {
    let root = store("unbounded");
    registry_unbounded(&root, &["exact", "tag"], "agent.memory");
    for index in 0..12 {
        append(
            &root,
            &format!("Mandatory policy {index}"),
            &["must"],
            None,
            "2026-01-01T00:00:00Z",
        );
    }
    let bundle = assemble(
        &root,
        "worker.v1",
        request(""),
        "test-owner",
        &Filter::default(),
    )
    .expect("a profile without caps must never fail on volume");
    let report = doctor::report(Some(&root), true, false).expect("doctor");

    assert_eq!(bundle.receipt.included.len(), 12);
    assert!(
        !bundle
            .receipt
            .excluded
            .iter()
            .any(|item| matches!(item.reason, ExclusionReason::RequiredOverflow))
    );
    assert!(!bundle.receipt.degraded);
    assert_eq!(report.context_profile_faults, 0);
    fs::remove_dir_all(root).expect("remove store");
}
