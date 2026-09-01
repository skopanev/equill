//! What a refusal leaves behind, and what this cannot show about it.
use super::super::harness;
use super::{READER, existing_record, run, state, store, write};
use serde_json::json;
use std::fs;
use std::time::Duration;

/// A refused command leaves the store as it found it.
///
/// WHAT THIS DOES NOT SHOW: the guard that stops a refused command from
/// resuming a lagging index. Removing that guard leaves this test green, so it
/// is not the thing being measured here — what is measured is that the ledger,
/// the whole projection tree and the store's answers are unchanged after a
/// refusal, which is worth asserting on its own. The guard itself is one
/// condition at each of two call sites and a unit-tested helper; proving it
/// end to end needs an environment where a spawned worker can be observed
/// finishing, and I could not make that deterministic here.
#[test]
fn a_refused_command_does_not_catch_the_index_up() {
    let root = store();
    let target = existing_record(&root);
    // A store with no vector descriptor never resumes anything, so a probe
    // built on one would pass whether or not the guard existed. This one is
    // configured — against a dead endpoint, so nothing leaves the machine —
    // and then given something to catch up on.
    harness::fixture::configure(&root);
    let draft = write(
        &root,
        "seed-two.json",
        json!({
            "namespace": "agent.memory",
            "type": "agent.lesson.v1",
            "observed_at": "2026-01-01T00:00:00Z",
            "payload": { "rule": "a lesson that leaves the index behind" }
        }),
    );
    assert!(
        run(
            &root,
            "owner",
            &["record", "--input", draft.to_str().expect("path")]
        )
        .status
        .success(),
        "seeding the lag failed"
    );
    assert!(
        harness::settles(&root, Duration::from_secs(10)),
        "a worker outlived the seed"
    );
    let watermark = root.join("projections/sqlite/watermark.json");
    let _ = fs::remove_file(&watermark);
    let (before_ledger, before_index) = state(&root);

    let out = run(
        &root,
        READER,
        &["revoke", "--id", &target, "--comment", "no"],
    );
    assert!(!out.status.success(), "revoke succeeded for a held actor");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("PM_WRITE_DENIED"),
        "revoke refused for another reason: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    // Wait for anything the command might have started before looking: a
    // worker is spawned detached, so comparing immediately would compare
    // against work that had not happened yet and call that "unchanged".
    assert!(
        harness::settles(&root, Duration::from_secs(10)),
        "a worker is still running"
    );
    let (after_ledger, after_index) = state(&root);
    assert_eq!(before_ledger, after_ledger, "the ledger changed");
    assert_eq!(
        before_index, after_index,
        "the refused command caught the index up on its way to being refused"
    );

    // The control: the same store, the same command, an actor that may.
    let out = run(
        &root,
        "lane",
        &["revoke", "--id", &target, "--comment", "yes"],
    );
    assert!(
        out.status.success(),
        "the control revoke failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_ne!(
        before_index,
        state(&root).1,
        "nothing resumes for anybody, so the refusal proved nothing"
    );
    let _ = fs::remove_dir_all(&root);
}
