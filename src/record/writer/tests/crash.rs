use super::super::seam::{self, Step};
use crate::{ingest::import_jsonl, record::tests::store};
use serde_json::json;
use std::{fs, process::Command};

#[test]
fn transaction_cleanup_child() {
    let Ok(root) = std::env::var("EQUILL_TEST_TRANSACTION_ROOT") else {
        return;
    };
    let root = std::path::Path::new(&root);
    let step = match std::env::var("EQUILL_TEST_TRANSACTION_STEP")
        .unwrap()
        .as_str()
    {
        "abort" => Step::AfterAbortReceiptRemoval,
        _ => Step::AfterBatchDataRemoval,
    };
    seam::kill_at(step);
    let _ = import_jsonl(root, &root.join("synthetic.jsonl"), "writer");
    panic!("child did not reach its crash boundary");
}

#[test]
fn commit_boundary_child() {
    let Ok(root) = std::env::var("EQUILL_TEST_BOUNDARY_ROOT") else {
        return;
    };
    let root = std::path::Path::new(&root);
    let boundary = std::env::var("EQUILL_TEST_BOUNDARY_STEP").unwrap();
    let step = match boundary.as_str() {
        "before" => Step::BeforeAppend,
        "partial" => Step::PartialAppend,
        "sync" => Step::BeforeLedgerSync,
        "append" => Step::AfterAppend,
        "receipt" => Step::AfterReceipt(0),
        "receipts" => Step::AfterReceipts,
        "outcome" => Step::BeforeOutcome,
        "projection" => Step::BeforeProjection,
        _ => panic!("unknown synthetic boundary"),
    };
    seam::kill_at(step);
    if std::env::var("EQUILL_TEST_BOUNDARY_KIND").unwrap() == "keyed" {
        let _ = crate::record::append_request(
            root,
            super::request(Some("synthetic crash retry"), "synthetic crash fixture"),
            "writer",
        );
    } else {
        let _ = import_jsonl(root, &root.join("synthetic.jsonl"), "writer");
    }
    panic!("child did not reach its crash boundary");
}

fn crash(root: &std::path::Path, kind: &str, step: &str) {
    let output = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "record::writer::tests::crash::commit_boundary_child",
            "--nocapture",
        ])
        .env("EQUILL_TEST_BOUNDARY_ROOT", root)
        .env("EQUILL_TEST_BOUNDARY_KIND", kind)
        .env("EQUILL_TEST_BOUNDARY_STEP", step)
        .output()
        .unwrap();
    use std::os::unix::process::ExitStatusExt;
    assert_eq!(output.status.signal(), Some(libc::SIGABRT), "{kind}/{step}");
}

#[test]
fn killed_keyed_writes_replay_the_original_durable_coordinate() {
    for step in [
        "before", "partial", "sync", "append", "receipt", "receipts", "outcome",
    ] {
        let root = store();
        crash(&root, "keyed", step);
        assert!(
            crate::record::read_all(&root)
                .unwrap_err()
                .to_string()
                .contains("recovery pending")
        );
        let prior = (!matches!(step, "before" | "partial")).then(|| super::pending_id(&root));
        let request = || super::request(Some("synthetic crash retry"), "synthetic crash fixture");
        let first = crate::record::append_request(&root, request(), "writer").unwrap();
        let retry = crate::record::append_request(&root, request(), "writer").unwrap();
        if let Some(id) = prior {
            assert_eq!(first.id, id, "{step}");
        }
        assert_eq!((first.id, first.receipt), (retry.id, retry.receipt.clone()));
        assert_eq!(crate::record::read_all(&root).unwrap().len(), 1, "{step}");
        assert!(root.join(retry.receipt).is_file());
        fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn killed_batches_recover_every_durable_boundary_without_partial_results() {
    for step in [
        "before",
        "partial",
        "sync",
        "append",
        "receipt",
        "receipts",
        "projection",
    ] {
        let root = store();
        let input = root.join("synthetic.jsonl");
        let lines = (0..3).map(|i| format!("{}\n", json!({
            "id":format!("legacy-{i}"),"ts":"2026-01-01T00:00:00Z",
            "actor":"legacy-writer","namespace":"agent.memory","type":"agent.lesson.v1",
            "observed_at":"2026-01-01T00:00:00Z","payload":{"rule":"synthetic crash fixture"}
        }))).collect::<String>();
        fs::write(&input, lines).unwrap();
        crash(&root, "batch", step);
        let first = import_jsonl(&root, &input, "writer").unwrap();
        assert_eq!(first.imported + first.skipped, 3, "{step}");
        let again = import_jsonl(&root, &input, "writer").unwrap();
        assert_eq!((again.imported, again.skipped), (0, 3), "{step}");
        let records = crate::record::read_all(&root).unwrap();
        assert_eq!(records.len(), 3, "{step}");
        let month = &records[0].recorded_at[..7];
        assert_eq!(
            fs::read_dir(root.join(format!("receipts/writes/{month}")))
                .unwrap()
                .count(),
            3
        );
        assert_eq!(crate::projection::verify(&root, &records).unwrap(), 3);
        assert!(!root.join("transactions/batch.json").exists());
        fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn a_process_killed_during_commit_or_abort_cleanup_recovers() {
    for phase in ["commit", "abort"] {
        let root = store();
        let input = root.join("synthetic.jsonl");
        let lines = (0..3).map(|i| format!("{}\n", json!({
            "id":format!("legacy-{i}"),"ts":"2026-01-01T00:00:00Z",
            "actor":"legacy-writer","namespace":"agent.memory","type":"agent.lesson.v1",
            "observed_at":"2026-01-01T00:00:00Z","payload":{"rule":"synthetic crash fixture"}
        }))).collect::<String>();
        fs::write(&input, lines).unwrap();
        if phase == "abort" {
            seam::fail(Some(Step::PartialAppend));
            assert!(import_jsonl(&root, &input, "writer").is_err());
            seam::fail(None);
        }
        let output = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "record::writer::tests::crash::transaction_cleanup_child",
                "--nocapture",
            ])
            .env("EQUILL_TEST_TRANSACTION_ROOT", &root)
            .env("EQUILL_TEST_TRANSACTION_STEP", phase)
            .output()
            .unwrap();
        use std::os::unix::process::ExitStatusExt;
        assert_eq!(output.status.signal(), Some(libc::SIGABRT));
        assert!(root.join("transactions/batch.json").exists());
        let retry = import_jsonl(&root, &input, "writer").unwrap();
        assert_eq!(retry.imported + retry.skipped, 3);
        assert_eq!(crate::record::read_all(&root).unwrap().len(), 3);
        assert!(!root.join("transactions/batch.json").exists());
        fs::remove_dir_all(root).unwrap();
    }
}
