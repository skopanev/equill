//! What a durable record confirmation costs, and whether it grows with the
//! store.
//!
//! Measured through one finite MCP session against the real binary — the path a
//! working agent uses — with cold and warm reported separately. The owner's
//! hard ceiling is 250ms per invocation; 50ms p95 and 100ms max are printed as
//! quality targets.
//!
//! The test that matters most here is the last one: the same measurement on an
//! empty store and on one that already holds several hundred records. A
//! confirmation that re-reads the ledger costs more on the larger store. That is
//! observable from outside the process, which a counter inside it would not be.
mod harness;

#[cfg(not(debug_assertions))]
use harness::provider::SlowProvider;
#[cfg(not(debug_assertions))]
use harness::session::Session;
#[cfg(not(debug_assertions))]
use harness::{children, exclusive_measurement, settles, store_against};
#[cfg(not(debug_assertions))]
use std::path::Path;
#[cfg(not(debug_assertions))]
use std::time::Duration;

/// The owner's hard release ceiling.
#[cfg(not(debug_assertions))]
const CEILING: Duration = Duration::from_millis(250);
#[cfg(not(debug_assertions))]
const TARGET_P95: Duration = Duration::from_millis(50);
#[cfg(not(debug_assertions))]
const TARGET_MAX: Duration = Duration::from_millis(100);
#[cfg(not(debug_assertions))]
const CALLS: usize = 60;
/// Large enough that a full-ledger read is unmistakable in the timings.
#[cfg(not(debug_assertions))]
const HISTORY: usize = 400;

#[cfg(not(debug_assertions))]
struct Timings {
    p50: Duration,
    p95: Duration,
    max: Duration,
}

#[cfg(not(debug_assertions))]
fn timings(mut taken: Vec<Duration>) -> Timings {
    taken.sort();
    Timings {
        p50: taken[taken.len() / 2],
        p95: taken[(taken.len() * 95).div_ceil(100) - 1],
        max: *taken.last().expect("timings"),
    }
}

#[cfg(not(debug_assertions))]
fn report(label: &str, timings: &Timings) {
    let met = timings.p95 <= TARGET_P95 && timings.max <= TARGET_MAX;
    eprintln!(
        "{label}: p50={:?} p95={:?} max={:?} — quality target {}",
        timings.p50,
        timings.p95,
        timings.max,
        if met { "met" } else { "missed" }
    );
    assert!(
        timings.max <= CEILING,
        "{label}: max {:?} over the {CEILING:?} release ceiling",
        timings.max
    );
}

#[cfg(not(debug_assertions))]
fn draft(index: usize) -> serde_json::Value {
    serde_json::json!({
        "namespace": "agent.memory",
        "type": "agent.lesson.v1",
        "observed_at": "2026-01-01T00:00:00Z",
        "payload": { "rule": format!("confirmation lesson number {index}") }
    })
}

/// One write, asserted as well as timed: durable, queued, and no worker failure
/// filed while it happened.
#[cfg(not(debug_assertions))]
fn write(session: &mut Session, root: &Path, index: usize) -> Duration {
    let (elapsed, response) = session.tool("record", serde_json::json!({ "draft": draft(index) }));
    assert!(
        response["error"].is_null(),
        "write {index} failed: {response}"
    );
    // Exactly queued, not "queued or disabled". Every store in this file is
    // configured with a projection, so accepting disabled would let the whole
    // measurement pass if that configuration ever went missing — the assertion
    // would then be proving nothing while looking green.
    let body = response.to_string();
    assert!(
        body.contains("\"projection\":\"queued\""),
        "write {index} did not report projection=queued: {response}"
    );
    elapsed
}

/// A failure filed since a given moment.
///
/// Compared against a mark rather than checked outright: a worker waiting on the
/// stalled provider exits at its ten-second client timeout by design, so a
/// long seeding phase WILL leave a filed failure behind. What must not happen is
/// a failure appearing during a measurement, because that would mean the
/// confirmations being timed were not running beside a live worker.
#[cfg(not(debug_assertions))]
fn failure_mark(root: &Path) -> Option<std::time::SystemTime> {
    std::fs::metadata(root.join("projections/qdrant/last-drain.json"))
        .and_then(|data| data.modified())
        .ok()
}

#[cfg(not(debug_assertions))]
fn failed_since(root: &Path, mark: Option<std::time::SystemTime>) -> bool {
    if !failed_outcome(root) {
        return false;
    }
    match (failure_mark(root), mark) {
        (Some(now), Some(then)) => now > then,
        (Some(_), None) => true,
        _ => false,
    }
}

#[cfg(not(debug_assertions))]
fn failed_outcome(root: &Path) -> bool {
    let Ok(bytes) = std::fs::read(root.join("projections/qdrant/last-drain.json")) else {
        return false;
    };
    let state: serde_json::Value = serde_json::from_slice(&bytes).unwrap_or_default();
    state["outcome"] == "failed"
}

/// Cold is the first confirmation in a fresh session on a fresh store: nothing
/// is open, nothing is cached. Warm is everything after it.
#[cfg(not(debug_assertions))]
#[test]
fn cold_and_warm_confirmations_stay_under_the_ceiling() {
    let _measuring = exclusive_measurement();
    let provider = SlowProvider::start();
    let root = store_against("confirm-cold", &provider.endpoint());
    let mut session = Session::open(&root);

    let cold = write(&mut session, &root, 0);
    eprintln!("cold record: {cold:?}");
    assert!(
        cold <= CEILING,
        "the first confirmation took {cold:?}, over the {CEILING:?} ceiling"
    );

    let mark = failure_mark(&root);
    let warm = timings(
        (1..CALLS)
            .map(|index| write(&mut session, &root, index))
            .collect(),
    );
    report("warm record", &warm);
    assert!(
        !failed_since(&root, mark),
        "a worker filed a failure during the measurement"
    );
    assert!(
        children(&root) > 0,
        "no worker is running at the end of the measurement"
    );

    drop(session);
    provider.release();
    settles(&root, harness::WORKER_PATIENCE);
    let _ = std::fs::remove_dir_all(&root);
}

/// The one that proves the ledger is not being re-read. Same measurement, one
/// store empty and one already holding four hundred records: if confirmation
/// scans history, the second is measurably slower.
#[cfg(not(debug_assertions))]
#[test]
fn confirmation_does_not_get_slower_as_the_store_grows() {
    let _measuring = exclusive_measurement();
    let provider = SlowProvider::start();

    let fresh = store_against("confirm-fresh", &provider.endpoint());
    let mut session = Session::open(&fresh);
    let mark = failure_mark(&fresh);
    let empty = timings(
        (0..CALLS)
            .map(|index| write(&mut session, &fresh, index))
            .collect(),
    );
    assert!(
        !failed_since(&fresh, mark),
        "a worker failed during the empty-store measurement"
    );
    drop(session);
    report("on an empty store", &empty);

    let aged = store_against("confirm-aged", &provider.endpoint());
    let mut session = Session::open(&aged);
    for index in 0..HISTORY {
        write(&mut session, &aged, index);
    }
    // Seeding takes longer than a worker's patience, so a filed failure from
    // that phase is expected. The mark is taken after it, before timing.
    let mark = failure_mark(&aged);
    let loaded = timings(
        (HISTORY..HISTORY + CALLS)
            .map(|index| write(&mut session, &aged, index))
            .collect(),
    );
    assert!(
        !failed_since(&aged, mark),
        "a worker failed during the aged-store measurement"
    );
    drop(session);
    report("on a store with history", &loaded);

    // Generous: the claim is that cost does not track history, not that the two
    // are identical. A full-ledger read over four hundred records is not a 30%
    // difference, it is a multiple.
    let ratio = loaded.p50.as_secs_f64() / empty.p50.as_secs_f64().max(f64::EPSILON);
    eprintln!("p50 ratio aged/empty: {ratio:.2}");
    assert!(
        ratio <= 1.5,
        "confirmation on a {HISTORY}-record store is {ratio:.2}x the cost on an empty one; \
         the ledger is still being read on the write path"
    );

    provider.release();
    settles(&fresh, harness::WORKER_PATIENCE);
    settles(&aged, harness::WORKER_PATIENCE);
    let _ = std::fs::remove_dir_all(&fresh);
    let _ = std::fs::remove_dir_all(&aged);
}
