//! What the filter tells the operator it did, and what the health check makes
//! of a selector the filter left with nothing to search.
use super::types::{NOTE, set_embed_types, two_types};
use serde_json::json;
use std::fs;
use std::path::Path;

/// A selector with a vector strategy over a type the store does not embed
/// returns nothing, and nothing reads as "no relevant memory" rather than
/// "never indexed". The health check is where that difference can still be
/// noticed.
#[test]
fn doctor_names_a_selector_searching_vectors_for_an_unembedded_type() {
    let (root, _, _, _, _) = two_types("embed-types-doctor");
    register_selector(&root, "note.hybrid", NOTE, &["hybrid"]);
    assert!(
        crate::command::doctor::report(Some(&root), false, false)
            .expect("doctor")
            .ok,
        "the store is inconsistent before a filter exists"
    );

    set_embed_types(&root, &["agent.lesson.v1"]);
    let report = crate::command::doctor::report(Some(&root), false, false).expect("doctor");

    assert_eq!(report.vector_uncovered_selectors, 1);
    assert!(!report.ok, "doctor passed a store that answers silently");
    assert!(
        report
            .checks
            .iter()
            .any(|check| check.id == "vector-embed-types" && check.items == 1)
    );
    fs::remove_dir_all(root).expect("remove store");
}

/// The operator has to be able to see the filter doing something rather than
/// infer it from a number that got smaller.
#[test]
fn status_says_how_many_the_filter_left_out_and_rebuild_counts_the_same() {
    let (root, _, _, _, _) = two_types("embed-types-counts");
    let before = crate::command::status::report(Some(&root)).expect("status");
    assert_eq!(
        before.store.and_then(|store| store.vector).map(|vector| (
            vector.vector_eligible_records,
            vector.vector_skipped_records
        )),
        Some((2, None)),
        "an unfiltered store reported a skipped count, which is a different answer from no filter"
    );

    set_embed_types(&root, &["agent.lesson.v1"]);
    let after = crate::command::status::report(Some(&root))
        .expect("status")
        .store
        .and_then(|store| store.vector)
        .expect("vector counts");
    let text = crate::command::output::status(
        &crate::command::status::report(Some(&root)).expect("status"),
    );

    assert_eq!(after.vector_eligible_records, 1);
    assert_eq!(after.vector_skipped_records, Some(1));
    assert!(
        text.contains("1 skipped by embed_types"),
        "the human line does not say the filter did anything:\n{text}"
    );
    // The rebuild report reads its count from the same snapshot, so the two
    // surfaces cannot drift into disagreeing about one number.
    assert_eq!(
        crate::vector::coverage::corpus_snapshot(&root)
            .expect("snapshot")
            .skipped_by_type,
        1
    );
    fs::remove_dir_all(root).expect("remove store");
}

fn register_selector(root: &Path, id: &str, type_name: &str, strategies: &[&str]) {
    let file = root.join(format!("{id}.json"));
    fs::write(
        &file,
        serde_json::to_vec(&json!({
            "id": id,
            "version": "1.0.0",
            "type": type_name,
            "strategies": strategies,
            "expect": "any"
        }))
        .expect("selector json"),
    )
    .expect("selector file");
    crate::context::register_selector(root, &file, "owner").expect("register selector");
}
