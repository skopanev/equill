//! What a sync does while the store keeps being written to.
use super::{embedder, fixture};
use crate::vector::operator::execute;
use crate::vector::{VectorState, corpus, state};
use std::fs;

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
    assert_eq!(after.pending_records, Some(1));
    fs::remove_dir_all(root).unwrap();
}
