//! What a sync does while the store keeps being written to.
use super::{embedder, fixture};
use crate::vector::operator::execute;
use crate::vector::{VectorState, corpus, state};
use std::fs;

#[test]
fn internal_sync_snapshot_waits_for_writer_without_provider_failure() {
    let (root, config, index) = fixture("snapshot-contention");
    super::add(&root, "coalesced tail");
    let lock = crate::kernel::lock::StoreLock::exclusive(&root).unwrap();
    let reader_root = root.clone();
    let (started_tx, started_rx) = std::sync::mpsc::channel();
    let (done_tx, done_rx) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        started_tx.send(()).unwrap();
        done_tx
            .send(execute(&reader_root, &config, &index, || {
                Ok(embedder(&config, None))
            }))
            .unwrap();
    });
    started_rx.recv().unwrap();
    let early = done_rx.recv_timeout(std::time::Duration::from_millis(150));
    drop(lock);
    let waited = matches!(early, Err(std::sync::mpsc::RecvTimeoutError::Timeout));
    let result = match early {
        Ok(result) => result,
        Err(_) => done_rx.recv().unwrap(),
    };
    worker.join().unwrap();
    eprintln!("vector contention: waited={waited}, result={result:?}");
    fs::remove_dir_all(root).unwrap();
    assert!(
        waited,
        "internal sync incorrectly treated writer contention as provider failure"
    );
    assert_eq!(result.unwrap().records, 2);
}

/// Writing never stops in a live store, so a sync that demanded a still ledger
/// would never finish. It processes the snapshot it captured and succeeds; what
/// arrived meanwhile is the next call's tail, and the checkpoint says so.
#[test]
fn an_append_during_sync_does_not_fail_it() {
    let (root, config, index) = fixture("concurrent");
    let before = corpus(&root).unwrap().0.len();

    let report = execute(&root, &config, &index, || {
        Ok(embedder(&config, Some(root.clone())))
    })
    .expect("a concurrent append must not fail the sync");

    // The record appended mid-run is in the ledger but outside this snapshot.
    let after = corpus(&root).unwrap().0.len();
    assert_eq!(after, before + 1);
    assert_eq!(report.records, before);
    let checkpoint = index.inner.lock().unwrap().checkpoint.clone();
    let (indexed, digest) = checkpoint.expect("the pass records what it covered");
    assert_eq!(
        indexed, before,
        "the watermark never jumps past the boundary"
    );
    assert_eq!(digest, report.corpus_sha256);
    // Health is unaffected throughout: an append during the pass never demotes
    // the index, so semantic search stays available while the model runs.
    assert_eq!(state(&root).unwrap(), VectorState::Ready);
    let after = crate::vector::freshness_of(&root).expect("freshness");
    assert_eq!(after.freshness, crate::vector::VectorFreshness::Lagging);
    assert_eq!(after.pending_records, None);
    fs::remove_dir_all(root).unwrap();
}
