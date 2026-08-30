//! The two things that run before every command: is anything outstanding, and
//! may this worker run at all.
use super::harness::{bare, configured, counting_starter, starts};
use crate::vector::catchup::starter::with_starter;
use crate::vector::drain::outstanding_for_tests;
use crate::vector::{after_commit, handoff_for_tests, resume, run_worker};

/// The gate must not read the ledger. It answers from two small marker files,
/// because it runs before every command and a store's ledger can be large.
#[test]
fn the_gate_answers_from_markers_and_never_scans_the_ledger() {
    let root = configured("gate-cheap");
    with_starter(counting_starter, || after_commit(&root, 0));
    // Make the ledger enormous relative to the markers. If the gate scanned it,
    // this would be the slow case; it must not even notice.
    let ledger = root.join("records");
    let file = std::fs::read_dir(&ledger)
        .expect("records")
        .flatten()
        .next()
        .expect("a ledger file")
        .path();
    let line = std::fs::read_to_string(&file).expect("read");
    let one = line.lines().next().expect("a record").to_owned();
    let bulk = std::iter::repeat_n(one.as_str(), 5_000)
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(&file, format!("{line}\n{bulk}\n")).expect("bulk");

    let started = std::time::Instant::now();
    let answer = outstanding_for_tests(&root);
    let elapsed = started.elapsed();

    assert!(
        answer,
        "a published target with no checkpoint is outstanding"
    );
    assert!(
        elapsed < std::time::Duration::from_millis(50),
        "the gate took {elapsed:?} on a 5000-line ledger; it is scanning it"
    );
}

/// A store with the projection off never reaches the gate's later steps, and a
/// store with no target has nothing to do.
#[test]
fn a_store_with_no_target_is_not_outstanding() {
    let root = bare("gate-quiet");

    assert!(
        !outstanding_for_tests(&root),
        "nothing published, nothing owed"
    );
    let report = with_starter(counting_starter, || resume(&root));
    assert!(!report.spawned);
    assert_eq!(starts(), 0);
}

/// The worker command is reachable by anyone who can run the binary. Hiding it
/// from `--help` is presentation; the ticket is the part that refuses.
#[test]
fn a_worker_without_a_handoff_refuses_to_run() {
    let root = bare("no-ticket");

    let refused = run_worker(&root);

    assert!(refused.is_err(), "an unissued worker must not run");
    let message = refused.unwrap_err().to_string();
    assert!(message.contains("no handoff"), "{message}");
}

/// And a ticket is single use: the second attempt finds nothing to consume, so
/// a leaked invocation cannot be replayed.
#[test]
fn a_handoff_can_only_be_consumed_once() {
    let root = bare("one-ticket");
    handoff_for_tests(&root).expect("issue");

    run_worker(&root).expect("the issued worker runs");
    let replay = run_worker(&root);

    assert!(replay.is_err(), "the ticket does not survive its use");
    assert!(
        !crate::vector::handoff_path_for_tests(&root).exists(),
        "and nothing is left behind to find"
    );
}

/// An invariant, not a scenario: a failed attempt and a current index cannot
/// both be true. Whatever else a report says, those two together would tell a
/// caller the index is up to date and that catching it up just failed.
#[test]
fn a_failed_attempt_is_never_reported_as_a_current_index() {
    let root = configured("never-current-on-failure");

    // Inline, so the failure against the unreachable provider lands in this
    // report rather than a child's.
    let report = crate::vector::after_commit_inline(&root, 1);

    assert!(
        report.attempt_error.is_some(),
        "the provider is unreachable; this must fail"
    );
    assert!(
        !matches!(report.projection, crate::vector::Projection::Current),
        "a report that failed claimed the index was current: {report:?}"
    );
}
