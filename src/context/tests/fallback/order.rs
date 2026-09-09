//! The preference is the store's, in either direction.
use super::super::fixtures::records::append_scoped;
use super::super::fixtures::registries::registry;
use super::super::fixtures::support::store;
use super::{VECTOR_ONLY, assembled, ordered_fallback, seed, vector_answers};
use crate::filter::Filter;
use std::fs;

/// The preference belongs to the store. `hybrid_order` may name text first —
/// the settings reader accepts it — and a setting that can be written has to be
/// able to take effect. Preferring vectors regardless would silently invert it.
#[test]
fn the_configured_first_source_is_the_one_that_answers() {
    let (root, textual, semantic) = seed("fallback-order");
    ordered_fallback(&root, ["fts", "vector"]);

    let selected = assembled(&root, vector_answers, &Filter::default());

    assert_eq!(
        selected,
        vec![textual],
        "text was configured first and the vector half answered instead"
    );
    assert!(!selected.contains(&semantic));
    fs::remove_dir_all(root).expect("remove store");
}

/// And with text first, an empty text half falls back to vectors — the same
/// contract read in the other direction.
#[test]
fn text_first_falls_back_to_vectors_when_text_finds_nothing() {
    let root = store("fallback-order-empty");
    registry(&root, 4_000, 1_000, &["hybrid"], "agent.memory");
    let semantic = append_scoped(
        &root,
        VECTOR_ONLY,
        &[],
        None,
        "2026-01-02T00:00:00Z",
        Some("private"),
    );
    ordered_fallback(&root, ["fts", "vector"]);

    let selected = assembled(&root, vector_answers, &Filter::default());

    assert_eq!(
        selected,
        vec![semantic],
        "text found nothing for this query and the vector half was suppressed anyway"
    );
    fs::remove_dir_all(root).expect("remove store");
}
