use super::request;
use crate::record::{append_request, read_all, tests::store};
use std::fs;

#[test]
fn a_key_replays_the_original_coordinate_and_receipt() {
    let root = store();
    let first = append_request(&root, request(Some("opaque operation"), "same"), "writer").unwrap();
    let again = append_request(&root, request(Some("opaque operation"), "same"), "writer").unwrap();
    assert_eq!(
        (again.id, again.receipt, again.sha256),
        (first.id, first.receipt, first.sha256)
    );
    assert_eq!(read_all(&root).unwrap().len(), 1);
    let conflict = append_request(
        &root,
        request(Some("opaque operation"), "other private content"),
        "writer",
    )
    .unwrap_err()
    .to_string();
    assert!(conflict.contains("idempotency conflict"));
    assert!(!conflict.contains("opaque operation"));
    assert!(!conflict.contains("other private content"));
    for entry in fs::read_dir(root.join("receipts/operations/committed")).unwrap() {
        let bytes = fs::read_to_string(entry.unwrap().path()).unwrap();
        assert!(!bytes.contains("opaque operation"));
        assert!(!bytes.contains("payload"));
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn no_key_keeps_identical_restatements_independent() {
    let root = store();
    let first = append_request(&root, request(None, "same"), "writer").unwrap();
    let second = append_request(&root, request(None, "same"), "writer").unwrap();
    assert_ne!(first.id, second.id);
    assert_eq!(read_all(&root).unwrap().len(), 2);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn the_same_opaque_key_is_independent_between_actors_and_stores() {
    let first_store = store();
    let other_store = store();
    crate::governance::grant(
        &first_store,
        "worker",
        "agent.memory",
        &["agent.lesson.v1".into()],
        None,
        "writer",
    )
    .unwrap();
    let initial_records = read_all(&first_store).unwrap().len();
    let append = |root: &std::path::Path, actor| {
        append_request(
            root,
            request(Some("shared opaque key"), "synthetic scope"),
            actor,
        )
        .unwrap()
    };
    let first = append(&first_store, "writer");
    let actor = append(&first_store, "worker");
    let other = append(&other_store, "writer");
    assert_ne!(first.id, actor.id);
    assert_ne!(first.id, other.id);
    assert_eq!(append(&first_store, "writer").id, first.id);
    assert_eq!(append(&first_store, "worker").id, actor.id);
    assert_eq!(append(&other_store, "writer").id, other.id);
    assert_eq!(read_all(&first_store).unwrap().len(), initial_records + 2);
    assert_eq!(read_all(&other_store).unwrap().len(), 1);
    fs::remove_dir_all(first_store).unwrap();
    fs::remove_dir_all(other_store).unwrap();
}

#[test]
fn every_interruption_recovers_before_an_unrelated_append() {
    use super::super::seam::{self, Step};
    for step in [
        Step::BeforeAppend,
        Step::AfterAppend,
        Step::BeforeLedgerSync,
        Step::AfterReceipts,
        Step::BeforeOutcome,
    ] {
        let root = store();
        seam::fail(Some(step));
        assert!(append_request(&root, request(Some("retry"), "first"), "writer").is_err());
        seam::fail(None);
        assert!(
            read_all(&root)
                .unwrap_err()
                .to_string()
                .contains("recovery pending")
        );
        let prior = (step != Step::BeforeAppend).then(|| super::pending_id(&root));
        append_request(&root, request(None, "unrelated"), "writer").unwrap();
        let retried = append_request(&root, request(Some("retry"), "first"), "writer").unwrap();
        if let Some(id) = prior {
            assert_eq!(retried.id, id);
        }
        assert_eq!(read_all(&root).unwrap().len(), 2);
        assert!(root.join(retried.receipt).is_file());
        fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn an_outcome_never_bypasses_current_authority() {
    let root = store();
    crate::governance::grant(
        &root,
        "worker",
        "agent.memory",
        &["agent.lesson.v1".into()],
        None,
        "writer",
    )
    .unwrap();
    append_request(&root, request(Some("key"), "same"), "worker").unwrap();
    crate::governance::revoke_grant(&root, "worker", None, "writer").unwrap();
    assert!(append_request(&root, request(Some("key"), "same"), "worker").is_err());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn keyed_recovery_never_truncates_an_unrelated_ledger_tail() {
    use super::super::seam::{self, Step};
    let root = store();
    seam::fail(Some(Step::PartialAppend));
    assert!(append_request(&root, request(Some("retry"), "synthetic first"), "writer").is_err());
    seam::fail(None);
    let ledger = fs::read_dir(root.join("records"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let bytes = b"synthetic unrelated tail must remain untouched";
    fs::write(&ledger, bytes).unwrap();
    assert!(append_request(&root, request(Some("retry"), "synthetic first"), "writer").is_err());
    assert_eq!(fs::read(&ledger).unwrap(), bytes);
    assert!(root.join("transactions/batch.json").is_file());
    fs::remove_dir_all(root).unwrap();
}
