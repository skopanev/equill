//! Whether compaction may call the projections settled.
use super::plan_tests::{add, store};
use super::run::{run, with_pause};
use crate::record::read_all;
use std::time::Duration;

/// A catch-up that could not start is not a catch-up that finished.
///
/// When the drain lease is held elsewhere the worker returns a default report:
/// nothing done, and no error to notice. Reading that as success let compaction
/// declare the projections settled and clear its journal, leaving the survivors
/// carrying stale hashes with nothing left to find them.
#[test]
fn a_held_drain_lease_stops_compaction_claiming_the_projection_is_settled() {
    let root = store("held-lease");
    let first = add(&root, "older", None);
    add(&root, "newer", Some(first));

    // An index that answers, so the only thing that can fail is the catch-up.
    // With an unreachable provider this test would go red for a reason that has
    // nothing to do with the lease.
    let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let recorder = seen.clone();
    let index = std::sync::Arc::new(move |condemned: &[uuid::Uuid]| {
        recorder.lock().unwrap().extend_from_slice(condemned);
        Ok(true)
    });

    // Someone else is draining.
    let _lease = crate::kernel::lock::TryLock::acquire(&root, "vector-drain.lock")
        .expect("lock")
        .expect("the lease was already held");

    let outcome = super::projections::with_index(index, || run(&root, true, "owner"));

    // The removal is never reached now: the lease is taken before it, which is
    // stricter than refusing afterwards — a sync holding the lease has already
    // read a pre-compaction snapshot and would put these points back.
    assert!(
        seen.lock().unwrap().is_empty(),
        "points were removed while a catch-up held the lease"
    );

    assert!(
        outcome.is_err(),
        "compaction reported success while the projection was never settled"
    );
    assert!(
        super::journal::Journal::read(&root)
            .expect("journal")
            .is_some(),
        "the journal was cleared even though the work was not finished"
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// Governance and the writer hold different locks, so holding the governance
/// one says nothing about appends. Without the writer's lock a record landing
/// between the read and the swap is dropped by a rewrite that never saw it —
/// gone from an immutable ledger.
///
/// The window is opened deliberately rather than hoped for: a race test that
/// waits on luck comes out green whether or not the lock is held, which is the
/// failure this test exists to avoid in itself.
#[test]
fn a_record_written_inside_the_window_survives() {
    let root = store("window");
    let first = add(&root, "older", None);
    add(&root, "newer", Some(first));

    let racing = root.clone();
    let writer = std::thread::spawn(move || {
        // Late enough that compaction has read the ledger, early enough that it
        // has not published.
        std::thread::sleep(Duration::from_millis(120));
        add(&racing, "written during compaction", None)
    });
    let report =
        with_pause(Duration::from_millis(400), || run(&root, true, "owner")).expect("compaction");
    let landed = writer.join().expect("writer thread");

    assert_eq!(report.removed, 1);
    let after = read_all(&root).expect("ledger");
    assert!(
        after.iter().any(|record| record.id == landed),
        "a record written during compaction was dropped from the ledger"
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// A survivor's receipt describes the bytes that are now in the ledger.
///
/// Cutting a link rewrites the record, so its stored hash moves. A receipt
/// still carrying the old one would make a verification report corruption for
/// a record nobody touched — the store accusing itself of damage it did
/// deliberately.
#[test]
fn a_rewritten_survivor_and_its_receipt_agree() {
    let root = store("receipts");
    let first = add(&root, "older", None);
    let survivor = add(&root, "newer", Some(first));

    run(&root, true, "owner").expect("compaction");

    let record = read_all(&root)
        .expect("ledger")
        .into_iter()
        .find(|record| record.id == survivor)
        .expect("survivor");
    let digest = crate::kernel::digest::sha256_hex(&serde_json::to_vec(&record).expect("bytes"));
    let month = &record.recorded_at[..7];
    let path = root
        .join("receipts/writes")
        .join(month)
        .join(format!("{survivor}.json"));
    let receipt: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).expect("receipt")).expect("json");

    assert_eq!(
        receipt["record_sha256"].as_str(),
        Some(digest.as_str()),
        "the receipt still attests to bytes the ledger no longer holds"
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// The whole operation, on a store whose vector projection is populated.
///
/// The catch-up runs for real — the delta, the relabelling, the checkpoint —
/// against a stand-in index that holds points and an embedder factory that
/// panics. A stub returning a report saying "embeddings: 0" would say that
/// whatever the code did, which is the mistake this replaces.
#[test]
fn compaction_removes_dead_points_and_settles_without_embedding() {
    let (root, config, index) = crate::vector::tests::sync::fixture("compact-populated");
    let first = read_all(&root).expect("ledger")[0].id;

    // Indexed while it is still the living head — a snapshot excludes
    // superseded records, so a point for it can only exist from before it was
    // replaced. Which is exactly the point compaction has to remove.
    let embedded = crate::vector::tests::sync::embedder(&config, None);
    crate::vector::operator::execute(&root, &config, &index, || {
        Ok::<_, crate::kernel::error::Error>(embedded)
    })
    .expect("seed the index");
    assert!(
        index.inner.lock().unwrap().points.contains_key(&first),
        "the fixture did not index the record it is about to supersede"
    );

    let survivor = add(&root, "newer", Some(first));
    let embedded = crate::vector::tests::sync::embedder(&config, None);
    crate::vector::operator::execute(&root, &config, &index, || {
        Ok::<_, crate::kernel::error::Error>(embedded)
    })
    .expect("index the survivor");
    let before = index.inner.lock().unwrap().points.clone();
    assert!(
        before.contains_key(&survivor),
        "the fixture did not index the survivor"
    );

    let dropped = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let recorder = dropped.clone();
    let forget = std::sync::Arc::new(move |condemned: &[uuid::Uuid]| {
        recorder.lock().unwrap().extend_from_slice(condemned);
        Ok(true)
    });

    let catch_up_config = config.clone();
    let catch_up_root = root.clone();
    let catch_up_index = index.clone();
    let catch_up = std::sync::Arc::new(move |_: &std::path::Path, _: &str| {
        // The real algorithm, with the model made unreachable: needing it is a
        // failure rather than a number nobody reads.
        crate::vector::operator::execute(
            &catch_up_root,
            &catch_up_config,
            &catch_up_index,
            || -> Result<crate::vector::tests::sync::FakeEmbedder, crate::kernel::error::Error> {
                panic!("compaction sent unchanged text back to the model")
            },
        )
    });

    let report = crate::vector::with_standin(catch_up, || {
        super::projections::with_index(forget, || run(&root, true, "owner"))
    })
    .expect("compaction");

    assert_eq!(report.removed, 1);
    assert_eq!(
        *dropped.lock().unwrap(),
        vec![first],
        "compaction removed the wrong points, or none"
    );

    let after = index.inner.lock().unwrap();
    let kept = after.points.get(&survivor).expect("the survivor's point");
    let was = before.get(&survivor).expect("the survivor was indexed");
    assert_eq!(
        kept.input_sha256, was.input_sha256,
        "the meaning changed, which cutting a link must not do"
    );
    assert_ne!(
        kept.record_sha256, was.record_sha256,
        "the new envelope hash never reached the point"
    );
    assert!(
        after.checkpoint.is_some(),
        "the catch-up left no checkpoint, so the next write would redo history"
    );
    drop(after);
    let _ = std::fs::remove_dir_all(&root);
}
