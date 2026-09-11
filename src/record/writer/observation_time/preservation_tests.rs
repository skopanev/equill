use super::historical_fixture::seed;
use crate::record::{
    AppendRequest, append, append_only_request, read_all,
    tests::{lesson, store},
};
use jiff::Timestamp;
use std::fs;

fn request(rule: &str) -> AppendRequest {
    let mut draft = lesson(rule);
    draft.observed_at = Timestamp::MAX.to_string();
    AppendRequest {
        draft,
        idempotency_key: Some("pre-fix operation".into()),
    }
}

#[test]
fn pre_fix_future_operation_recovers_and_replays_without_revalidating_time() {
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
    let original = seed(
        &root,
        request("pre-fix").draft,
        "worker",
        Some("pre-fix operation"),
    );
    let before = fs::read(root.join(&original.ledger)).unwrap();
    super::super::recover(&root).unwrap();
    for _ in 0..2 {
        let replay = append_only_request(&root, request("pre-fix"), "worker").unwrap();
        assert_eq!(replay.id, original.id);
        assert_eq!(replay.sha256, original.sha256);
        assert_eq!(replay.receipt, original.receipt);
    }
    assert_eq!(fs::read(root.join(&original.ledger)).unwrap(), before);
    let conflict = append_only_request(&root, request("different"), "worker")
        .unwrap_err()
        .to_string();
    assert!(conflict.contains("idempotency conflict"));
    crate::governance::revoke_grant(&root, "worker", None, "writer").unwrap();
    assert!(append_only_request(&root, request("pre-fix"), "worker").is_err());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn pre_fix_future_record_survives_doctor_rebuild_and_native_compact() {
    let root = store();
    let original = seed(&root, request("pre-fix").draft, "writer", None);
    let before = fs::read(root.join(&original.ledger)).unwrap();
    crate::projection::rebuild(&root).unwrap();
    assert!(
        crate::command::doctor::report(Some(&root), true, false)
            .unwrap()
            .ok
    );
    let first = append(&root, lesson("old"), "writer").unwrap();
    let mut replacement = lesson("replacement");
    replacement.supersedes = Some(first.id);
    append(&root, replacement, "writer").unwrap();
    let compacted = crate::compact::native::run(&root, true, "writer").unwrap();
    assert_eq!(compacted.removed, 1);
    let record = read_all(&root)
        .unwrap()
        .into_iter()
        .find(|r| r.id == original.id)
        .unwrap();
    assert_eq!(
        format!("{}\n", serde_json::to_string(&record).unwrap()).as_bytes(),
        before
    );
    crate::projection::rebuild(&root).unwrap();
    assert!(
        crate::command::doctor::report(Some(&root), true, false)
            .unwrap()
            .ok
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn superseding_future_record_is_refused_but_revocation_of_old_bad_record_works() {
    let root = store();
    let original = seed(&root, request("pre-fix").draft, "writer", None);
    let mut replacement = request("replacement").draft;
    replacement.supersedes = Some(original.id);
    assert!(append(&root, replacement, "writer").is_err());
    assert_eq!(read_all(&root).unwrap().len(), 1);
    crate::record::revoke(&root, original.id, Some("synthetic correction"), "writer").unwrap();
    assert_eq!(read_all(&root).unwrap().len(), 2);
    fs::remove_dir_all(root).unwrap();
}
