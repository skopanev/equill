//! What has to be on disk before a write says it succeeded.
//!
//! A committed receipt is a name in a directory, and a name is no more durable
//! than the directory holding it. These assert the ORDER of the publications
//! and the difference between a directory that has to be created and one that
//! is already there — an assertion that could not tell those apart would pass
//! whether the work happened always, never, or exactly when it should.
mod receipts;

use super::super::append_only;
use super::super::tests::{lesson, store};
use std::fs;

/// A committed receipt is a name in a directory, and a name is no more durable
/// than the directory holding it.
///
/// So the directories are published before this reports success, and the order
/// is asserted rather than assumed: the destination first, because that is what
/// makes the receipt exist, and the staging directory second, because that is
/// what makes it stop existing where it was. The failure injection is the other
/// half — an unpublished rename must not come back as a completed write.
#[test]
fn a_committed_receipt_publishes_its_directories_before_reporting_success() {
    use crate::kernel::path::{Step, fail, reset, steps};

    let root = store();
    reset();
    append_only(&root, lesson("the first record of its month"), "writer").expect("write");
    assert_eq!(
        steps(),
        [
            Step::PendingCreated,
            Step::Staged,
            Step::WritesCreated,
            Step::MonthCreated,
            Step::Committed,
            Step::Drained
        ],
        "the first write of a store did not publish everything it created"
    );

    // The second one finds the month already there and does not pay for it
    // again. Asserted rather than assumed: a publication that happened every
    // time would be a cost on every write, and one that never happened would be
    // the gap this exists to close — the same assertion cannot tell those apart
    // unless it distinguishes the two cases.
    reset();
    append_only(&root, lesson("the second record of its month"), "writer").expect("write");
    assert_eq!(
        steps(),
        [Step::Staged, Step::Committed, Step::Drained],
        "directories that already existed were published again, or the stage was not"
    );

    reset();
    fail(Step::MonthCreated);
    let unpublished = append_only(
        &root,
        lesson("in a month that cannot be published"),
        "writer",
    );
    // Same month, so nothing is created and nothing is published: the injection
    // has nothing to catch, and the write succeeds. Stated rather than left
    // implicit, because a test that injected into the wrong case would look
    // exactly like one that passed.
    assert!(
        unpublished.is_ok(),
        "an existing month was treated as a fresh one"
    );

    reset();
    fail(Step::Committed);
    let refused = append_only(
        &root,
        lesson("the write that cannot be published"),
        "writer",
    )
    .expect_err("a commit whose directory could not be published");
    assert!(
        matches!(refused, crate::kernel::error::Error::PostCommit(_)),
        "an unpublished rename was reported as a completed write: {refused:?}"
    );
    reset();
    let _ = fs::remove_dir_all(&root);
}

/// Nothing reaches the ledger until the stage that describes it is on disk.
///
/// The stage is written and published before the append, so that a crash
/// between them leaves a stage with no record — which recovery reads as a
/// pre-append crash and sets aside — rather than a record with no stage, which
/// it cannot finish at all. A publication that fails must therefore stop the
/// write before the ledger grows, not after.
#[test]
fn a_stage_that_cannot_be_published_stops_the_write_before_the_ledger() {
    use crate::kernel::path::{Step, fail, reset};

    for (step, when) in [
        (Step::PendingCreated, "the staging directory was created"),
        (Step::Staged, "the stage itself"),
    ] {
        let root = store();
        reset();
        fail(step);
        let refused = append_only(&root, lesson("the write that must not land"), "writer")
            .expect_err("a write whose stage could not be published");
        reset();
        assert!(
            matches!(refused, crate::kernel::error::Error::Integrity(_)),
            "publishing {when} failed and the write reported something else: {refused:?}"
        );
        assert_eq!(
            records(&root),
            0,
            "the ledger grew although publishing {when} failed"
        );
        let _ = fs::remove_dir_all(&root);
    }
}

/// The same, once the staging directory is no longer new.
#[test]
fn a_stage_in_an_existing_directory_still_has_to_be_published() {
    use crate::kernel::path::{Step, fail, reset};

    let root = store();
    append_only(
        &root,
        lesson("the write that establishes the store"),
        "writer",
    )
    .expect("first");
    let before = records(&root);

    reset();
    fail(Step::Staged);
    let refused = append_only(&root, lesson("the write that must not land"), "writer")
        .expect_err("a write whose stage could not be published");
    reset();
    assert!(
        matches!(refused, crate::kernel::error::Error::Integrity(_)),
        "an unpublished stage was reported as something else: {refused:?}"
    );
    assert_eq!(
        records(&root),
        before,
        "the ledger grew although the stage was never published"
    );
    let _ = fs::remove_dir_all(&root);
}

/// Records in the ledger, counting a store that has none as none.
fn records(root: &std::path::Path) -> usize {
    let directory = root.join("records");
    fs::read_dir(&directory)
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .filter_map(|entry| fs::read_to_string(entry.path()).ok())
                .map(|contents| {
                    contents
                        .lines()
                        .filter(|line| !line.trim().is_empty())
                        .count()
                })
                .sum()
        })
        .unwrap_or(0)
}
