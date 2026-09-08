//! A record whose envelope changed but whose meaning did not.
use super::{FakeEmbedder, embedder, fixture};
use crate::kernel::error::Error;
use crate::vector::operator::execute_with_progress;
use std::fs;

/// Compaction cuts a `supersedes` link, so the record's hash moves. Its
/// embedding input does not: the input carries namespace, type, tags and
/// payload — provenance is deliberately excluded, precisely so that rewriting
/// how a record was stored does not churn what it means.
///
/// Proven the strongest way this suite has: the embedder factory panics, so a
/// second pass that needed the model fails the test outright rather than
/// reporting a count somebody has to notice.
#[test]
fn cutting_a_link_relabels_the_point_without_loading_the_model() {
    let (root, config, index) = fixture("relabel");
    let first = execute_with_progress(&root, &config, &index, || Ok(embedder(&config, None)), None)
        .expect("first sync");
    assert_eq!(first.embeddings, 1, "the fixture embedded nothing to reuse");
    let before = index.inner.lock().unwrap().points.clone();
    let relabels_before = index.inner.lock().unwrap().points_relabelled;

    // The envelope as compaction leaves it. `actor` is provenance and is not
    // part of the embedding input, so this is the same record to the model and
    // a different record to the ledger.
    let ledger = fs::read_dir(root.join("records"))
        .expect("records")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .next()
        .expect("one ledger");
    let rewritten = fs::read_to_string(&ledger)
        .expect("read")
        .lines()
        .map(|line| {
            let mut record: serde_json::Value = serde_json::from_str(line).expect("record");
            record["actor"] = serde_json::json!("compactor");
            record.to_string()
        })
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(&ledger, format!("{rewritten}\n")).expect("write");

    let second = execute_with_progress(
        &root,
        &config,
        &index,
        || -> Result<FakeEmbedder, Error> {
            panic!("a changed envelope sent unchanged text back to the model")
        },
        None,
    )
    .expect("second sync");

    assert_eq!(second.embeddings, 0, "the model was asked for nothing new");
    let after = index.inner.lock().unwrap();
    assert!(
        after.points_relabelled > relabels_before,
        "the point kept a stale record hash instead of being relabelled"
    );
    for (id, point) in &before {
        let now = after.points.get(id).expect("the point survived");
        assert_eq!(
            now.input_sha256, point.input_sha256,
            "the meaning changed, which this fixture was built to avoid"
        );
        assert_ne!(
            now.record_sha256, point.record_sha256,
            "the new envelope hash never reached the point"
        );
    }
    drop(after);
    let _ = fs::remove_dir_all(&root);
}
