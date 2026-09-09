//! What compaction leaves behind in the vector projection.
use super::journal::with_interrupt;
use super::plan_tests::{add, store};
use super::run::{run, with_pause};
use crate::record::read_all;
use std::time::Duration;

/// The whole operation, on a store whose vector projection is populated. The
/// catch-up runs for real against a stand-in index holding points and vectors,
/// with an embedder factory that panics — a stub reporting "embeddings: 0"
/// would say so whatever the code did.
#[test]
fn compaction_removes_dead_points_and_settles_without_embedding() {
    let (root, config, index) = crate::vector::tests::sync::fixture("compact-populated");
    let first = read_all(&root).expect("ledger")[0].id;

    // Indexed while still the living head: a snapshot excludes superseded ones.
    let embedded = crate::vector::tests::sync::embedder(&config, None);
    crate::vector::operator::execute(&root, &config, &index, || {
        Ok::<_, crate::kernel::error::Error>(embedded)
    })
    .expect("seed the index");
    let stale = {
        let state = index.inner.lock().unwrap();
        (
            state.points.get(&first).cloned().expect("the seeded point"),
            state.vectors.get(&first).cloned().expect("its vector"),
        )
    };

    let survivor = add(&root, "newer", Some(first));
    let embedded = crate::vector::tests::sync::embedder(&config, None);
    crate::vector::operator::execute(&root, &config, &index, || {
        Ok::<_, crate::kernel::error::Error>(embedded)
    })
    .expect("index the survivor");

    // The second pass dropped that point as history. Put it back: without a
    // stale point the removal below removes nothing.
    {
        let mut state = index.inner.lock().unwrap();
        state.points.insert(first, stale.0.clone());
        state.vectors.insert(first, stale.1.clone());
    }
    let before = index.inner.lock().unwrap().points.clone();
    let vectors_before = index.inner.lock().unwrap().vectors.clone();
    assert!(
        before.contains_key(&first),
        "the fixture has no stale point to remove"
    );

    let removing = index.clone();
    let forget = std::sync::Arc::new(move |condemned: &[uuid::Uuid]| {
        use crate::vector::operator::SyncIndex as _;
        removing.delete("physical", condemned)?;
        Ok(true)
    });

    let catch_up_config = config.clone();
    let catch_up_root = root.clone();
    let catch_up_index = index.clone();
    let catch_up = std::sync::Arc::new(move |_: &std::path::Path, _: &str| {
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

    let after = index.inner.lock().unwrap();
    assert!(
        !after.points.contains_key(&first) && !after.vectors.contains_key(&first),
        "the stale point survived the compaction"
    );

    let kept = after.points.get(&survivor).expect("the survivor's point");
    let was = before.get(&survivor).expect("the survivor was indexed");
    let record = read_all(&root)
        .expect("ledger")
        .into_iter()
        .find(|item| item.id == survivor)
        .expect("survivor");
    assert_eq!(
        kept.record_sha256,
        crate::kernel::digest::sha256_hex(&serde_json::to_vec(&record).expect("bytes")),
        "the point does not carry the hash of the bytes now in the ledger"
    );
    assert_eq!(kept.input_sha256, was.input_sha256, "the meaning changed");
    assert_eq!(kept.model_sha256, was.model_sha256, "the model changed");
    assert_eq!(
        after.vectors.get(&survivor),
        vectors_before.get(&survivor),
        "the vector was recomputed for a record whose text never changed"
    );
    let (records, digest) = after.checkpoint.clone().expect("a checkpoint");
    assert_eq!(
        records, 1,
        "the checkpoint counts records that are not there"
    );
    assert!(!digest.is_empty());
    drop(after);

    // The store carries on: an ordinary write is embedded, and the record
    // compaction left alone is not embedded again.
    let written = add(&root, "written after compaction", None);
    let embedded = crate::vector::tests::sync::embedder(&config, None);
    let caught_up = crate::vector::operator::execute(&root, &config, &index, || {
        Ok::<_, crate::kernel::error::Error>(embedded)
    })
    .expect("catch up after the write");

    assert_eq!(
        caught_up.embeddings, 1,
        "the write after compaction re-embedded history instead of only itself"
    );
    let settled = index.inner.lock().unwrap();
    assert!(
        settled.points.contains_key(&written),
        "the write after compaction never reached the index"
    );
    assert_eq!(
        settled.vectors.get(&survivor),
        vectors_before.get(&survivor),
        "the survivor was re-embedded by the next write"
    );
    drop(settled);
    let _ = std::fs::remove_dir_all(&root);
}

/// A catch-up that could not start is not one that finished: with the lease
/// held elsewhere the worker returns a default report — nothing done, no error
/// — and reading that as success let compaction clear its journal.
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

/// A record appended between the crash and the recovery is not carried away.
///
/// The write lands in the directory that is still current; publishing the
/// prepared copy over it would move that directory aside as a backup, with the
/// new record inside. Recovery refuses instead of guessing.
#[test]
fn an_append_after_the_crash_is_not_lost_to_the_prepared_copy() {
    let root = store("crash-append");
    let first = add(&root, "older", None);
    add(&root, "newer", Some(first));

    let interrupted = with_interrupt("before-records", || run(&root, true, "owner"));
    assert!(interrupted.is_err(), "the fixture did not interrupt");

    // The window: nothing is published yet, and the store is writable again.
    let landed = add(&root, "written between crash and recovery", None);

    let resumed = run(&root, true, "owner");
    assert!(
        resumed.is_err(),
        "recovery published a stale copy over a ledger that had moved on"
    );
    assert!(
        read_all(&root)
            .expect("ledger")
            .iter()
            .any(|record| record.id == landed),
        "a record written after the crash was lost"
    );
    let _ = std::fs::remove_dir_all(&root);
}
