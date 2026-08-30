//! The 0.2.9 failure shape, closed against the real CLI binary.
//!
//! Timing lives in tests/session_latency.rs: a CLI invocation pays for a fork,
//! an exec and a dynamic link before any of this code runs, which measures
//! process startup rather than the write. This file is correctness — it runs in
//! both profiles, because a wrong answer is wrong at any speed.
mod harness;

use harness::{settles, store};
use std::time::{Duration, Instant};

/// The write must still return promptly; this is a smoke ceiling, not the
/// owner's threshold.
const MAX: Duration = Duration::from_millis(100);

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
    assert_eq!(
        body["text_index"], "ready",
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
