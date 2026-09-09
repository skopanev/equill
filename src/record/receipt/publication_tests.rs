use super::publication::{self, Interruption};
use crate::record::{
    AppendRequest, append, append_request, read_all,
    tests::{lesson, store},
};
use serde_json::json;
use std::{fs, path::Path, process::Command};

fn input(root: &Path) -> std::path::PathBuf {
    let path = root.join("synthetic-import.jsonl");
    fs::write(
        &path,
        format!(
            "{}\n",
            json!({"id":"legacy-one","ts":"2026-01-01T00:00:00Z",
        "actor":"legacy-writer","namespace":"agent.memory","type":"agent.lesson.v1",
        "observed_at":"2026-01-01T00:00:00Z","payload":{"rule":"synthetic imported record"}})
        ),
    )
    .unwrap();
    path
}

#[test]
fn receipt_staging_child() {
    let Ok(root) = std::env::var("EQUILL_TEST_RECEIPT_STAGE") else {
        return;
    };
    let root = Path::new(&root);
    publication::interrupt(Interruption::Kill);
    if std::env::var("EQUILL_TEST_RECEIPT_IMPORT").is_ok() {
        let _ = crate::ingest::import_jsonl(root, &root.join("synthetic-import.jsonl"), "writer");
    } else {
        let _ = append_request(
            root,
            AppendRequest {
                draft: lesson("synthetic interrupted record"),
                idempotency_key: Some("synthetic operation".into()),
            },
            "writer",
        );
    }
    panic!("child missed receipt staging crash");
}

#[test]
fn killed_partial_receipt_stages_never_poison_keyed_or_atomic_retry() {
    for atomic in [false, true] {
        let root = store();
        let original = append(&root, lesson("synthetic retained record"), "writer").unwrap();
        let original_receipt = fs::read(root.join(&original.receipt)).unwrap();
        let input = input(&root);
        let mut child = Command::new(std::env::current_exe().unwrap());
        child
            .args([
                "--exact",
                "record::receipt::publication_tests::receipt_staging_child",
                "--nocapture",
            ])
            .env("EQUILL_TEST_RECEIPT_STAGE", &root);
        if atomic {
            child.env("EQUILL_TEST_RECEIPT_IMPORT", "1");
        }
        let output = child.output().unwrap();
        use std::os::unix::process::ExitStatusExt;
        assert_eq!(output.status.signal(), Some(libc::SIGABRT));
        assert_eq!(read_all(&root).unwrap().len(), 1);
        assert!(!root.join("transactions/batch.json").exists());
        assert!(!root.join("receipts/operations/pending").exists());
        assert_eq!(
            fs::read_dir(root.join("receipts/pending")).unwrap().count(),
            1
        );
        crate::ingest::import_jsonl(&root, &input, "writer").unwrap();
        append(&root, lesson("synthetic next write"), "writer").unwrap();
        let records = read_all(&root).unwrap();
        assert_eq!(records.len(), 3);
        assert_eq!(
            fs::read(root.join(&original.receipt)).unwrap(),
            original_receipt
        );
        assert_eq!(
            fs::read_dir(root.join("receipts/pending")).unwrap().count(),
            0
        );
        assert_eq!(
            fs::read_dir(root.join(format!("receipts/writes/{}", &records[0].recorded_at[..7])))
                .unwrap()
                .count(),
            3
        );
        fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn a_staging_write_error_leaves_no_partial_pending_claim() {
    let root = store();
    publication::interrupt(Interruption::Error);
    assert!(append(&root, lesson("synthetic interrupted record"), "writer").is_err());
    publication::interrupt(Interruption::None);
    assert_eq!(
        fs::read_dir(root.join("receipts/pending")).unwrap().count(),
        0
    );
    append(&root, lesson("synthetic next write"), "writer").unwrap();
    assert_eq!(read_all(&root).unwrap().len(), 1);
    fs::remove_dir_all(root).unwrap();
}
