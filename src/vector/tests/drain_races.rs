//! The interleavings a catch-up has to survive.
use super::drain::{add, configure_unreachable, store};
use crate::vector::after_commit;
use std::fs;

/// The regression this exists to prevent: two catch-ups interleaving so that
/// one publishes a target the other has already passed. A watermark that walks
/// backwards leaves the drain comparing an index at N+1 against a target of N,
/// which it can never satisfy — a loop that never agrees with itself.
#[test]
fn a_published_target_never_walks_backwards() {
    let root = store("monotonic");
    configure_unreachable(&root);
    add(&root, "first");
    add(&root, "second");
    after_commit(&root, "owner");
    let reached = super::super::desired::read(&root)
        .expect("desired")
        .expect("published")
        .records;

    // A stale publisher, holding a snapshot from before those writes, tries to
    // record what it saw. It must not undo what has already been reached.
    super::super::desired::publish(&root, reached - 1, &"a".repeat(64)).expect("stale publish");
    let after_stale = super::super::desired::read(&root)
        .expect("desired")
        .expect("published");

    assert_eq!(after_stale.records, reached, "a stale target is ignored");
    // Moving forward still works, which is the point of the guard.
    super::super::desired::publish(&root, reached + 1, &"b".repeat(64)).expect("forward publish");
    assert_eq!(
        super::super::desired::read(&root)
            .expect("desired")
            .expect("published")
            .records,
        reached + 1
    );
    fs::remove_dir_all(root).expect("cleanup");
}

/// Concurrent writers must leave the watermark describing the ledger as it
/// actually is, not as whichever of them read it last happened to see.
#[test]
fn concurrent_catch_ups_agree_on_the_ledger_they_left_behind() {
    let root = store("concurrent");
    configure_unreachable(&root);
    let writers = (0..4)
        .map(|index| {
            let root = root.clone();
            std::thread::spawn(move || {
                add(&root, &format!("rule {index}"));
                after_commit(&root, "owner");
            })
        })
        .collect::<Vec<_>>();
    for writer in writers {
        writer.join().expect("writer thread");
    }

    let published = super::super::desired::read(&root)
        .expect("desired")
        .expect("published");
    let ledger = crate::record::read_all(&root).expect("records").len();

    assert_eq!(ledger, 4, "every write landed");
    assert_eq!(
        published.records, ledger,
        "the target describes the ledger, whoever published last"
    );
    fs::remove_dir_all(root).expect("cleanup");
}

/// What makes the holder loop instead of leaving: a write that lands during an
/// active drain moves the target past what is indexed, and the holder's exit
/// condition must then be false. The previous busy test was sequential — every
/// `add` already drains — so it never exercised this at all.
#[test]
fn a_write_during_a_drain_denies_the_exit_condition() {
    let root = store("second-pass");
    configure_unreachable(&root);
    add(&root, "already indexed");
    // Pretend the drain got this far: the checkpoint matches the ledger.
    let (records, digest) = super::super::operator::corpus(&root).expect("corpus");
    let config = super::super::config::load(&root)
        .expect("config")
        .expect("configured");
    super::super::state::stage_ready(
        &root,
        &config,
        "equill_drain_test_physical",
        Some((records.len(), &digest)),
    )
    .expect("stage")
    .commit()
    .expect("commit");
    super::super::desired::publish(&root, records.len(), &digest).expect("publish");
    let settled = super::super::drain::caught_up(&root).expect("caught up");

    // Now a write lands while the holder still owns the drain lock.
    add(&root, "arrived mid-drain");
    let (after, after_digest) = super::super::operator::corpus(&root).expect("corpus");
    super::super::desired::publish(&root, after.len(), &after_digest).expect("publish");
    let unsettled = super::super::drain::caught_up(&root).expect("caught up");

    assert!(settled, "with nothing outstanding the holder may leave");
    assert!(
        !unsettled,
        "with a tail outstanding the holder must take another pass"
    );
    fs::remove_dir_all(root).expect("cleanup");
}
