//! Killing a compaction at every point where the store is between states.
//!
//! Not only between phases: the dangerous windows are inside each rename pair
//! and between a rename and the journal entry that describes it. A recovery
//! tested only at phase boundaries would miss exactly the states that leave a
//! store unreadable.
use super::journal::Journal;
use super::journal::with_interrupt;
use super::race_tests::{add, store};
use super::run::run;
use crate::record::read_all;

/// Every point at which the process can die, in the order it reaches them.
const POINTS: [&str; 6] = [
    "before-records",
    "inside-records",
    "after-records",
    "before-receipts/writes",
    "inside-receipts/writes",
    "after-receipts/writes",
];

/// After an interruption anywhere, the next run leaves a store that reads,
/// agrees with its receipts, and has finished the compaction.
#[test]
fn a_compaction_killed_anywhere_is_finished_by_the_next_run() {
    for point in POINTS {
        let root = store(&format!("crash-{}", point.replace('/', "-")));
        let first = add(&root, "older", None);
        let survivor = add(&root, "newer", Some(first));

        let interrupted = with_interrupt(point, || run(&root, true, "owner"));
        assert!(
            interrupted.is_err(),
            "the fixture did not interrupt at {point}"
        );

        // Whatever state the store is in, the next run has to cope with it —
        // including the one where the ledger directory is missing entirely.
        let resumed = run(&root, true, "owner")
            .unwrap_or_else(|error| panic!("recovery failed after {point}: {error}"));
        let _ = resumed;

        let after = read_all(&root)
            .unwrap_or_else(|error| panic!("the store is unreadable after {point}: {error}"));
        assert!(
            after.iter().any(|record| record.id == survivor),
            "the surviving record was lost at {point}"
        );
        assert!(
            !after.iter().any(|record| record.id == first),
            "the removed record came back at {point}"
        );
        assert!(
            after.iter().all(|record| record.supersedes.is_none()),
            "a dangling link survived at {point}"
        );
        assert!(
            Journal::read(&root).expect("journal").is_none(),
            "the journal outlived the finished compaction at {point}"
        );

        // And the store still takes writes.
        let written = add(&root, "written after recovery", None);
        assert!(
            read_all(&root)
                .expect("ledger")
                .iter()
                .any(|record| record.id == written),
            "the store stopped accepting writes after {point}"
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}

/// A receipt left over from the interrupted half is reconciled by the run that
/// finishes the work, not left describing bytes that no longer exist.
#[test]
fn recovery_leaves_the_receipts_agreeing_with_the_ledger() {
    let root = store("crash-receipts");
    let first = add(&root, "older", None);
    let survivor = add(&root, "newer", Some(first));

    let interrupted = with_interrupt("after-records", || run(&root, true, "owner"));
    assert!(interrupted.is_err());
    run(&root, true, "owner").expect("recovery");

    let record = read_all(&root)
        .expect("ledger")
        .into_iter()
        .find(|record| record.id == survivor)
        .expect("survivor");
    let digest = crate::kernel::digest::sha256_hex(&serde_json::to_vec(&record).expect("bytes"));
    let path = root
        .join("receipts/writes")
        .join(&record.recorded_at[..7])
        .join(format!("{survivor}.json"));
    let receipt: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).expect("receipt")).expect("json");

    assert_eq!(
        receipt["record_sha256"].as_str(),
        Some(digest.as_str()),
        "recovery left a receipt attesting to bytes the ledger no longer holds"
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// A record appended between the crash and the recovery is not carried away.
///
/// The store takes writes the moment the process is gone, and that write lands
/// in the directory that is still current. Publishing the prepared copy over it
/// would move the live directory — with the new record in it — aside as a
/// backup, and the record would be gone from an immutable ledger. Recovery
/// refuses instead of guessing.
#[test]
fn an_append_after_the_crash_is_not_lost_to_the_prepared_copy() {
    let root = store("crash-append");
    let first = add(&root, "older", None);
    add(&root, "newer", Some(first));

    let interrupted = with_interrupt("before-records", || run(&root, true, "owner"));
    assert!(interrupted.is_err(), "the fixture did not interrupt");

    // The window: nothing is published yet, and the store is writable again.
    let landed = add(&root, "written between crash and recovery", None);

    let resumed = run(&root, true, "owner");
    assert!(
        resumed.is_err(),
        "recovery published a stale copy over a ledger that had moved on"
    );
    assert!(
        read_all(&root)
            .expect("ledger")
            .iter()
            .any(|record| record.id == landed),
        "a record written after the crash was lost"
    );
    let _ = std::fs::remove_dir_all(&root);
}
