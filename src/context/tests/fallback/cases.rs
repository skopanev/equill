//! The contract itself: when each half answers, and what bounds the answer.
use super::super::fixtures::records::append;
use super::super::fixtures::registries::registry;
use super::super::fixtures::support::store;
use super::{
    assembled, fallback_only, seed, vector_answers, vector_is_empty, vector_is_unavailable,
};
use crate::filter::Filter;
use std::fs;

/// When vectors answer, text adds nothing — even though the text index would
/// have found the other record for this very query.
#[test]
fn a_vector_answer_is_not_topped_up_with_text() {
    let (root, textual, semantic) = seed("fallback-nonempty");

    let selected = assembled(&root, vector_answers, &Filter::default());

    assert_eq!(
        selected,
        vec![semantic],
        "the text half topped up an answer the vector half had already given"
    );
    assert!(!selected.contains(&textual));
    fs::remove_dir_all(root).expect("remove store");
}

/// An index that answers with nothing has not answered.
#[test]
fn an_empty_vector_answer_falls_back_to_text() {
    let (root, textual, _) = seed("fallback-empty");

    let selected = assembled(&root, vector_is_empty, &Filter::default());

    assert_eq!(
        selected,
        vec![textual],
        "an empty vector half emptied the answer instead of falling back"
    );
    fs::remove_dir_all(root).expect("remove store");
}

/// Neither has an index that could not be reached.
#[test]
fn an_unavailable_index_falls_back_to_text() {
    let (root, textual, _) = seed("fallback-unavailable");

    let selected = assembled(&root, vector_is_unavailable, &Filter::default());

    assert_eq!(
        selected,
        vec![textual],
        "an unreachable index emptied the answer instead of falling back"
    );
    fs::remove_dir_all(root).expect("remove store");
}

/// The case the decision was moved for: the vector half finds something, and
/// everything it finds is then excluded. Deciding before the filter ran would
/// have called that an answer and left the bundle empty.
#[test]
fn vectors_excluded_by_the_filter_still_leave_the_text_answer() {
    let (root, textual, _) = seed("fallback-filtered");
    let excluding = Filter::parse(&["scope=!private".to_string()], false).expect("filter");

    let selected = assembled(&root, vector_answers, &excluding);

    assert_eq!(
        selected,
        vec![textual],
        "every vector candidate was excluded and the text half was suppressed anyway"
    );
    fs::remove_dir_all(root).expect("remove store");
}

/// The bound comes from the store's settings, not from a constant here, and it
/// applies to a fallback answer exactly as it applies to any other.
#[test]
fn the_settings_cap_bounds_a_fallback_answer() {
    let root = store("fallback-cap");
    registry(&root, 400_000, 100_000, &["hybrid"], "agent.memory");
    for index in 0..35 {
        append(
            &root,
            &format!("Searchable needle number {index}"),
            &[],
            None,
            "2026-01-01T00:00:00Z",
        );
    }
    fallback_only(&root);

    let selected = assembled(&root, vector_is_empty, &Filter::default());

    assert_eq!(
        selected.len(),
        30,
        "the fallback answer ignored default_budget_records"
    );
    fs::remove_dir_all(root).expect("remove store");
}

/// The cap is one ceiling over the whole bundle, not a budget for relevance
/// with the obligatory records sitting on top of it. Five required records
/// leave room for twenty-five others, and a fallback answer is bound by what
/// is left rather than by the full thirty.
#[test]
fn required_records_take_their_slots_out_of_the_same_cap() {
    let root = store("fallback-cap-required");
    registry(&root, 400_000, 100_000, &["hybrid"], "agent.memory");
    for index in 0..5 {
        append(
            &root,
            &format!("Obligatory note {index}"),
            &["must"],
            None,
            "2026-01-01T00:00:00Z",
        );
    }
    for index in 0..35 {
        append(
            &root,
            &format!("Searchable needle number {index}"),
            &[],
            None,
            "2026-01-02T00:00:00Z",
        );
    }
    fallback_only(&root);

    let selected = assembled(&root, vector_is_empty, &Filter::default());
    let required = crate::record::read_all(&root)
        .expect("records")
        .into_iter()
        .filter(|record| record.tags.iter().any(|tag| tag == "must"))
        .map(|record| record.id)
        .collect::<Vec<_>>();
    let kept_required = required.iter().filter(|id| selected.contains(id)).count();

    assert_eq!(selected.len(), 30, "the ceiling moved");
    assert_eq!(kept_required, 5, "an obligatory record lost its slot");
    assert_eq!(
        selected.len() - kept_required,
        25,
        "the required records were added on top of the cap instead of inside it"
    );
    fs::remove_dir_all(root).expect("remove store");
}
