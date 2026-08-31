//! The 0.2.9 failure shape, closed against the real CLI binary.
//!
//! Timing lives in tests/session_latency.rs: a CLI invocation pays for a fork,
//! an exec and a dynamic link before any of this code runs, which measures
//! process startup rather than the write. This file is correctness — it runs in
//! both profiles, because a wrong answer is wrong at any speed.
mod harness;

use harness::{settles, store};
use std::time::{Duration, Instant};

/// A ceiling that separates the two things this test is between: a write that
/// went to the provider, and a write that ran on a busy machine.
///
/// The failure being closed took 22 to 31 SECONDS, because it waited on a
/// provider that was not there. Anything in that class is orders of magnitude
/// over this. A tight number here would instead be measuring the machine — a
/// debug-build CLI invocation timed while the rest of the suite runs — and it
/// did: 63-80ms alone, over 100ms once in three runs under full parallel load.
/// A ceiling that fails for that reason teaches everyone to rerun until green.
///
/// The owner-facing numbers are measured where the owner's contract lives, at
/// the MCP boundary in tests/write_confirmation.rs, in release, serialized
/// against other measurements.
const MAX: Duration = Duration::from_secs(2);

/// The exact failure shape 0.2.9 shipped, closed against the real binary.
///
/// Live 0.2.9 took 22-31 SECONDS on a write, and reported a vector attempt error
/// while announcing the projection ready — a response that said, at once, that
/// the index was up to date and that updating it had just failed. One canonical
/// record command has to disprove every part of that.
#[test]
fn one_canonical_write_against_a_dead_provider_has_the_right_shape() {
    let root = store("owner-regression");
    let input = harness::draft(&root, 0);

    let started = Instant::now();
    let out = std::process::Command::new(harness::binary())
        .args(["record", "--json", "--input"])
        .arg(&input)
        .arg("--store")
        .arg(&root)
        .env("EQUILL_ACTOR", "owner")
        .output()
        .expect("record");
    let elapsed = started.elapsed();

    assert!(out.status.success(), "the write must exit zero");
    let body: serde_json::Value = serde_json::from_slice(&out.stdout).expect("json response");
    eprintln!("owner regression: elapsed={elapsed:?} body={body}");

    assert_eq!(body["ok"], true, "the write reports success");
    assert_eq!(body["durable"], true, "and says the record is durable");
    assert!(
        elapsed <= MAX,
        "the write took {elapsed:?}, over the {MAX:?} ceiling"
    );
    // Named, and honest. This said "ready" while the text index was written
    // inside confirmation. It is written after it now — for the same reason the
    // vector index is: a caller waiting on a projection is waiting on something
    // the ledger can reconstruct. "queued" is the weaker claim and the true one.
    assert_eq!(
        body["text_index"], "queued",
        "the text index state is named, not left as a bare `projection`"
    );
    assert!(
        body["projection"].is_null(),
        "an unqualified `projection` field reads as a claim about search overall"
    );
    assert_eq!(
        body["vector"]["projection"], "queued",
        "a write against a dead provider is queued, never current"
    );
    assert!(
        body["vector"]["attempt_error"].is_null(),
        "the writing process must not have tried the provider itself: {}",
        body["vector"]
    );
    // The human-readable form must not claim general freshness either.
    let text = std::process::Command::new(harness::binary())
        .args(["record", "--input"])
        .arg(harness::draft(&root, 1))
        .arg("--store")
        .arg(&root)
        .env("EQUILL_ACTOR", "owner")
        .output()
        .expect("record");
    let printed = String::from_utf8_lossy(&text.stdout);
    assert!(
        printed.contains("Vector index: queued"),
        "the printed form hides the vector state: {printed}"
    );
    settles(&root, Duration::from_secs(10));
    let _ = std::fs::remove_dir_all(&root);
}
