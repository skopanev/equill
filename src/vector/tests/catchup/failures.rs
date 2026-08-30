//! What a handoff does when the child cannot be created, never claims the work,
//! or dies before an ordinary command comes along.
use super::harness::{configured, counting_starter, reset_starts, starts};
use crate::vector::catchup::starter::with_starter;
use crate::vector::{after_commit, resume};

/// A starter that fails, as a spawn can. The claim must be released and the
/// report must not claim a handoff that never happened.
fn failing_starter(_store: &std::path::Path) -> Result<(), crate::kernel::error::Error> {
    Err(crate::kernel::error::Error::Projection(
        "spawn refused".into(),
    ))
}

#[test]
fn a_failed_start_reports_no_handoff_and_frees_the_claim() {
    let root = configured("spawn-fails");

    let report = with_starter(failing_starter, || after_commit(&root, 0));

    assert!(!report.spawned, "a refused spawn is not a handoff");
    // A later attempt still works: a refused spawn must not leave a claim behind
    // that wedges every writer after it.
    let retry = with_starter(counting_starter, || resume(&root));
    assert!(retry.spawned, "the store is not poisoned by one failure");
}

/// A start that succeeds hands the work off, whether or not the child has taken
/// the lock yet. The parent deliberately does not wait to find out: waiting put
/// the provider's latency back on the writing path.
///
/// What stops a second worker is the claim, not an observation of the first.
#[test]
fn a_started_child_is_a_handoff_without_waiting_to_watch_it() {
    let root = configured("no-claim");

    let first = with_starter(counting_starter, || after_commit(&root, 0));
    let second = with_starter(counting_starter, || resume(&root));

    assert!(first.spawned, "the first caller started a worker");
    assert!(
        !second.spawned,
        "the second found a live claim and started nothing"
    );
    assert_eq!(starts(), 1, "one start, no matter how many callers");
}

/// The non-query half of the recovery requirement: a command that is not a
/// search, get or context must also restart a dead worker's work.
#[test]
fn a_non_query_command_also_restarts_outstanding_work() {
    let root = configured("non-query");
    reset_starts();
    with_starter(counting_starter, || after_commit(&root, 0));
    // That worker died without consuming its claim; recovery comes from the
    // claim being released, not from anyone watching it die.
    crate::vector::catchup::handoff::release_for_tests(&root);

    // schema list opens the store and reads nothing about vectors.
    let resumed = with_starter(counting_starter, || {
        crate::command::cli::Command::Schema {
            command: crate::command::cli::SchemaCommand::List {
                store: root.clone(),
            },
        }
        .store_to_resume()
        .map(resume)
    });

    assert!(
        resumed.is_some_and(|report| report.spawned),
        "an ordinary non-query command restarts the work"
    );
}
