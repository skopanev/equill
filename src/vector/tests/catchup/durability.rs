use super::add;
use super::harness::{configured, counting_starter};
use crate::vector::catchup::starter::with_starter;
use crate::vector::{after_commit, run_once};

/// The target is the only durable statement of what the index still owes the
/// ledger. It has to survive a crash, which means the bytes must be on the
/// device and the rename must be too — not merely in the page cache.
#[test]
fn a_published_target_is_written_atomically_and_leaves_no_residue() {
    let root = configured("durable-target");
    let directory = root.join("projections/qdrant");

    with_starter(counting_starter, || after_commit(&root, 0));

    let target = super::super::super::desired::read(&root)
        .expect("read")
        .expect("published");
    assert_eq!(target.revision, 1);
    let residue = std::fs::read_dir(&directory)
        .expect("directory")
        .flatten()
        .filter(|entry| entry.file_name().to_string_lossy().starts_with(".desired-"))
        .count();
    assert_eq!(residue, 0, "staging must leave nothing behind");

    // Republishing replaces the file byte-for-byte rather than appending or
    // truncating in place.
    add(&root, "a second lesson");
    with_starter(counting_starter, || after_commit(&root, 0));
    let moved = super::super::super::desired::read(&root)
        .expect("read")
        .expect("published");
    assert_eq!(moved.revision, 2);
    assert!(moved.revision > target.revision);
}

/// A detached worker writes nowhere the caller can see, so its outcome is kept
/// on disk — with counts and an error class, never a provider message that
/// might quote a payload.
#[test]
fn a_worker_records_a_sanitized_outcome() {
    let root = configured("last-drain");
    with_starter(counting_starter, || after_commit(&root, 0));

    run_once(&root);

    let state: serde_json::Value = serde_json::from_slice(
        &std::fs::read(root.join("projections/qdrant/last-drain.json")).expect("outcome file"),
    )
    .expect("json");
    assert_eq!(state["schema"], "equill.qdrant-last-drain.v1");
    assert_eq!(state["outcome"], "failed");
    assert!(
        state["error_class"].is_string(),
        "the class is recorded: {state}"
    );
    let text = state.to_string();
    for forbidden in ["lesson", "rule", "127.0.0.1"] {
        assert!(
            !text.contains(forbidden),
            "the outcome file must not quote {forbidden}: {text}"
        );
    }
}

/// A pass that succeeds without moving the index any closer to its target.
///
/// This is the shape the bound exists for: work that keeps reporting success
/// and never converges. A failing pass stops the worker for its own reason and
/// never reaches the bound at all.
fn a_pass_that_never_converges(
    _store: &std::path::Path,
) -> Result<crate::vector::VectorSyncReport, crate::kernel::error::Error> {
    Ok(crate::vector::VectorSyncReport {
        ok: true,
        projection: "qdrant",
        collection: String::new(),
        records: 0,
        embeddings: 0,
        points_upserted: 0,
        upsert_batches: 0,
        corpus_sha256: String::new(),
        duration_ms: 0,
    })
}

/// A worker that cannot converge stops at its pass bound, and says why.
///
/// The bound is the only thing between a store that never agrees with itself
/// and a process that runs forever, and until now nothing asserted it fires.
/// It had never been written because reaching the real bound costs 64 passes
/// or fifteen minutes. Both seams used here are compiled out of a release
/// build: there is no setting anybody could use to shorten a production bound
/// or to replace the work a real worker does.
#[test]
fn a_worker_that_cannot_converge_stops_at_its_pass_bound() {
    let root = super::harness::bare("bounded-passes");
    // A target nothing will ever reach, so the exit condition stays false and
    // only the bound can stop the loop.
    crate::vector::desired::publish(&root, 9_999).expect("target");
    // The deadline is generous but finite on purpose: with the pass bound
    // removed this test must FAIL rather than hang, or the mutation that
    // proves it discriminating could never be run.
    let report =
        crate::vector::catchup::bounds::with_bounds(2, std::time::Duration::from_secs(5), || {
            crate::vector::catchup::bounds::with_pass(a_pass_that_never_converges, || {
                run_once(&root)
            })
        });

    assert_eq!(report.passes, 2, "the loop did not stop at the pass bound");
    assert_eq!(
        report.attempt_error.as_deref(),
        Some("drain stopped at its bound without converging"),
        "it stopped for another reason"
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// The same worker stops on the clock too, without waiting fifteen minutes.
///
/// Asserted separately from the pass bound because either one alone would let
/// the other rot: a loop bounded only by passes runs as long as a slow pass
/// takes, and one bounded only by time runs as many passes as it can fit.
#[test]
fn a_worker_that_cannot_converge_stops_at_its_deadline() {
    let root = super::harness::bare("bounded-clock");
    crate::vector::desired::publish(&root, 9_999).expect("target");
    // A deadline already spent: the first completed pass is past it, so the
    // clock stops the loop while the pass bound is nowhere near.
    let report =
        crate::vector::catchup::bounds::with_bounds(1_000, std::time::Duration::ZERO, || {
            crate::vector::catchup::bounds::with_pass(a_pass_that_never_converges, || {
                run_once(&root)
            })
        });

    assert_eq!(report.passes, 1, "the deadline did not stop the first pass");
    assert_eq!(
        report.attempt_error.as_deref(),
        Some("drain stopped at its bound without converging"),
        "it stopped for another reason"
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// A panicking body gives the stand-in back.
///
/// Measured through the consequence, not through the slot: after a panic
/// inside the seam, an ordinary run has to reach the unreachable provider and
/// fail there. Before the guard it reported "stopped at its bound" instead —
/// it was still running the substitute, which is what makes a leak so much
/// worse than a second failure: a leaked pass turns a provider that is down
/// into one that succeeds.
#[test]
fn a_panicking_body_hands_the_pass_back() {
    let root = super::harness::bare("seam-panic");
    crate::vector::desired::publish(&root, 9_999).expect("target");
    let escaped = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        crate::vector::catchup::bounds::with_pass(a_pass_that_never_converges, || {
            panic!("the body fails, as a failing assertion would")
        })
    }));
    assert!(escaped.is_err(), "the panic was supposed to escape");

    let report = run_once(&root);
    assert_eq!(
        report.attempt_error.as_deref(),
        Some("projection failed: vector qdrant: list aliases failed"),
        "the substitution outlived the panic and answered for the provider"
    );
}

/// A seam inside a seam hands back what the outer one installed.
///
/// Restoring to none instead would leave the outer body running against
/// production for the rest of its own test — the failure that looks like the
/// outer seam never worked.
#[test]
fn a_nested_seam_restores_the_one_around_it() {
    let root = super::harness::bare("seam-nested");
    crate::vector::desired::publish(&root, 9_999).expect("target");
    let report =
        crate::vector::catchup::bounds::with_bounds(2, std::time::Duration::from_secs(5), || {
            crate::vector::catchup::bounds::with_pass(a_pass_that_never_converges, || {
                // An inner seam over the same slot, entered and left before
                // the outer body does its work.
                crate::vector::catchup::bounds::with_pass(a_pass_that_never_converges, || {});
                run_once(&root)
            })
        });
    assert_eq!(
        report.attempt_error.as_deref(),
        Some("drain stopped at its bound without converging"),
        "the inner seam cleared the outer one instead of restoring it"
    );
    assert_eq!(report.passes, 2, "the outer bound was lost with it");
}
