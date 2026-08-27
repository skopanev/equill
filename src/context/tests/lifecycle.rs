use super::super::assemble;
use super::super::model::ExclusionReason;
use super::support::{append, append_scoped, registry, request, store};
use serde_json::json;
use std::fs;

#[test]
fn unchanged_request_is_byte_stable_and_filters_lifecycle() {
    let root = store("stable");
    registry(&root, 4_000, 1_000, &["exact", "tag"], "agent.memory");
    let old = append(
        &root,
        "Old mandatory rule",
        &["must"],
        None,
        "2026-01-01T00:00:00Z",
    );
    let live = append(
        &root,
        "New mandatory rule",
        &["must"],
        Some(old),
        "2026-01-02T00:00:00Z",
    );
    let relevant = append(
        &root,
        "Use focused verification",
        &[],
        None,
        "2026-01-03T00:00:00Z",
    );
    let future = append(&root, "Future rule", &[], None, "2027-01-01T00:00:00Z");
    let revoked = append(
        &root,
        "Revoked rule",
        &["equill:revoked"],
        None,
        "2026-01-01T00:00:00Z",
    );
    let expired = append(
        &root,
        "Expired rule",
        &["expires:2026-01-04T00:00:00Z"],
        None,
        "2026-01-01T00:00:00Z",
    );
    let mismatch = append(
        &root,
        "Unrelated active rule",
        &[],
        None,
        "2026-01-01T00:00:00Z",
    );

    let first =
        assemble(&root, "worker.v1", request("verification"), "test-owner").expect("context");
    let second =
        assemble(&root, "worker.v1", request("verification"), "test-owner").expect("repeat");

    assert_eq!(first.content, second.content);
    assert_eq!(first.bundle_digest, second.bundle_digest);
    assert_eq!(first.selected_record_ids, vec![live, relevant]);
    assert_eq!(first.receipt_path, second.receipt_path);
    for (id, reason) in [
        (old, ExclusionReason::Superseded),
        (future, ExclusionReason::InvalidAtRequestTime),
        (revoked, ExclusionReason::Revoked),
        (expired, ExclusionReason::Expired),
        (mismatch, ExclusionReason::SelectorMismatch),
    ] {
        assert!(
            first
                .receipt
                .excluded
                .iter()
                .any(|item| item.id == id && item.reason == reason)
        );
    }
    fs::remove_dir_all(root).expect("remove store");
}

#[test]
fn registered_coordinate_pointer_filters_without_domain_semantics() {
    let root = store("coordinate");
    registry(&root, 4_000, 1_000, &["exact"], "agent.memory");
    let selected = append_scoped(
        &root,
        "Scoped verification",
        &[],
        None,
        "2026-01-01T00:00:00Z",
        Some("scope-a"),
    );
    append_scoped(
        &root,
        "Scoped verification",
        &[],
        None,
        "2026-01-01T00:00:00Z",
        Some("scope-b"),
    );
    let mut input = request("verification");
    input.coordinates.insert("scope".into(), json!("scope-a"));
    let bundle = assemble(&root, "worker.v1", input, "test-owner").expect("context");

    assert_eq!(bundle.selected_record_ids, vec![selected]);
    fs::remove_dir_all(root).expect("remove store");
}
