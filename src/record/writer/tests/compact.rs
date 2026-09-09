use super::request;
use crate::record::{append_request, read_all, tests::store};
use std::fs;

#[test]
fn physical_compaction_expires_removed_keys_and_preserves_retained_replay() {
    let root = store();
    let old = append_request(&root, request(Some("old operation"), "obsolete"), "writer").unwrap();
    let mut next = request(Some("kept operation"), "current");
    next.draft.supersedes = Some(old.id);
    let kept = append_request(&root, next, "writer").unwrap();
    let preview = crate::compact::native::run(&root, false, "writer").unwrap();
    assert_eq!(preview.expired_idempotency_keys, 1);
    let compact = crate::compact::native::run(&root, true, "writer").unwrap();
    assert_eq!(compact.expired_idempotency_keys, 1);
    let directory = root.join("receipts/operations/committed");
    assert_eq!(fs::read_dir(&directory).unwrap().count(), 1);
    for entry in fs::read_dir(directory).unwrap() {
        let bytes = fs::read_to_string(entry.unwrap().path()).unwrap();
        assert!(!bytes.contains(&old.id.to_string()));
        assert!(!bytes.contains(&old.sha256));
    }
    let mut retry = request(Some("kept operation"), "current");
    retry.draft.supersedes = Some(old.id);
    let replay = append_request(&root, retry, "writer").unwrap();
    assert_eq!(replay.id, kept.id);
    assert_eq!(read_all(&root).unwrap().len(), 1);
    // Expired keys carry no dead content: a later client-supplied request is
    // validated as a new operation, with a new coordinate.
    let new = append_request(
        &root,
        request(Some("old operation"), "new deliberate request"),
        "writer",
    )
    .unwrap();
    assert_ne!(new.id, old.id);
    assert_eq!(read_all(&root).unwrap().len(), 2);
    fs::remove_dir_all(root).unwrap();
}
