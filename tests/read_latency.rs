//! Read paths through the CLI, on a disposable store.
//!
//! Kept alongside the session gate rather than replaced by it: this is what a
//! shell user or a script actually pays, and it is worth knowing even though the
//! owner's threshold is measured at the session boundary. A CLI invocation
//! includes a fork, an exec and a dynamic link, so the numbers here are larger
//! by that much and are reported rather than gated on.
//!
//! No vector projection is configured: a read has no use for a worker, so there
//! is nothing running in the background to interfere with or to wait for.
mod harness;

#[cfg(not(debug_assertions))]
use harness::{binary, exclusive_measurement, store};
#[cfg(not(debug_assertions))]
use std::path::Path;
#[cfg(not(debug_assertions))]
use std::process::Command;
#[cfg(not(debug_assertions))]
use std::time::{Duration, Instant};

/// A ceiling generous enough to catch a real regression without pretending the
/// owner's 50ms applies to a cold process.
#[cfg(not(debug_assertions))]
const CEILING: Duration = Duration::from_millis(250);
#[cfg(not(debug_assertions))]
const READS: usize = 50;
#[cfg(not(debug_assertions))]
const SEEDED: usize = 200;

#[cfg(not(debug_assertions))]
#[test]
fn cli_reads_are_measured_and_reported() {
    let _measuring = exclusive_measurement();
    let root = store("cli-read");
    let _ = std::fs::remove_file(root.join("registry/vector/qdrant.json"));
    seed(&root);
    harness::fixture::register_reader_profile(&root);

    for (label, args) in [
        (
            "cli search",
            vec!["search", "--query", "lesson", "--limit", "10"],
        ),
        (
            "cli context",
            vec!["context", "--profile", "reader", "--query", "lesson"],
        ),
    ] {
        let mut taken: Vec<Duration> = (0..READS).map(|_| run(&root, &args)).collect();
        taken.sort();
        let p95 = taken[(taken.len() * 95).div_ceil(100) - 1];
        let max = *taken.last().expect("timings");
        eprintln!(
            "{label}: p95={p95:?} max={max:?} median={:?}",
            taken[taken.len() / 2]
        );
        assert!(max <= CEILING, "{label}: max {max:?} over {CEILING:?}");
    }
    let _ = std::fs::remove_dir_all(&root);
}

#[cfg(not(debug_assertions))]
fn run(root: &Path, args: &[&str]) -> Duration {
    let started = Instant::now();
    let out = Command::new(binary())
        .args(args)
        .arg("--store")
        .arg(root)
        .env("EQUILL_ACTOR", "owner")
        .output()
        .expect("command");
    let elapsed = started.elapsed();
    assert!(
        out.status.success(),
        "{args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    elapsed
}

#[cfg(not(debug_assertions))]
fn seed(root: &Path) {
    for index in 0..SEEDED {
        let input = harness::draft(root, index);
        let out = Command::new(binary())
            .args(["record", "--input"])
            .arg(&input)
            .arg("--store")
            .arg(root)
            .env("EQUILL_ACTOR", "owner")
            .output()
            .expect("seed");
        assert!(out.status.success(), "seeding failed at {index}");
    }
}
