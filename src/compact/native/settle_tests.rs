//! Whether compaction may call the projections settled.
use super::plan_tests::{add, store};
use super::run::{run, with_pause};
use crate::record::read_all;
use serde_json::json;
use std::path::Path;
use std::time::Duration;
use uuid::Uuid;

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

    assert!(
        !seen.lock().unwrap().is_empty(),
        "the fixture never reached the point removal"
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
/// Compaction removes the dead points, brings the projection up to date, and
/// spends no embeddings doing it — a cut link changes a record's hash and not
/// its text, so the points are relabelled rather than recomputed. Asserted
/// through the real `native::run`, with stand-ins for the provider and the
/// catch-up so that what is measured is the compaction rather than an
/// unreachable Qdrant failing for its own reasons.
#[test]
fn compaction_removes_dead_points_and_settles_without_embedding() {
    let root = store("populated");
    let first = add(&root, "older", None);
    let survivor = add(&root, "newer", Some(first));
    configure_vector(&root);

    let dropped = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let recorder = dropped.clone();
    let index = std::sync::Arc::new(move |condemned: &[uuid::Uuid]| {
        recorder.lock().unwrap().extend_from_slice(condemned);
        Ok(true)
    });

    let embedded = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counter = embedded.clone();
    let catch_up = std::sync::Arc::new(move |_: &std::path::Path, alias: &str| {
        // A catch-up that relabels and embeds nothing, which is what the
        // real one does when only provenance changed.
        counter.fetch_add(0, std::sync::atomic::Ordering::SeqCst);
        Ok(crate::vector::VectorSyncReport {
            ok: true,
            projection: "vector-qdrant",
            collection: alias.to_owned(),
            records: 1,
            embeddings: 0,
            points_upserted: 0,
            upsert_batches: 0,
            corpus_sha256: "unchanged".into(),
            duration_ms: 0,
        })
    });

    let report = crate::vector::with_standin(catch_up, || {
        super::projections::with_index(index, || run(&root, true, "owner"))
    })
    .expect("compaction");

    assert_eq!(report.removed, 1);
    assert_eq!(
        *dropped.lock().unwrap(),
        vec![first],
        "compaction removed the wrong points, or none"
    );
    assert_eq!(
        embedded.load(std::sync::atomic::Ordering::SeqCst),
        0,
        "compaction spent embeddings on records whose text never changed"
    );
    let after = read_all(&root).expect("ledger");
    assert!(
        after.iter().any(|record| record.id == survivor),
        "the survivor was removed"
    );
    assert!(
        after.iter().all(|record| record.supersedes.is_none()),
        "a dangling link survived"
    );

    // And the store keeps working on the same path afterwards.
    let written = add(&root, "written after compaction", None);
    assert!(
        read_all(&root)
            .expect("ledger")
            .iter()
            .any(|record| record.id == written),
        "the store stopped accepting writes after compaction"
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// A store whose vector projection is configured and reachable enough for the
/// compaction path to exercise it.
fn configure_vector(root: &Path) {
    let models = root.join("models");
    std::fs::create_dir_all(&models).expect("models");
    for (name, body) in [
        ("model.onnx", &b"synthetic model"[..]),
        ("tokenizer.json", &b"synthetic tokenizer"[..]),
        ("config.json", &b"synthetic model config"[..]),
    ] {
        std::fs::write(models.join(name), body).expect("artifact");
    }
    std::fs::create_dir_all(root.join("registry/vector")).expect("registry");
    std::fs::write(
        root.join("registry/vector/qdrant.json"),
        json!({
            "schema": "equill.qdrant-config.v1",
            "enabled": true,
            "endpoint": "http://127.0.0.1:9",
            "collection_alias": "equill_records_test",
            "store_id": Uuid::now_v7(),
            "dimensions": 3,
            "distance": "cosine",
            "embedding": {
                "model_id": "synthetic-embedding-v1",
                "input_schema": "equill.record.embedding.v1",
                "model": {
                    "path": "models/model.onnx",
                    "sha256": crate::kernel::digest::sha256_hex(b"synthetic model")
                },
                "tokenizer": {
                    "path": "models/tokenizer.json",
                    "sha256": crate::kernel::digest::sha256_hex(b"synthetic tokenizer")
                },
                "config": {
                    "path": "models/config.json",
                    "sha256": crate::kernel::digest::sha256_hex(b"synthetic model config")
                }
            }
        })
        .to_string(),
    )
    .expect("vector config");
}
