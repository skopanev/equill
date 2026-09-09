use super::super::super::{ContextBundle, RuntimeBudget, assemble_with_limits};
use super::super::fixtures::records::append;
use super::super::fixtures::registries::registry_unbounded;
use super::super::fixtures::support::{request, store};
use crate::filter::Filter;
use crate::kernel::error::Error;
use crate::projection::SearchRequest;
use crate::record::StoredRecord;
use crate::vector::{RejectedHit, with_semantic_half};
use std::fs;
use std::path::Path;

fn seeded(name: &str, strategy: &str) -> std::path::PathBuf {
    let root = store(name);
    registry_unbounded(&root, &[strategy], "agent.memory");
    for index in 0..40 {
        append(
            &root,
            &format!("Synthetic needle record {index:02}"),
            &[],
            None,
            "2026-01-01T00:00:00Z",
        );
    }
    if strategy == "hybrid" {
        stage_current(&root);
    }
    root
}

fn stage_current(root: &Path) {
    let config = crate::vector::tests::support::config(root);
    crate::vector::tests::support::write(root, &config);
    let (records, digest) = crate::vector::corpus(root).expect("corpus");
    let directory = root.join("projections/qdrant");
    fs::create_dir_all(&directory).expect("marker directory");
    fs::write(
        directory.join("state.json"),
        serde_json::to_vec(&serde_json::json!({
            "schema": "equill.qdrant-state.v2",
            "state": "ready",
            "store_id": config["store_id"],
            "collection_alias": config["collection_alias"],
            "physical_collection": "equill_records_test_p0",
            "model_sha256": config["embedding"]["model"]["sha256"],
            "indexed_records": records.len(),
            "indexed_sha256": digest,
        }))
        .expect("marker"),
    )
    .expect("state marker");
}

fn records(root: &Path) -> Vec<StoredRecord> {
    crate::record::read_all(root)
        .expect("records")
        .into_iter()
        .filter(|record| record.type_name == "agent.lesson.v1")
        .collect()
}

fn vector<const N: usize>(
    root: &Path,
    _request: &SearchRequest,
) -> Result<(Vec<StoredRecord>, Vec<RejectedHit>), Error> {
    Ok((records(root).into_iter().take(N).collect(), Vec::new()))
}

fn unavailable(
    _root: &Path,
    _request: &SearchRequest,
) -> Result<(Vec<StoredRecord>, Vec<RejectedHit>), Error> {
    Err(Error::Projection("synthetic vector unavailable".into()))
}

fn unexpected(
    _root: &Path,
    _request: &SearchRequest,
) -> Result<(Vec<StoredRecord>, Vec<RejectedHit>), Error> {
    panic!("text-only context asked the vector provider")
}

fn context(root: &Path, limit: usize) -> ContextBundle {
    assemble_with_limits(
        root,
        "worker.v1",
        request("needle"),
        "test-owner",
        &Filter::default(),
        RuntimeBudget {
            tokens: None,
            records: Some(limit),
        },
    )
    .expect("context")
}

fn assert_mix<const V: usize>(limit: usize, vector_count: usize, fts_count: usize) {
    let root = seeded(&format!("mixed-{limit}-{V}"), "hybrid");
    let vector_ids = records(&root)
        .into_iter()
        .take(V.min(limit))
        .map(|record| record.id)
        .collect::<Vec<_>>();
    let bundle = with_semantic_half(vector::<V>, || context(&root, limit));
    let semantic = bundle.receipt.semantic.as_ref().expect("hybrid account");

    assert_eq!(bundle.selected_record_ids.len(), limit);
    assert_eq!(
        &bundle.selected_record_ids[..vector_count],
        vector_ids.as_slice(),
        "FTS displaced or reordered a vector hit"
    );
    assert_eq!(semantic.vector_selected_records, Some(vector_count));
    assert_eq!(semantic.fts_selected_records, Some(fts_count));
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn vector_first_fill_generalizes_and_deduplicates_before_fts() {
    assert_mix::<30>(30, 30, 0);
    assert_mix::<20>(30, 20, 10);
    assert_mix::<5>(30, 5, 25);
    assert_mix::<5>(7, 5, 2);
}

#[test]
fn required_and_core_consume_slots_before_vector_first_relevant_fill() {
    let root = seeded("mixed-tiers", "hybrid");
    let required = append(
        &root,
        "Synthetic needle required",
        &["must"],
        None,
        "2026-01-01T00:00:00Z",
    );
    let core = append(
        &root,
        "Synthetic needle core",
        &["core"],
        None,
        "2026-01-01T00:00:00Z",
    );
    stage_current(&root);
    let vector_ids = records(&root)
        .into_iter()
        .take(5)
        .map(|record| record.id)
        .collect::<Vec<_>>();
    let bundle = with_semantic_half(vector::<5>, || context(&root, 7));
    let semantic = bundle.receipt.semantic.as_ref().expect("hybrid account");

    assert_eq!(bundle.selected_record_ids[0], required);
    assert_eq!(bundle.selected_record_ids[1], core);
    assert_eq!(&bundle.selected_record_ids[2..], vector_ids.as_slice());
    assert_eq!(semantic.vector_selected_records, Some(5));
    assert_eq!(semantic.fts_selected_records, Some(0));
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn unavailable_vector_falls_back_to_the_full_record_budget() {
    let root = seeded("mixed-fallback", "hybrid");
    let bundle = with_semantic_half(unavailable, || context(&root, 30));
    let semantic = bundle.receipt.semantic.as_ref().expect("fallback account");

    assert_eq!(bundle.selected_record_ids.len(), 30);
    assert_eq!(semantic.answered_by, "fts");
    assert_eq!(semantic.vector_selected_records, Some(0));
    assert_eq!(semantic.fts_selected_records, Some(30));
    assert!(semantic.fallback.is_some());
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn lagging_vector_with_record_ceiling_answers_and_reports_its_freshness() {
    let root = seeded("mixed-lagging", "hybrid");
    crate::vector::tests::support::stage_lagging_index(&root, 20, 1);
    let bundle = with_semantic_half(vector::<30>, || context(&root, 7));
    let semantic = bundle.receipt.semantic.as_ref().expect("hybrid account");

    assert_eq!(bundle.selected_record_ids.len(), 7);
    assert_eq!(semantic.answered_by, "hybrid");
    assert_eq!(semantic.vector_selected_records, Some(7));
    assert_eq!(semantic.fts_selected_records, Some(0));
    assert!(semantic.fallback.is_none());
    assert_eq!(
        semantic.vector_freshness,
        crate::vector::VectorFreshness::Lagging
    );
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn text_only_selector_preserves_existing_behavior_when_vector_is_disabled() {
    let root = seeded("mixed-text-only", "fts");
    let bundle = with_semantic_half(unexpected, || context(&root, 30));

    assert_eq!(bundle.selected_record_ids.len(), 30);
    assert!(bundle.receipt.semantic.is_none());
    let receipt = serde_json::to_string(&bundle.receipt).expect("receipt");
    assert!(!receipt.contains("vector_selected_records"));
    assert!(!receipt.contains("fts_selected_records"));
    fs::remove_dir_all(root).expect("cleanup");
}
