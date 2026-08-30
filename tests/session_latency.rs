//! The owner's contract measured where an agent actually meets it: inside one
//! MCP session, at the JSON-RPC boundary.
//!
//! A CLI invocation pays for a fork, an exec and a dynamic link before any of
//! this code runs — on this host most of the 50ms budget — so timing it says
//! more about process startup than about a write. A session is opened once and
//! serves many calls, which is how a real agent uses the store, and it isolates
//! the number the contract is about.
//!
//! Release only: a debug build is a different program for timing purposes.
mod harness;

#[cfg(not(debug_assertions))]
use harness::provider::SlowProvider;
#[cfg(not(debug_assertions))]
use harness::session::Session;
#[cfg(not(debug_assertions))]
use harness::{exclusive_measurement, settles, store, store_against};
#[cfg(not(debug_assertions))]
use std::path::Path;
#[cfg(not(debug_assertions))]
use std::time::Duration;

#[cfg(not(debug_assertions))]
/// The owner's hard release ceiling: exceeding it is a failure.
const CEILING: Duration = Duration::from_millis(250);
/// The quality targets. Printed on every run and compared, but a miss is a
/// number to look at rather than a release blocker — the ceiling is what gates.
#[cfg(not(debug_assertions))]
const TARGET_P95: Duration = Duration::from_millis(50);
#[cfg(not(debug_assertions))]
const TARGET_MAX: Duration = Duration::from_millis(100);
#[cfg(not(debug_assertions))]
const CALLS: usize = 50;

#[cfg(not(debug_assertions))]
struct Timings {
    p95: Duration,
    max: Duration,
    median: Duration,
}

#[cfg(not(debug_assertions))]
fn timings(mut taken: Vec<Duration>) -> Timings {
    taken.sort();
    Timings {
        p95: taken[(taken.len() * 95).div_ceil(100) - 1],
        max: *taken.last().expect("timings"),
        median: taken[taken.len() / 2],
    }
}

#[cfg(not(debug_assertions))]
fn report(label: &str, timings: &Timings) {
    let met = timings.p95 <= TARGET_P95 && timings.max <= TARGET_MAX;
    eprintln!(
        "{label}: p95={:?} max={:?} median={:?} — quality target {}",
        timings.p95,
        timings.max,
        timings.median,
        if met { "met" } else { "missed" }
    );
    // Only the ceiling gates. The targets are reported so drift toward the
    // ceiling is visible before it becomes a failure.
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
        "payload": { "rule": format!("session lesson number {index}") }
    })
}

/// Fifty writes through one session, against a provider that keeps its worker
/// busy — the case where a write that waited on the projection would show.
#[cfg(not(debug_assertions))]
#[test]
fn a_session_write_returns_before_the_projection_does() {
    // Only against the other measurement in this suite, and only by a lock the
    // tests own.
    let _measuring = exclusive_measurement();
    let provider = SlowProvider::start();
    let root = store_against("session-write", &provider.endpoint());
    let mut session = Session::open(&root);

    let mut taken = Vec::with_capacity(CALLS);
    let mut queued = 0;
    for index in 0..CALLS {
        let (elapsed, response) =
            session.tool("record", serde_json::json!({ "draft": draft(index) }));
        assert!(
            response["error"].is_null(),
            "write {index} failed: {response}"
        );
        taken.push(elapsed);
        if response.to_string().contains("\"queued\"") {
            queued += 1;
        }
        // Checked on every call. A worker that died early would leave the rest
        // of the measurement timing writes with nothing running, which is a
        // different and much easier thing to be fast at.
        assert!(
            !failed_outcome(&root),
            "the worker filed a failure at write {index}; it was not busy"
        );
    }
    assert!(
        harness::children(&root) > 0,
        "no worker is running at the end of the measurement"
    );

    report("session record", &timings(taken));
    assert!(
        queued > 0,
        "no write reported a queued projection; the writes were not asynchronous"
    );
    // Waited for: a worker forked during the first write may still be reaching
    // the network when the last one returns, and a sampled check would call
    // that a failure.
    assert!(
        request_pending(&provider, Duration::from_secs(30)),
        "the worker never sent a request to the provider, so it was never busy"
    );
    drop(session);
    provider.release();
    settles(&root, Duration::from_secs(10));
    let _ = std::fs::remove_dir_all(&root);
}

/// The reads, through the same session. Fifty of each.
#[cfg(not(debug_assertions))]
#[test]
fn session_reads_stay_within_the_contract() {
    let _measuring = exclusive_measurement();
    // No vector projection on this store: a read measurement has no use for a
    // worker, and not configuring one means there is nothing to wait for and
    // nothing to interfere with.
    let root = store_without_projection("session-read");
    seed(&root);
    let mut session = Session::open(&root);

    let search = timings(
        (0..CALLS)
            .map(|_| {
                let (elapsed, response) = session.tool(
                    "search",
                    serde_json::json!({ "query": "lesson", "limit": 10 }),
                );
                assert!(response["error"].is_null(), "search failed: {response}");
                elapsed
            })
            .collect(),
    );
    report("session search", &search);

    let context = timings(
        (0..CALLS)
            .map(|_| {
                let (elapsed, response) = session.tool(
                    "context",
                    serde_json::json!({ "profile": "reader", "query": "lesson" }),
                );
                assert!(response["error"].is_null(), "context failed: {response}");
                elapsed
            })
            .collect(),
    );
    report("session context", &context);

    drop(session);
    let _ = std::fs::remove_dir_all(&root);
}

/// Two hundred records and a profile to read them through, written before any
/// timing starts.
#[cfg(not(debug_assertions))]
fn seed(root: &Path) {
    let mut session = Session::open(root);
    for index in 0..200 {
        let response = session
            .tool("record", serde_json::json!({ "draft": draft(index) }))
            .1;
        assert!(
            response["error"].is_null(),
            "seed {index} failed: {response}"
        );
    }
    drop(session);
    harness::fixture::register_reader_profile(root);
}

/// A store with schema and records but no vector configuration, so a read
/// measurement starts no background work at all.
#[cfg(not(debug_assertions))]
fn store_without_projection(name: &str) -> std::path::PathBuf {
    let root = store(name);
    let _ = std::fs::remove_file(root.join("registry/vector/qdrant.json"));
    root
}

/// Whether a worker has already filed a failed outcome for this store.
#[cfg(not(debug_assertions))]
fn failed_outcome(root: &Path) -> bool {
    let Ok(bytes) = std::fs::read(root.join("projections/qdrant/last-drain.json")) else {
        return false;
    };
    let state: serde_json::Value = serde_json::from_slice(&bytes).unwrap_or_default();
    state["outcome"] == "failed"
}

/// Wait until the provider is holding a request from the worker.
#[cfg(not(debug_assertions))]
fn request_pending(provider: &SlowProvider, within: Duration) -> bool {
    let deadline = std::time::Instant::now() + within;
    while std::time::Instant::now() < deadline {
        if provider.requests() > 0 {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    false
}
