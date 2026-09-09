//! Where a rebuild says its work stopped.
//!
//! A rebuild indexes the corpus it captured at the start and then activates a
//! checkpoint. The number in that checkpoint is what the gate compares against
//! the target to decide whether anything is still owed — so if the two are
//! taken at different moments, a write that landed in between is claimed as
//! covered by a pass that never saw it. Nothing downstream can notice: the
//! collection is healthy, the digest is real, and the record is simply not in
//! it, forever, until somebody runs a sync by hand.
use super::{PHYSICAL, add, embedder, fixture};
use crate::vector::catchup::drain::outstanding_for_tests;
use crate::vector::operator::{capture, execute};
use crate::vector::state;
use std::fs;

/// The regression: a write during the pass leaves the checkpoint behind the
/// target, so the next sync is required rather than skipped.
#[test]
fn a_write_during_a_rebuild_leaves_the_checkpoint_behind_the_target() {
    let (root, config, index) = fixture("rebuild-boundary");
    execute(&root, &config, &index, || Ok(embedder(&config, None))).expect("settle");
    assert!(
        !outstanding_for_tests(&root),
        "the premise did not hold: the store was already owed work"
    );

    // A rebuild starts: corpus and target captured together.
    let captured = capture(&root).expect("capture");
    // A write lands while the model would have been running.
    add(&root, "arrived while the pass was running");
    // The rebuild finishes and activates what it captured.
    state::stage_ready(
        &root,
        &config,
        PHYSICAL,
        Some((
            captured.records.len(),
            &captured.digest,
            captured.revision,
            captured.embed_types_sha256.as_deref(),
        )),
    )
    .expect("stage")
    .commit()
    .expect("commit");

    assert!(
        outstanding_for_tests(&root),
        "the rebuild claimed a record it never embedded, and no further pass would run"
    );
    // And the pass that follows picks the record up.
    execute(&root, &config, &index, || Ok(embedder(&config, None))).expect("tail sync");
    assert!(!outstanding_for_tests(&root), "the tail did not settle");
    fs::remove_dir_all(root).expect("remove store");
}

/// The same pass with the target read at the end instead of with the corpus,
/// which is what the code did. It is here because the argument for capturing
/// them together is the difference between these two tests, not an assertion
/// that capturing together is better.
#[test]
fn reading_the_target_at_activation_is_what_swallowed_the_tail() {
    let (root, config, index) = fixture("rebuild-boundary-late");
    execute(&root, &config, &index, || Ok(embedder(&config, None))).expect("settle");

    let captured = capture(&root).expect("capture");
    add(&root, "arrived while the pass was running");
    // The old boundary: the corpus from the start, the target from now.
    let late = crate::vector::desired::read(&root)
        .expect("desired")
        .map_or(0, |target| target.revision);
    state::stage_ready(
        &root,
        &config,
        PHYSICAL,
        Some((
            captured.records.len(),
            &captured.digest,
            late,
            captured.embed_types_sha256.as_deref(),
        )),
    )
    .expect("stage")
    .commit()
    .expect("commit");

    assert!(
        !outstanding_for_tests(&root),
        "the late target no longer swallows the tail, so the capture this code does is no longer justified by this test"
    );
    assert!(
        late > captured.revision,
        "the write did not move the target, so this measures nothing"
    );
    fs::remove_dir_all(root).expect("remove store");
}
