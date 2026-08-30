//! What a caller is told about how far behind the index is.
use super::{add, embedder, fixture};
use crate::vector::operator::execute;
use crate::vector::{VectorState, corpus, state};
use serde_json::json;
use std::fs;
/// A store that is being written to is always a little behind. That is not a
/// fault, and it must not take semantic search offline — the answer simply
/// comes from the checkpoint, and the report says so.
#[test]
fn a_lagging_index_is_healthy_and_still_answers() {
    let (root, config, index) = fixture("lagging");
    execute(&root, &config, &index, || Ok(embedder(&config, None))).expect("first sync");
    let current = crate::vector::freshness_of(&root).expect("freshness");

    // One more record arrives; nothing re-syncs.
    add(&root, "a later thought");
    let lagging = crate::vector::freshness_of(&root).expect("freshness");

    assert_eq!(current.freshness, crate::vector::VectorFreshness::Current);
    assert_eq!(current.pending_records, Some(0));
    assert_eq!(lagging.freshness, crate::vector::VectorFreshness::Lagging);
    assert_eq!(lagging.pending_records, Some(1));
    // Health is untouched by the lag: the index is behind, not broken.
    assert_eq!(state(&root).unwrap(), VectorState::Ready);
    fs::remove_dir_all(root).unwrap();
}

/// A marker written by an older build is a checkpoint, not a fault. Refusing it
/// would take a working index offline for a schema change it never made; the
/// honest answer is that its snapshot is unknown until the next sync records one.
#[test]
fn a_v1_marker_stays_searchable_with_unknown_freshness() {
    let (root, config, index) = fixture("v1-marker");
    execute(&root, &config, &index, || Ok(embedder(&config, None))).expect("sync");
    let marker = root.join("projections/qdrant/state.json");
    let mut stored: serde_json::Value =
        serde_json::from_slice(&fs::read(&marker).unwrap()).unwrap();
    let object = stored.as_object_mut().unwrap();
    object.insert("schema".into(), json!("equill.qdrant-state.v1"));
    object.remove("indexed_records");
    object.remove("indexed_sha256");
    fs::write(&marker, serde_json::to_vec(&stored).unwrap()).unwrap();

    let reading = crate::vector::freshness_of(&root).expect("freshness");

    assert_eq!(reading.freshness, crate::vector::VectorFreshness::Unknown);
    assert_eq!(reading.pending_records, None);
    // Still searchable: an old checkpoint is behind on bookkeeping, not broken.
    assert_eq!(state(&root).unwrap(), VectorState::Ready);
    // The next sync writes v2 and freshness becomes answerable again.
    execute(&root, &config, &index, || Ok(embedder(&config, None))).expect("second sync");
    let after = crate::vector::freshness_of(&root).expect("freshness");
    assert_eq!(after.freshness, crate::vector::VectorFreshness::Current);
    fs::remove_dir_all(root).unwrap();
}

/// The point of a checkpoint is that it survives a bad day. A pass that fails
/// partway must leave the previous snapshot answering — otherwise one flaky
/// upsert costs a working index, and the store is worse off for having tried.
#[test]
fn a_failure_after_a_good_snapshot_leaves_the_good_one_serving() {
    let (root, config, index) = fixture("checkpoint");
    execute(&root, &config, &index, || Ok(embedder(&config, None))).expect("first sync");
    let good = crate::vector::freshness_of(&root).expect("freshness");

    // A second record arrives and its sync fails at the upsert.
    add(&root, "a thought that will not index");
    index.inner.lock().unwrap().fail_upsert = true;
    let failed = execute(&root, &config, &index, || Ok(embedder(&config, None)));
    let after = crate::vector::freshness_of(&root).expect("freshness");

    assert!(failed.is_err(), "the caller is told the pass failed");
    // Health is untouched, so strict vector still answers from snapshot A.
    assert_eq!(state(&root).unwrap(), VectorState::Ready);
    assert_eq!(good.freshness, crate::vector::VectorFreshness::Current);
    assert_eq!(after.freshness, crate::vector::VectorFreshness::Lagging);
    // Pending is honest rather than falsely zero.
    assert_eq!(after.pending_records, Some(1));
    assert_eq!(after.indexed_records, good.indexed_records);
    fs::remove_dir_all(root).unwrap();
}

/// A half-written checkpoint is not a smaller number, it is no answer: a count
/// whose snapshot is missing would let a reader believe a freshness that was
/// never recorded.
#[test]
fn an_incomplete_or_foreign_checkpoint_is_refused_rather_than_believed() {
    let (root, config, index) = fixture("checkpoint-shape");
    execute(&root, &config, &index, || Ok(embedder(&config, None))).expect("sync");
    let marker = root.join("projections/qdrant/state.json");
    let rewrite = |mutate: &dyn Fn(&mut serde_json::Map<String, serde_json::Value>)| {
        let mut stored: serde_json::Value =
            serde_json::from_slice(&fs::read(&marker).unwrap()).unwrap();
        mutate(stored.as_object_mut().unwrap());
        fs::write(&marker, serde_json::to_vec(&stored).unwrap()).unwrap();
    };

    rewrite(&|object| {
        object.remove("indexed_sha256");
    });
    let half = crate::vector::freshness_of(&root);

    rewrite(&|object| {
        object.insert("indexed_sha256".into(), json!("not-a-digest"));
    });
    let malformed = crate::vector::freshness_of(&root);

    rewrite(&|object| {
        object.insert("indexed_sha256".into(), json!("a".repeat(64)));
        object.insert("collection_alias".into(), json!("someone_elses_alias"));
    });
    let foreign = crate::vector::freshness_of(&root).expect("a foreign marker is not an error");

    assert!(half.is_err(), "a count without its snapshot is refused");
    assert!(malformed.is_err(), "a digest that is not one is refused");
    // A marker describing another alias answers nothing rather than lying.
    assert_eq!(foreign.freshness, crate::vector::VectorFreshness::Unknown);
    assert_eq!(foreign.pending_records, None);
    fs::remove_dir_all(root).unwrap();
}

/// The sync path over an empty index does the same work a bootstrap does, and
/// this covers that shape: it indexes the snapshot it captured and checkpoints
/// exactly that boundary, lagging from the moment it finished. The rebuild
/// operator itself is covered end to end by the endpoint-gated test, which runs
/// it against a live provider.
#[test]
fn a_first_pass_checkpoints_its_own_snapshot_despite_a_concurrent_append() {
    let (root, config, index) = fixture("rebuild-race");
    let captured = corpus(&root).unwrap().0.len();

    let report = execute(&root, &config, &index, || {
        Ok(embedder(&config, Some(root.clone())))
    })
    .expect("a concurrent append must not prevent activation");

    assert_eq!(report.records, captured);
    assert_eq!(corpus(&root).unwrap().0.len(), captured + 1);
    let reading = crate::vector::freshness_of(&root).expect("freshness");
    // Lagging from the moment it finished, which is the honest description.
    assert_eq!(reading.freshness, crate::vector::VectorFreshness::Lagging);
    assert_eq!(reading.indexed_records, Some(captured));
    assert_eq!(reading.pending_records, Some(1));
    assert_eq!(state(&root).unwrap(), VectorState::Ready);
    fs::remove_dir_all(root).unwrap();
}
