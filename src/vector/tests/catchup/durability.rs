use super::add;
use super::harness::{configured, counting_starter};
use crate::vector::catchup::starter::with_starter;
use crate::vector::{after_commit, run_once};

/// The target is the only durable statement of what the index still owes the
/// ledger. It has to survive a crash, which means the bytes must be on the
/// device and the rename must be too — not merely in the page cache.
#[test]
fn a_published_target_is_written_atomically_and_leaves_no_residue() {
    let root = configured("durable-target");
    let directory = root.join("projections/qdrant");

    with_starter(counting_starter, || after_commit(&root, 0));

    let target = super::super::super::desired::read(&root)
        .expect("read")
        .expect("published");
    assert_eq!(target.revision, 1);
    let residue = std::fs::read_dir(&directory)
        .expect("directory")
        .flatten()
        .filter(|entry| entry.file_name().to_string_lossy().starts_with(".desired-"))
        .count();
    assert_eq!(residue, 0, "staging must leave nothing behind");

    // Republishing replaces the file byte-for-byte rather than appending or
    // truncating in place.
    add(&root, "a second lesson");
    with_starter(counting_starter, || after_commit(&root, 0));
    let moved = super::super::super::desired::read(&root)
        .expect("read")
        .expect("published");
    assert_eq!(moved.revision, 2);
    assert!(moved.revision > target.revision);
}

/// A detached worker writes nowhere the caller can see, so its outcome is kept
/// on disk — with counts and an error class, never a provider message that
/// might quote a payload.
#[test]
fn a_worker_records_a_sanitized_outcome() {
    let root = configured("last-drain");
    with_starter(counting_starter, || after_commit(&root, 0));

    run_once(&root);

    let state: serde_json::Value = serde_json::from_slice(
        &std::fs::read(root.join("projections/qdrant/last-drain.json")).expect("outcome file"),
    )
    .expect("json");
    assert_eq!(state["schema"], "equill.qdrant-last-drain.v1");
    assert_eq!(state["outcome"], "failed");
    assert!(
        state["error_class"].is_string(),
        "the class is recorded: {state}"
    );
    let text = state.to_string();
    for forbidden in ["lesson", "rule", "127.0.0.1"] {
        assert!(
            !text.contains(forbidden),
            "the outcome file must not quote {forbidden}: {text}"
        );
    }
}
