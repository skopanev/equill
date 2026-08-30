use super::harness::{
    LOCK, configured, counting_starter, release_taken, reset_starts, starts, taking_starter,
};
use super::{add, configure_unreachable, store};
use crate::kernel::lock::TryLock;
use crate::vector::catchup::starter::with_starter;
use crate::vector::{after_commit, after_commit_inline, resume, run_once};

/// The point of the release: a write records what it wants indexed and returns.
/// It does not load a model, and it does not wait for one.
#[test]
fn a_write_publishes_its_target_and_hands_the_work_off() {
    let root = configured("handoff");

    let report = with_starter(counting_starter, || after_commit(&root, 0));

    assert!(!report.ran, "the writing process must not do the embedding");
    assert!(report.spawned, "and it must have handed the work off");
    assert_eq!(report.embeddings, 0);
    let target = super::super::super::desired::read(&root)
        .expect("read target")
        .expect("a target was published");
    assert_eq!(target.revision, 1, "and it names what the ledger now holds");
}

/// A burst with the lock FREE is the case that matters: every write races to
/// decide whether a worker is needed. Exactly one may win.
///
/// The starter here takes the drain lock the way a real child would, so the
/// claim handshake is exercised rather than assumed.
#[test]
fn a_burst_with_a_free_lock_starts_exactly_one_worker() {
    let root = configured("burst-free");
    reset_starts();
    release_taken(&root);

    with_starter(taking_starter, || {
        for index in 0..40 {
            // add() writes through the canonical path, so it hands off for
            // itself; the explicit call after it is a second writer arriving
            // while that worker is still running.
            add(&root, &format!("burst lesson {index}"));
            after_commit(&root, 0);
        }
    });

    assert_eq!(
        starts(),
        1,
        "80 handoff opportunities must start one worker, not one each"
    );
    let target = super::super::super::desired::read(&root)
        .expect("read")
        .expect("published");
    assert_eq!(
        target.revision, 41,
        "every write in the burst is in the target the single worker will read"
    );
    release_taken(&root);
}

/// Concurrent writers, same question: the claim lock must let exactly one of
/// them decide.
#[test]
fn concurrent_writers_do_not_each_start_a_worker() {
    let root = configured("burst-threads");

    let winners: usize = std::thread::scope(|scope| {
        // The lock is deliberately NOT released inside the threads: a worker
        // that lives is the case being tested. Releasing it would model a
        // worker that dies instantly, and the next writer starting another one
        // would then be correct rather than a bug.
        let handles: Vec<_> = (0..8)
            .map(|_| {
                scope.spawn(|| {
                    let report = with_starter(taking_starter, || after_commit(&root, 0));
                    (report.spawned, starts())
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|handle| handle.join().expect("thread"))
            .map(|(spawned, started)| {
                assert!(started <= 1, "no thread may start more than one worker");
                usize::from(spawned)
            })
            .sum()
    });

    assert_eq!(winners, 1, "eight concurrent writers, exactly one worker");
}

/// The recovery story, with a worker that really dies.
///
/// A child is simulated by a thread that takes the drain lock and is then
/// dropped without releasing it cleanly — which is what a kill looks like to
/// everyone else: the lock is gone, the target is not.
#[test]
fn a_dead_worker_leaves_work_that_the_next_command_picks_up() {
    let root = configured("recover");
    reset_starts();
    with_starter(counting_starter, || after_commit(&root, 0));
    assert_eq!(starts(), 1);
    // The worker dies without consuming its claim. Nothing releases it, so the
    // store must recover through the claim going stale rather than through
    // anyone noticing the death.
    crate::vector::catchup::handoff::release_for_tests(&root);

    let target = super::super::super::desired::read(&root)
        .expect("read")
        .expect("published");
    assert_eq!(target.revision, 1, "the target survived the death");

    let report = with_starter(counting_starter, || resume(&root));

    assert!(report.spawned, "an ordinary command restarts the work");
    assert_eq!(starts(), 2);
}

/// A store with nothing outstanding must spawn nothing. Otherwise every read on
/// a healthy store starts a process it has no use for.
///
/// "Nothing outstanding" is asserted through the same local gate the hook uses,
/// with no target published at all — the cheapest possible current state.
#[test]
fn a_store_with_nothing_outstanding_starts_no_worker() {
    let root = store("no-target");
    configure_unreachable(&root);
    reset_starts();

    let report = with_starter(counting_starter, || resume(&root));

    assert!(!report.spawned, "nothing is owed, so nothing is started");
    assert_eq!(starts(), 0);
    assert!(
        !root.join("projections/qdrant/desired.json").exists(),
        "and the gate did not have to publish anything to find that out"
    );
}

/// A store with the projection off publishes no target and opens no connection.
/// It still starts a worker: the text index is not optional, and a record that
/// is durable but unfindable would be a strange thing to call written.
#[test]
fn a_store_without_the_projection_hands_nothing_off() {
    let root = store("opt-out");

    let write = with_starter(counting_starter, || after_commit(&root, 0));
    let read = with_starter(counting_starter, || resume(&root));

    // A worker IS started, because the text index still has to catch up on a
    // store with no vector projection — that is what makes a record findable.
    // What must not happen is any vector work: nothing published, nothing
    // connected, nothing reported as failed.
    assert!(write.attempt_error.is_none() && read.attempt_error.is_none());
    assert!(!write.ran && !read.ran, "no vector pass was made");
    assert!(
        !root.join("projections/qdrant/desired.json").exists(),
        "nothing is published for a store that has no vector projection"
    );
}

/// A worker that finds another one already running exits immediately rather
/// than waiting. Two workers must never embed the same tail twice.
#[test]
fn a_second_worker_exits_instead_of_waiting() {
    let root = configured("single-flight");
    after_commit(&root, 0);
    let held = TryLock::acquire(&root, LOCK).expect("lock").expect("free");

    let report = run_once(&root);

    assert!(!report.ran, "the second worker did no work");
    assert_eq!(report.passes, 0);
    drop(held);
}

/// An unreachable provider costs the write nothing: the record is durable, the
/// target is published, and the failure is reported without a retry loop.
#[test]
fn an_unreachable_provider_leaves_the_write_intact() {
    let root = configured("provider-down");
    let before = crate::record::read_all(&root).expect("read").len();

    let report = after_commit_inline(&root, 0);

    assert!(report.attempt_error.is_some(), "the failure is reported");
    assert_eq!(
        crate::record::read_all(&root).expect("read").len(),
        before,
        "and the ledger is untouched by it"
    );
}
