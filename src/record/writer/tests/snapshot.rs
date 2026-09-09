use super::super::seam::{self, Step};
use crate::kernel::{error::Error, lock::StoreLock};
use crate::record::{self, append_only, append_request, read_all, tests::store};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

fn files(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn visit(root: &Path, result: &mut BTreeMap<PathBuf, Vec<u8>>) {
        for entry in fs::read_dir(root).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                visit(&path, result);
            } else {
                result.insert(path.clone(), fs::read(path).unwrap());
            }
        }
    }
    let mut result = BTreeMap::new();
    visit(root, &mut result);
    result
}

fn locator(root: &Path) -> crate::projection::LedgerLocator {
    let record = read_all(root).unwrap().remove(0);
    crate::projection::LedgerLocator {
        record_id: record.id,
        ledger: format!("records/{}.jsonl", &record.recorded_at[..7]),
        record_sha256: crate::kernel::digest::sha256_hex(&serde_json::to_vec(&record).unwrap()),
    }
}

#[test]
fn committed_reads_never_create_files_and_busy_reads_do_not_wait_for_writers() {
    let root = store();
    // A store initialized before its first append can have no writer lock.
    let lock_path = root.join("locks/writer.lock");
    if lock_path.exists() {
        fs::remove_file(&lock_path).unwrap();
    }
    let before = files(&root);
    assert!(read_all(&root).unwrap().is_empty());
    assert!(!lock_path.exists());
    assert_eq!(files(&root), before);
    append_only(&root, super::draft("synthetic prior"), "writer").unwrap();
    let coordinate = locator(&root);
    let lock = StoreLock::exclusive(&root).unwrap();
    let before = files(&root);
    let (send, receive) = std::sync::mpsc::channel();
    let reader_root = root.clone();
    let reader = std::thread::spawn(move || {
        let errors = [
            read_all(&reader_root).unwrap_err(),
            record::read_located(&reader_root, &[coordinate]).unwrap_err(),
            crate::projection::catch_up_text(&reader_root).unwrap_err(),
        ];
        send.send(errors.into_iter().all(|error| {
            matches!(error,
            Error::Io(error) if error.kind() == std::io::ErrorKind::WouldBlock)
        }))
        .unwrap();
    });
    let result = receive.recv_timeout(Duration::from_secs(2));
    drop(lock);
    reader.join().unwrap();
    assert!(
        result.unwrap(),
        "read must answer before writer releases lock"
    );
    assert_eq!(files(&root), before);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn unresolved_partial_full_and_corrupt_journals_never_expose_rows() {
    for step in [
        Step::BeforeAppend,
        Step::PartialAppend,
        Step::AfterAppend,
        Step::AfterReceipts,
    ] {
        let root = store();
        append_only(&root, super::draft("synthetic prior"), "writer").unwrap();
        let coordinate = locator(&root);
        seam::fail(Some(step));
        assert!(
            append_request(
                &root,
                super::request(Some("pending"), "synthetic pending"),
                "writer"
            )
            .is_err()
        );
        seam::fail(None);
        for corrupt in [false, true] {
            if corrupt {
                fs::write(root.join("transactions/batch.json"), b"invalid journal").unwrap();
            }
            let before = files(&root);
            assert!(
                read_all(&root)
                    .unwrap_err()
                    .to_string()
                    .contains("recovery pending")
            );
            assert!(
                record::read_located(&root, std::slice::from_ref(&coordinate))
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
            assert!(
                crate::vector::corpus(&root)
                    .unwrap_err()
                    .to_string()
                    .contains("recovery pending")
            );
            assert!(
                crate::defense::audit(&root)
                    .unwrap_err()
                    .to_string()
                    .contains("recovery pending")
            );
            assert_eq!(
                files(&root),
                before,
                "read changed unresolved journal/store"
            );
        }
        fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn a_captured_reader_cannot_grow_into_a_later_aborted_transaction() {
    let root = store();
    let prior = append_only(&root, super::draft("synthetic stable"), "writer").unwrap();
    let captured = record::snapshot::capture(&root, None).unwrap();
    let bytes = captured.bytes;
    // Successful acquisition proves capture released before parsing/hashing.
    let lock = StoreLock::exclusive(&root).unwrap();
    drop(lock);
    seam::fail(Some(Step::PartialAppend));
    assert!(
        append_request(
            &root,
            super::request(Some("aborted"), "synthetic interrupted"),
            "writer"
        )
        .is_err()
    );
    seam::fail(None);
    let snapshot = record::read_captured(&root, captured).unwrap();
    assert_eq!(snapshot.bytes, bytes);
    assert_eq!(snapshot.records.len(), 1);
    assert_eq!(snapshot.records[0].id, prior.id);
    append_request(
        &root,
        super::request(Some("aborted"), "synthetic interrupted"),
        "writer",
    )
    .unwrap();
    crate::projection::catch_up_text(&root).unwrap();
    // A stale reader may finish after recovery. It can only publish already
    // committed records, never the discarded transaction coordinate.
    crate::projection::index_batch(&root, &snapshot.records).unwrap();
    let truth = read_all(&root).unwrap();
    assert_eq!(truth.len(), 2);
    assert_eq!(crate::projection::verify(&root, &truth).unwrap(), 2);
    fs::remove_dir_all(root).unwrap();
}

fn append_after_capture(root: &Path) {
    let free = crate::kernel::lock::TryLock::acquire(root, "writer.lock").unwrap();
    assert!(
        free.is_some(),
        "snapshot kept writer lock during parsing/index work"
    );
    drop(free);
    append_only(root, super::draft("synthetic late"), "writer").unwrap();
}

#[test]
fn text_index_uses_the_captured_position_not_a_later_append() {
    let root = store();
    append_only(&root, super::draft("synthetic first"), "writer").unwrap();
    let bytes = record::snapshot::capture(&root, None).unwrap().bytes;
    let indexed = record::snapshot::with_after_capture(append_after_capture, || {
        crate::projection::catch_up_text(&root).unwrap()
    });
    assert_eq!(indexed, 1);
    let covered = crate::projection::watermark(&root).unwrap();
    assert_eq!((covered.indexed_records, covered.ledger_bytes), (1, bytes));
    assert_eq!(crate::projection::catch_up_text(&root).unwrap(), 1);
    assert_eq!(
        crate::projection::verify(&root, &read_all(&root).unwrap()).unwrap(),
        2
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn vector_corpus_uses_captured_hashes_without_reading_the_new_tail() {
    let root = store();
    append_only(&root, super::draft("synthetic first"), "writer").unwrap();
    let before = crate::vector::corpus(&root).unwrap();
    let captured = record::snapshot::with_after_capture(append_after_capture, || {
        crate::vector::corpus(&root).unwrap()
    });
    assert_eq!(captured.0.len(), 1);
    assert_eq!(captured.1, before.1);
    assert_eq!(crate::vector::corpus(&root).unwrap().0.len(), 2);
    fs::remove_dir_all(root).unwrap();
}
