use crate::ingest::import_jsonl;
use crate::record::{hotpath, read_all, tests::store};
use serde_json::json;
use std::fs;
use std::path::Path;

fn source(root: &Path, count: usize) -> std::path::PathBuf {
    let input = root.join("synthetic-import.jsonl");
    let lines = (0..count)
        .map(|index| line(index, json!({"rule":format!("synthetic record {index}")})))
        .collect::<String>();
    fs::write(&input, lines).unwrap();
    input
}

fn line(index: usize, payload: serde_json::Value) -> String {
    format!(
        "{}\n",
        json!({"id":format!("legacy-{index}"), "ts":"2026-01-01T00:00:00Z",
        "namespace":"agent.memory", "type":"agent.lesson.v1", "actor":"legacy-writer",
        "observed_at":"2026-01-01T00:00:00Z", "payload":payload })
    )
}

#[test]
fn a_batch_reads_truth_once_syncs_once_and_uses_one_sql_transaction() {
    let root = store();
    let input = source(&root, 20);
    hotpath::reset();
    let report = import_jsonl(&root, &input, "writer").unwrap();
    assert_eq!(report.imported, 20);
    assert_eq!(hotpath::touched().ledger_reads, 1);
    assert_eq!(hotpath::write_counts(), (1, 1));
    let records = read_all(&root).unwrap();
    assert_eq!(crate::projection::verify(&root, &records).unwrap(), 20);
    let month = &records[0].recorded_at[..7];
    assert_eq!(
        fs::read_dir(root.join(format!("receipts/writes/{month}")))
            .unwrap()
            .count(),
        20
    );
    assert!(!root.join("transactions/batch.json").exists());
    assert_eq!(
        fs::read_dir(root.join("transactions/batches"))
            .unwrap()
            .count(),
        0
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn invalid_final_line_leaves_no_batch_artifacts() {
    let root = store();
    let input = source(&root, 3);
    use std::io::Write;
    fs::OpenOptions::new()
        .append(true)
        .open(&input)
        .unwrap()
        .write_all(line(3, json!({"rule":42})).as_bytes())
        .unwrap();
    let error = import_jsonl(&root, &input, "writer").unwrap_err();
    assert!(error.to_string().contains("line 4:"));
    assert!(read_all(&root).unwrap().is_empty());
    assert!(!root.join("transactions").exists());
    assert!(!root.join("receipts/pending").exists());
    assert!(!root.join("receipts/writes").exists());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn every_batch_boundary_recovers_zero_or_all_without_duplicates() {
    use super::super::seam::{self, Step};
    for step in [
        Step::BeforeAppend,
        Step::PartialAppend,
        Step::AfterAppend,
        Step::AfterReceipt(0),
        Step::AfterReceipts,
        Step::BeforeProjection,
        Step::AfterBatchDataRemoval,
    ] {
        let root = store();
        let input = source(&root, 4);
        seam::fail(Some(step));
        assert!(import_jsonl(&root, &input, "writer").is_err());
        seam::fail(None);
        let retry = import_jsonl(&root, &input, "writer").unwrap();
        assert_eq!(retry.imported + retry.skipped, 4);
        let records = read_all(&root).unwrap();
        assert_eq!(records.len(), 4, "{step:?}");
        assert_eq!(crate::projection::verify(&root, &records).unwrap(), 4);
        let again = import_jsonl(&root, &input, "writer").unwrap();
        assert_eq!((again.imported, again.skipped), (0, 4));
        assert!(!root.join("transactions/batch.json").exists());
        assert_eq!(
            fs::read_dir(root.join("transactions/batches"))
                .unwrap()
                .count(),
            0
        );
        fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn abort_receipt_cleanup_can_be_interrupted_and_repeated() {
    use super::super::seam::{self, Step};
    let root = store();
    let input = source(&root, 3);
    seam::fail(Some(Step::PartialAppend));
    assert!(import_jsonl(&root, &input, "writer").is_err());
    seam::fail(Some(Step::AfterAbortReceiptRemoval));
    assert!(import_jsonl(&root, &input, "writer").is_err());
    assert!(root.join("transactions/batch.json").exists());
    seam::fail(Some(Step::AfterBatchDataRemoval));
    assert!(import_jsonl(&root, &input, "writer").is_err());
    seam::fail(None);
    let report = import_jsonl(&root, &input, "writer").unwrap();
    assert_eq!(report.imported, 3);
    assert_eq!(read_all(&root).unwrap().len(), 3);
    assert!(!root.join("transactions/batch.json").exists());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_tampered_stage_cannot_authorize_partial_tail_rollback() {
    use super::super::seam::{self, Step};
    let root = store();
    let input = source(&root, 2);
    seam::fail(Some(Step::PartialAppend));
    assert!(import_jsonl(&root, &input, "writer").is_err());
    seam::fail(None);
    let ledger = fs::read_dir(root.join("records"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let original = fs::read(&ledger).unwrap();
    let stage = fs::read_dir(root.join("transactions/batches"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    fs::write(stage, b"synthetic damaged transaction").unwrap();
    assert!(import_jsonl(&root, &input, "writer").is_err());
    assert_eq!(fs::read(ledger).unwrap(), original);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_foreign_tail_is_never_truncated_by_batch_recovery() {
    use super::super::seam::{self, Step};
    let root = store();
    let input = source(&root, 2);
    seam::fail(Some(Step::PartialAppend));
    assert!(import_jsonl(&root, &input, "writer").is_err());
    seam::fail(None);
    let ledger = fs::read_dir(root.join("records"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let bytes = b"foreign bytes are not owned by the batch";
    fs::write(&ledger, bytes).unwrap();
    assert!(import_jsonl(&root, &input, "writer").is_err());
    assert_eq!(fs::read(ledger).unwrap(), bytes);
    assert!(root.join("transactions/batch.json").is_file());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn an_interrupted_batch_prefix_must_not_survive_retry_in_text_search() {
    use super::super::seam::{self, Step};
    let root = store();
    let input = source(&root, 4);
    seam::fail(Some(Step::PartialAppend));
    assert!(import_jsonl(&root, &input, "writer").is_err());
    seam::fail(None);
    assert!(root.join("transactions/batch.json").is_file());
    assert!(
        read_all(&root)
            .unwrap_err()
            .to_string()
            .contains("recovery pending")
    );
    assert!(
        crate::projection::catch_up_text(&root)
            .unwrap_err()
            .to_string()
            .contains("recovery pending")
    );
    let request = crate::projection::SearchRequest {
        query: Some("synthetic".into()),
        namespace: None,
        type_name: None,
        limit: 20,
    };
    let before = crate::projection::search(&root, &request).unwrap();
    assert!(
        before.hits.is_empty(),
        "no transaction prefix may become searchable"
    );
    assert_eq!(import_jsonl(&root, &input, "writer").unwrap().imported, 4);
    let truth = read_all(&root).unwrap();
    let after = crate::projection::search(&root, &request).unwrap();
    let orphaned = after
        .hits
        .iter()
        .filter(|hit| !truth.iter().any(|record| record.id == hit.record.id))
        .count();
    eprintln!(
        "atomic visibility: truth={}, search={}, orphaned={orphaned}",
        truth.len(),
        after.hits.len()
    );
    assert_eq!(
        orphaned, 0,
        "aborted transaction records survived in search"
    );
    assert_eq!(crate::projection::verify(&root, &truth).unwrap(), 4);
    fs::remove_dir_all(root).unwrap();
}
