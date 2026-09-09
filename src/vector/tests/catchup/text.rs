use super::store;
use crate::kernel::{error::Error, lock::TryLock};
use crate::record::{RecordDraft, append, append_only};
use crate::vector::catchup::{bounds, handoff, starter, text, worker};
use serde_json::json;
use std::{cell::Cell, fs, path::Path, time::Duration};

#[test]
fn cli_full_read_succeeds_while_background_snapshot_lock_is_held() {
    let root = store("shared-read");
    append_only(&root, draft("synthetic committed record"), "owner").unwrap();
    crate::projection::catch_up_text(&root).unwrap();
    super::configure_unreachable(&root);
    crate::vector::desired::advance(&root, 1).unwrap();
    assert!(crate::vector::drain::outstanding_for_tests(&root));
    let _lock = crate::kernel::lock::StoreLock::shared(&root).unwrap();
    starter::with_starter(
        |_| panic!("read-only CLI must not start a worker"),
        || {
            let output = crate::run([
                "equill",
                "--json",
                "doctor",
                "--store",
                root.to_str().unwrap(),
                "--full",
            ])
            .expect("CLI reads alongside background capture");
            let report: serde_json::Value = serde_json::from_str(&output).unwrap();
            assert_eq!(report["ok"], true);
        },
    );
    drop(_lock);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn background_text_capture_does_not_exclude_existing_public_reader() {
    let root = store("shared-text");
    append_only(&root, draft("synthetic pending text"), "owner").unwrap();
    let _reader = crate::kernel::lock::ReadLock::acquire(&root).unwrap();
    assert_eq!(
        crate::projection::catch_up_text_background(&root).unwrap(),
        1
    );
    drop(_reader);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn internal_text_snapshot_waits_for_writer_without_losing_tail() {
    let root = store("text-snapshot-contention");
    append_only(&root, draft("synthetic tail"), "owner").unwrap();
    let lock = crate::kernel::lock::StoreLock::exclusive(&root).unwrap();
    let reader_root = root.clone();
    let (started_tx, started_rx) = std::sync::mpsc::channel();
    let (done_tx, done_rx) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        started_tx.send(()).unwrap();
        done_tx
            .send(crate::projection::catch_up_text_background(&reader_root))
            .unwrap();
    });
    started_rx.recv().unwrap();
    let early = done_rx.recv_timeout(Duration::from_millis(150));
    drop(lock);
    let waited = matches!(early, Err(std::sync::mpsc::RecvTimeoutError::Timeout));
    let result = match early {
        Ok(result) => result,
        Err(_) => done_rx.recv().unwrap(),
    };
    worker.join().unwrap();
    eprintln!("text contention: waited={waited}, result={result:?}");
    fs::remove_dir_all(root).unwrap();
    assert!(
        waited,
        "internal text drain incorrectly treated writer contention as failure"
    );
    assert_eq!(result.unwrap(), 1);
}

thread_local! { static PASSES: Cell<usize> = const { Cell::new(0) }; }

fn draft(rule: &str) -> RecordDraft {
    serde_json::from_value(json!({"namespace":"agent.memory","type":"agent.lesson.v1",
        "observed_at":"2026-01-01T00:00:00Z","payload":{"rule":rule}}))
    .unwrap()
}

fn must_not_spawn(_: &Path) -> Result<(), Error> {
    panic!("the existing text drain must own the late write");
}

fn append_after_first_pass(root: &Path) {
    let count = PASSES.with(|slot| {
        let count = slot.get();
        slot.set(count + 1);
        count
    });
    if count == 0 {
        assert_eq!(
            crate::projection::watermark(root).unwrap().indexed_records,
            1
        );
        let report = append(root, draft("synthetic late tail"), "owner").unwrap();
        assert!(!report.vector.spawned);
    }
}

#[test]
fn a_disabled_vector_drain_covers_a_write_after_its_first_text_pass() {
    let root = store("text-only-tail");
    append_only(&root, draft("synthetic first"), "owner").unwrap();
    assert!(handoff::claim(&root).unwrap().is_some());
    PASSES.with(|slot| slot.set(0));
    let report = starter::with_starter(must_not_spawn, || {
        text::with_after_pass(append_after_first_pass, || {
            worker::run_worker(&root).unwrap()
        })
    });
    assert!(report.attempt_error.is_none());
    assert_eq!(PASSES.with(Cell::get), 2);
    let covered = crate::projection::watermark(&root).unwrap();
    let target = crate::projection::target(&root).unwrap();
    assert_eq!(
        (covered.indexed_records, covered.ledger_bytes),
        (2, target.ledger_bytes)
    );
    assert_eq!(
        crate::projection::verify(&root, &crate::record::read_all(&root).unwrap()).unwrap(),
        2
    );
    assert!(
        TryLock::acquire(&root, "vector-drain.lock")
            .unwrap()
            .is_some()
    );
    assert!(!root.join("projections/qdrant/handoff-active.json").exists());
    assert!(!root.join("projections/qdrant/handoff.json").exists());
    assert!(!root.join("projections/qdrant/desired.json").exists());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn the_text_only_drain_is_finite_and_reports_an_unfinished_tail() {
    let root = store("text-only-bound");
    append_only(&root, draft("synthetic first"), "owner").unwrap();
    assert!(handoff::claim(&root).unwrap().is_some());
    PASSES.with(|slot| slot.set(0));
    let report = bounds::with_bounds(1, Duration::from_secs(30), || {
        starter::with_starter(must_not_spawn, || {
            text::with_after_pass(append_after_first_pass, || {
                worker::run_worker(&root).unwrap()
            })
        })
    });
    assert!(report.attempt_error.unwrap().contains("bound"));
    assert_eq!(PASSES.with(Cell::get), 1);
    assert_eq!(
        crate::projection::watermark(&root).unwrap().indexed_records,
        1
    );
    let outcome: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join("projections/qdrant/last-drain.json")).unwrap())
            .unwrap();
    assert_eq!(outcome["outcome"], "failed");
    assert!(worker::run_once(&root).attempt_error.is_none());
    assert_eq!(
        crate::projection::watermark(&root).unwrap().indexed_records,
        2
    );
    let outcome: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join("projections/qdrant/last-drain.json")).unwrap())
            .unwrap();
    assert_eq!(outcome["outcome"], "converged");
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_successful_rebuild_publishes_the_text_position_it_actually_built() {
    let root = store("rebuild-text-position");
    append_only(&root, draft("synthetic first"), "owner").unwrap();
    crate::projection::catch_up_text(&root).unwrap();
    append_only(&root, draft("synthetic second"), "owner").unwrap();
    assert_eq!(
        crate::projection::watermark(&root).unwrap().indexed_records,
        1
    );
    // A removed ledger history can leave an older target ahead of actual truth.
    crate::projection::publish_target(&root, 3, 999).unwrap();
    crate::projection::rebuild(&root).unwrap();
    let covered = crate::projection::watermark(&root).unwrap();
    let target = crate::projection::target(&root).unwrap();
    assert_eq!(target.records, 2);
    assert_eq!(
        (covered.indexed_records, covered.ledger_bytes),
        (2, target.ledger_bytes)
    );
    fs::remove_dir_all(root).unwrap();
}
