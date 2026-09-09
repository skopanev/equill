use super::{embedder, fixture};
use crate::vector::{VectorFreshness, operator::execute};
use std::fs;

#[test]
fn a_revision_without_new_documents_reports_unknown_pending_without_scanning() {
    let (root, config, index) = fixture("revision-not-record-count");
    execute(&root, &config, &index, || Ok(embedder(&config, None))).unwrap();
    let current = crate::vector::freshness_of(&root).unwrap();
    assert_eq!(current.freshness, VectorFreshness::Current);
    assert_eq!(current.pending_records, Some(0));

    // Configuration invalidation advances even with no appended records.
    // Changes excluded by embedding filters also need not add documents.
    crate::vector::desired::advance(&root, 0).unwrap();
    fs::remove_dir_all(root.join("records")).unwrap();
    crate::record::hotpath::reset();
    let lagging = crate::vector::freshness_of(&root).unwrap();
    assert_eq!(lagging.freshness, VectorFreshness::Lagging);
    assert_eq!(lagging.pending_records, None);
    assert_eq!(lagging.indexed_records, current.indexed_records);
    assert_eq!(crate::record::hotpath::touched().ledger_reads, 0);
    let wire = serde_json::to_value(&lagging).unwrap();
    assert!(wire.get("pending_records").is_none());
    fs::remove_dir_all(root).unwrap();
}
