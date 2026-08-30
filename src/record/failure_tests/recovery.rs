//! Failures after the point of no return.
//!
//! The record is durable and, in one case, its receipt is not. These are the
//! only moments where something true can exist with nothing saying so, and the
//! rule is the same in both: the write is never un-done and never re-reported
//! as failed, because a caller that retries a completed write stores it twice.
use super::super::tests::{lesson, store};
use super::super::{append, append_only};
use super::{ledger_bytes, ledger_file, month_directory, receipts, seal, unseal};
use std::fs;

/// The other side of the same window, and the one that matters more.
///
/// The receipt is staged before the append and committed after it. If the
/// commit fails, the record is already durable — and this is the only moment in
/// the system where something true has no document saying so. What must happen
/// then is not that the write is undone (it cannot be; the ledger is immutable)
/// but that the transaction is finishable and is in fact finished before the
/// store does anything else.
#[test]
fn a_receipt_that_cannot_commit_is_finished_before_the_next_write() {
    let root = store();
    append(&root, lesson("the record before the injection"), "writer").expect("seed");
    let month = month_directory(&root);
    let committed = receipts(&root);
    let ledger_before = ledger_bytes(&root);

    // Staging still works; the rename into the month does not.
    seal(&month);
    let error = append_only(&root, lesson("the durable record"), "writer")
        .expect_err("the receipt could not be committed");

    // Named as what it is: after the commit point, not before it.
    assert!(
        matches!(error, crate::kernel::error::Error::PostCommit(_)),
        "a post-durable failure was reported as something else: {error:?}"
    );
    let told = error.to_string();
    let durable = fs::read_to_string(root.join(ledger_file(&root)))
        .or_else(|_| fs::read_to_string(ledger_file(&root)))
        .expect("ledger");
    let id = durable
        .lines()
        .next_back()
        .and_then(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .and_then(|value| value["id"].as_str().map(str::to_owned))
        .expect("the last record has an id");
    assert!(
        told.contains(&id),
        "the failure did not name the record that is durable: {told}"
    );
    let handle = format!("receipts/pending/{id}.json");
    assert!(
        told.contains(&handle),
        "the failure did not name where the unfinished receipt is: {told}"
    );
    assert!(
        root.join(&handle).is_file(),
        "the unfinished receipt was deleted, leaving a durable record with nothing to finish"
    );
    assert!(
        ledger_bytes(&root) > ledger_before,
        "the record the failure describes is not in the ledger"
    );

    // While it cannot be finished, the store does not take another write.
    let refused = append_only(&root, lesson("the write that must wait"), "writer")
        .expect_err("a write on top of an unresolved transaction");
    assert!(
        matches!(refused, crate::kernel::error::Error::Io(_)),
        "the blocked write failed for the wrong reason: {refused:?}"
    );

    // Once it can be, the next write finishes it first — same coordinate, one
    // record, and only then the new one.
    unseal(&month);
    let records_before = fs::read_to_string(ledger_file(&root))
        .expect("ledger")
        .lines()
        .count();
    append_only(&root, lesson("the write that proceeds"), "writer").expect("the next write");

    assert!(
        root.join(format!(
            "receipts/writes/{}/{id}.json",
            month.file_name().expect("month").to_string_lossy()
        ))
        .is_file(),
        "the original receipt was not finished at its own coordinate"
    );
    assert!(
        !root.join(&handle).exists(),
        "the unfinished receipt is still pending after recovery"
    );
    assert_eq!(
        receipts(&root),
        committed + 2,
        "recovery did not leave exactly the original receipt plus the new one"
    );
    assert_eq!(
        fs::read_to_string(ledger_file(&root))
            .expect("ledger")
            .lines()
            .filter(|line| line.contains(&id))
            .count(),
        1,
        "the recovered transaction left more than one record at its coordinate"
    );
    assert_eq!(
        fs::read_to_string(ledger_file(&root))
            .expect("ledger")
            .lines()
            .count(),
        records_before + 1,
        "recovery appended something of its own"
    );
    let _ = fs::remove_dir_all(&root);
}

/// What is written after the receipt is not part of the promise.
///
/// The lifecycle state and the published target both follow the commit. Both
/// are rebuildable: the state is refused unless its marker still describes the
/// ledger, and a missing target reads as unknown freshness rather than as
/// current. So a failure to write either of them must not fail the write — the
/// record is durable and its receipt is committed, and reporting that as an
/// error would invite a retry that stores the record a second time.
#[test]
fn a_marker_that_cannot_be_written_does_not_fail_a_completed_write() {
    let root = store();
    append(&root, lesson("the record before the injection"), "writer").expect("seed");
    let projections = root.join("projections");
    let lifecycle = root.join("projections/lifecycle");

    seal(&lifecycle);
    seal(&projections);
    let report = append_only(&root, lesson("the record under test"), "writer")
        .expect("a completed write reported as failed");
    unseal(&projections);
    unseal(&lifecycle);

    assert!(
        root.join(&report.receipt).is_file(),
        "the write reported success without a committed receipt"
    );
    // And the state that could not be written is treated as absent rather than
    // as still describing the store.
    assert!(
        super::super::lifecycle::load_state(&root)
            .expect("load")
            .is_none(),
        "a state that missed a record was accepted as current"
    );
    // The next write rebuilds and carries on.
    append_only(&root, lesson("the write after"), "writer").expect("the next write");
    assert!(
        super::super::lifecycle::load_state(&root)
            .expect("load")
            .is_some(),
        "the state was not rebuilt"
    );
    let _ = fs::remove_dir_all(&root);
}
