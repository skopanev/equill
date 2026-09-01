//! Putting a store genuinely behind, and watching whether a command catches
//! it up.
//!
//! Three things have to be true at once or the watching is vacuous, and each
//! of them silently turns the measurement green on its own:
//!
//! 1. The vector descriptor must exist BEFORE the record that is being caught
//!    up on. The catch-up needs a published target, and the writer publishes it
//!    on commit — configure afterwards and there is no target, so nothing ever
//!    resumes and the guard is not what made the store quiet.
//! 2. Cooldown must be cleared before every measurement. One failed spawn
//!    against the dead endpoint puts the store in cooldown, and the catch-up
//!    returns early while it is in effect — so every run after the first does
//!    nothing whatever the guard does.
//! 3. The trace to watch is the cooldown file, not the projection. Nothing
//!    reaches the endpoint here, so a catch-up that ran and one that was
//!    prevented leave identical data behind; what tells them apart is that the
//!    one that ran claimed the work, forked, failed to reach the address and
//!    recorded the failure.
use crate::harness;
use crate::{run, write};
use serde_json::json;
use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};

/// How long a worker is given to show itself. Every use pairs this negative
/// with a positive control measured over the same window, so a machine too
/// slow for this bound fails loudly instead of passing vacuously.
const WINDOW: Duration = Duration::from_secs(5);

/// Clear what would silence the next measurement.
pub fn unmute(root: &Path) {
    for name in ["cooldown.json", "handoff.json", "handoff-active.json"] {
        let _ = fs::remove_file(root.join("projections/qdrant").join(name));
    }
}

/// Wait for a command to start a catch-up, or give up.
pub fn starts(root: &Path) -> bool {
    let cooldown = root.join("projections/qdrant/cooldown.json");
    let deadline = Instant::now() + WINDOW;
    while Instant::now() < deadline {
        if cooldown.is_file() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    false
}

/// Clear the trace and confirm it stays cleared.
///
/// One wait is not enough. The seed runs more than one worker — the command
/// starts a catch-up and the writer starts another — and they fail at their
/// own pace, so a trace can land after the first has been waited for and
/// cleared. Clearing once and measuring immediately reads that straggler as
/// "the refused call started a catch-up": that is exactly how this test went
/// red with both guards in place, and only when the suite ran in parallel,
/// where everything is slower and the straggler had further to fall behind.
///
/// So: clear, watch for the length of a measurement, and if anything appears,
/// clear again. Silence for a whole window is the store actually being quiet.
fn quiesce(root: &Path) {
    for _ in 0..5 {
        unmute(root);
        // Both, and in this order: no worker still running for this store, and
        // no trace appearing for a whole measurement afterwards. Either alone
        // has been observed lying — `settles` reports an empty machine while a
        // fork is still on its way, and a cleared trace says nothing about the
        // worker that has not failed yet.
        if harness::settles(root, Duration::from_secs(30)) && !starts(root) {
            return;
        }
    }
    panic!("the store never went quiet, so a later trace could not be attributed");
}

/// Leave the store configured, behind, and quiet: the next command that may
/// resume will, and nothing is left over to be mistaken for it.
pub fn prepare(root: &Path) {
    // Configured against a dead endpoint, so nothing leaves the machine.
    harness::fixture::configure(root);
    let draft = write(
        root,
        "lag.json",
        json!({
            "namespace": "agent.memory",
            "type": "agent.lesson.v1",
            "observed_at": "2026-01-01T00:00:00Z",
            "payload": { "rule": "a lesson that leaves the index behind" }
        }),
    );
    assert!(
        run(
            root,
            "owner",
            &["record", "--input", draft.to_str().expect("path")]
        )
        .status
        .success(),
        "seeding the lag failed"
    );
    // Wait for the seed's own worker to exist before waiting for it to leave:
    // `settles` on a store whose worker has not been forked yet answers about
    // an empty machine, and that worker then writes under the snapshot taken
    // by the caller.
    assert!(
        harness::appears(
            &root.join("projections/qdrant/last-drain.json"),
            Duration::from_secs(10)
        ),
        "the seed never started a worker, so there is no catch-up to prevent"
    );
    assert!(
        harness::settles(root, Duration::from_secs(10)),
        "a worker outlived the seed"
    );
    // And then wait for that worker to finish FAILING. It is detached, so
    // `settles` can report an empty machine while the fork is still on its way
    // to the dead endpoint; its cooldown then lands seconds later and reads as
    // "the refused command started a catch-up". Waiting for the trace the seed
    // is certain to leave is what makes the later measurement mean anything.
    assert!(
        harness::appears(
            &root.join("projections/qdrant/cooldown.json"),
            Duration::from_secs(30)
        ),
        "the seed's worker never finished, so a later trace cannot be attributed"
    );
    // With no checkpoint against a published target the index cannot be shown
    // to be current, so the next command that may resume will.
    let _ = fs::remove_file(root.join("projections/qdrant/state.json"));
    let _ = fs::remove_file(root.join("projections/sqlite/watermark.json"));
    quiesce(root);
}
