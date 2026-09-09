//! What a configure that died between its two writes leaves behind.
//!
//! The descriptor and the target are separate writes. A kill between them —
//! not an error, which the undo handles, but the process simply ending — used
//! to leave a store whose filter was new and whose target was old. The gate
//! compares only those two numbers, so it saw nothing to do; the retry compared
//! the same file against itself, found no change, published nothing and
//! returned success. Nothing left to react to, and the vectors of an excluded
//! type answering searches the ledger no longer accounts for.
//!
//! The checkpoint now carries the filter it was taken under, so a marker from
//! before the change stops describing this store at all — and that is what the
//! gate reads, without needing the target to have moved.
use super::super::embedder;
use super::types::{NOTE, set_embed_types, stored, two_types};
use crate::vector::catchup::drain::outstanding_for_tests;
use crate::vector::coverage::fingerprint;
use crate::vector::operator::{execute, store_descriptor};
use serde_json::json;
use std::fs;
use std::path::Path;

fn narrowed(root: &Path) -> serde_json::Value {
    let mut descriptor = stored(root);
    descriptor["embed_types"] = json!(["agent.lesson.v1"]);
    descriptor
}

fn reloaded(root: &Path) -> crate::vector::VectorConfig {
    crate::vector::config::load(root)
        .expect("config")
        .expect("configured")
}

/// The kill, imitated exactly: the descriptor is stored and the target is never
/// published. Everything the ticket asks for follows from this one state.
#[test]
fn a_descriptor_stored_without_its_target_still_owes_the_pass() {
    let (root, config, index, _, note) = two_types("embed-crash");
    execute(&root, &config, &index, || Ok(embedder(&config, None))).expect("settle");
    assert!(
        !outstanding_for_tests(&root),
        "the premise did not hold: work was owed before the crash"
    );

    let before = Some(stored(&root));
    store_descriptor(&root, &narrowed(&root), before).expect("descriptor");

    // The gate sees work without the target having moved.
    assert!(
        outstanding_for_tests(&root),
        "a filter stored without its target left the index looking current"
    );

    // The retry of the same candidate: the filter is unchanged, so nothing is
    // published, and the store must still owe the pass rather than report a
    // success that settles it.
    set_embed_types(&root, &["agent.lesson.v1"]);
    assert!(
        outstanding_for_tests(&root),
        "the retry reported success and left a false current"
    );

    // And the ordinary pass converges: the excluded point goes, nothing is owed.
    let narrowed_config = reloaded(&root);
    execute(&root, &narrowed_config, &index, || {
        Ok(embedder(&narrowed_config, None))
    })
    .expect("sync after the crash");
    assert!(
        !index.inner.lock().unwrap().points.contains_key(&note),
        "the excluded point survived a pass that reported success"
    );
    assert!(!outstanding_for_tests(&root), "the pass did not settle");
    fs::remove_dir_all(root).expect("remove store");
}

/// A filter that changes while a pass is running must not be stamped onto that
/// pass's checkpoint: the corpus it indexed was taken under the old one.
#[test]
fn a_filter_changed_during_a_pass_leaves_the_checkpoint_owed() {
    let (root, config, index, _, _) = two_types("embed-crash-midpass");
    execute(&root, &config, &index, || Ok(embedder(&config, None))).expect("settle");

    // A pass begins under no filter.
    let captured = crate::vector::operator::capture(&root).expect("capture");
    assert_eq!(
        captured.embed_types_sha256, None,
        "the premise did not hold: the capture already carried a filter"
    );
    // The descriptor changes underneath it.
    store_descriptor(&root, &narrowed(&root), Some(stored(&root))).expect("descriptor");
    // The pass activates what it actually indexed.
    crate::vector::state::stage_ready(
        &root,
        &reloaded(&root),
        super::super::PHYSICAL,
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
        "a checkpoint from before the filter change was read as this store's position"
    );
    fs::remove_dir_all(root).expect("remove store");
}

/// A store that configures no filter writes no field, which is exactly what a
/// marker written before the field existed looks like. Those markers stay
/// usable, so nobody is forced through a pass for a change they did not make.
#[test]
fn a_marker_without_the_field_stays_usable_when_no_filter_is_configured() {
    let (root, config, index, _, _) = two_types("embed-crash-legacy");
    execute(&root, &config, &index, || Ok(embedder(&config, None))).expect("settle");
    let marker: serde_json::Value = serde_json::from_slice(
        &fs::read(root.join("projections/qdrant/state.json")).expect("read"),
    )
    .expect("json");

    assert!(
        marker.get("embed_types_sha256").is_none(),
        "a store with no filter wrote a field a legacy marker would not have:\n{marker}"
    );
    assert!(
        !outstanding_for_tests(&root),
        "a marker without the field was read as describing another store"
    );
    fs::remove_dir_all(root).expect("remove store");
}

/// The filter is a set. Tidying the configuration must not cost a full pass.
#[test]
fn reordering_or_repeating_names_does_not_change_the_fingerprint() {
    let plain = fingerprint(&["agent.lesson.v1".into(), NOTE.into()]);
    let shuffled = fingerprint(&[NOTE.into(), "agent.lesson.v1".into()]);
    let repeated = fingerprint(&[NOTE.into(), "agent.lesson.v1".into(), NOTE.into()]);

    assert_eq!(plain, shuffled);
    assert_eq!(plain, repeated);
    assert!(plain.is_some());
    assert_eq!(fingerprint(&[]), None, "no filter must write no field");
    assert_ne!(
        plain,
        fingerprint(&["agent.lesson.v1".into()]),
        "a narrower filter shares an identity with a wider one"
    );
}
