//! Killing a compaction at every point where the store is between states.
//!
//! Not only between phases: the dangerous windows are inside each rename pair
//! and between a rename and the journal entry that describes it. A recovery
//! tested only at phase boundaries would miss exactly the states that leave a
//! store unreadable.
use super::journal::Journal;
use super::journal::with_interrupt;
use super::plan_tests::{add, store};
use super::run::run;
use crate::record::read_all;

/// Every point the process can die at, in order.
const POINTS: [&str; 6] = [
    "before-records",
    "inside-records",
    "after-records",
    "before-receipts/writes",
    "inside-receipts/writes",
    "after-receipts/writes",
];

/// After an interruption anywhere, the next run leaves a store that reads and
/// has finished the compaction.
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

/// A receipt from the interrupted half is reconciled by the run that finishes
/// the work, not left describing bytes that no longer exist.
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

/// The child half of the crash test: compacts and dies inside the rename.
///
/// It is a test rather than a binary because the failpoints exist only in a
/// test build — a release binary must not carry a switch that ends a
/// compaction halfway. Ordinary runs of the suite skip it: without the
/// variable it does nothing.
#[test]
fn compaction_child_aborts_inside_the_rename() {
    let Ok(root) = std::env::var("EQUILL_TEST_COMPACT_CHILD") else {
        return;
    };
    let _ = run(std::path::Path::new(&root), true, "owner");
}

/// A real process killed inside the rename, not an error returned inside one.
/// An error unwinds and may put things back; a crash does neither, and the
/// state it leaves — no ledger directory at all — is what recovery must
/// survive. Asserting it is really gone separates this from error injection.
#[test]
fn a_process_killed_inside_the_rename_is_recovered_by_the_next_run() {
    let root = store("killed");
    let first = add(&root, "older", None);
    let survivor = add(&root, "newer", Some(first));

    let child = std::process::Command::new(std::env::current_exe().expect("test binary"))
        .args([
            "compact::native::crash_tests::compaction_child_aborts_inside_the_rename",
            "--exact",
            "--nocapture",
        ])
        .env("EQUILL_TEST_COMPACT_CHILD", &root)
        .env("EQUILL_TEST_COMPACT_HALT", "kill-inside-records")
        .output()
        .expect("child process");

    assert!(!child.status.success(), "the child was not killed");
    assert!(
        !root.join("records").is_dir(),
        "the child did not reach the gap inside the rename"
    );

    run(&root, true, "owner").expect("recovery after a real kill");

    assert!(root.join("records").is_dir(), "the ledger was not restored");
    let after = read_all(&root).expect("ledger");
    assert!(
        after.iter().any(|record| record.id == survivor),
        "the surviving record was lost"
    );
    let written = add(&root, "written after recovery", None);
    assert!(
        read_all(&root)
            .expect("ledger")
            .iter()
            .any(|record| record.id == written),
        "the store stopped accepting writes"
    );
    let _ = std::fs::remove_dir_all(&root);
}
