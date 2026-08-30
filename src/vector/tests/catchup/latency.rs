//! The claim this release exists to make: a write returns without waiting for
//! the index.
//!
//! Driven through the injected starter so it is deterministic and needs no
//! provider. A worker that claims the lock and then STAYS busy is the case that
//! matters — under the old synchronous design the append waited for exactly that
//! work, so a slow worker is what separates the two behaviours.
use super::harness::{LOCK, configured, counting_starter};
use crate::kernel::error::Error;
use crate::kernel::lock::TryLock;
use crate::vector::catchup::starter::with_starter;
use crate::vector::{after_commit, desired};
use std::path::Path;
use std::time::{Duration, Instant};

/// A coarse ceiling for the unit-level check. The owner's contract — 50
/// sequential writes, p95 under 50ms and max under 100ms — is measured against
/// the real binary in tests/write_latency.rs; this one only has to catch a write
/// that waits for a worker at all.
const RETURNS_WITHIN: Duration = Duration::from_millis(400);

/// How long the stand-in worker stays busy. Longer than the threshold the write
/// must beat, so a caller that waited for the worker cannot pass on timing.
const WORKER_RUNS_FOR: Duration = Duration::from_millis(1_500);

/// Claims the lock at once and then keeps working, on its own thread — the way
/// a real child embeds while the writer walks away. A caller that waits for this
/// takes at least WORKER_RUNS_FOR, which is what makes the timing assertion
/// meaningful rather than decorative.
fn slow_worker(store: &Path) -> Result<(), Error> {
    let taken = TryLock::acquire(store, LOCK).expect("lock").expect("free");
    std::thread::spawn(move || {
        std::thread::sleep(WORKER_RUNS_FOR);
        drop(taken);
    });
    Ok(())
}

#[test]
fn a_write_returns_without_waiting_for_a_busy_worker() {
    let root = configured("latency-busy");

    let started = Instant::now();
    let report = with_starter(slow_worker, || after_commit(&root, 0));
    let elapsed = started.elapsed();

    assert!(report.spawned, "the work was handed to the busy worker");
    assert!(
        elapsed < RETURNS_WITHIN,
        "the write waited {elapsed:?} on a worker that is still going"
    );
    // Direct proof rather than inference from a clock: the worker still holds
    // the lock at the moment the caller was released, so the caller demonstrably
    // did not wait for it to finish.
    assert!(
        TryLock::acquire(&root, LOCK).expect("probe").is_none(),
        "the worker had already finished; this test proved nothing"
    );
    // And it waited for none of the worker's progress: no passes, no embeddings
    // are attributed to the writing process.
    assert_eq!((report.passes, report.embeddings), (0, 0));
    assert!(!report.ran);
    // The target is durable before the caller is released, so the busy worker —
    // or the next one — will see this revision.
    let target = desired::read(&root).expect("read").expect("published");
    assert_eq!(target.revision, 1);
}

/// The other side: a writer that arrives while a claim is already live. It must
/// not start a second worker and must not wait to discover that.
#[test]
fn a_second_write_starts_nothing_and_still_returns_at_once() {
    let root = configured("latency-second");
    with_starter(counting_starter, || after_commit(&root, 0));

    let started = Instant::now();
    let report = with_starter(counting_starter, || after_commit(&root, 0));
    let elapsed = started.elapsed();

    assert!(
        !report.spawned,
        "a live claim means somebody is already on it"
    );
    assert!(
        elapsed < RETURNS_WITHIN,
        "the write waited {elapsed:?} to find out somebody else was working"
    );
    // The target still advances, so whoever is working will see this revision.
    assert!(desired::read(&root).expect("read").is_some());
}
