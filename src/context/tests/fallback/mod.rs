//! Text as a fallback for the vector half, not as a top-up.
//!
//! The contract the owner asked for: when the vector half answers, the text
//! half adds nothing; when it does not, the text half answers instead. The
//! second clause is the one that was broken — "no top-up" had been read as
//! "stop after the first source", so an empty or unreachable index produced an
//! empty answer where text could have produced a real one.
//!
//! These run through `assemble`, so the decision is exercised where it now
//! lives: after the coordinates, grants, filter and lifecycle have had their
//! say, which is the only point at which "the vector half answered" can be
//! told from "the vector half found things that were all excluded".
use super::super::assemble;
use super::fixtures::records::{append, append_scoped};
use super::fixtures::registries::registry;
use super::fixtures::support::{request, store};
use crate::filter::Filter;
use crate::projection::SearchRequest;
use crate::record::StoredRecord;
use crate::vector::{RejectedHit, with_semantic_half};
use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};
use uuid::Uuid;

type Half = Result<(Vec<StoredRecord>, Vec<RejectedHit>), crate::kernel::error::Error>;

/// One record the text index can find by the query word, and one it cannot.
/// Keeping them apart is what makes "which half answered" observable at all.
const TEXT_FINDS: &str = "Searchable needle in the ledger";
const VECTOR_ONLY: &str = "A thought about compaction";

fn seed(name: &str) -> (PathBuf, Uuid, Uuid) {
    let root = store(name);
    registry(&root, 4_000, 1_000, &["hybrid"], "agent.memory");
    let textual = append(&root, TEXT_FINDS, &[], None, "2026-01-01T00:00:00Z");
    // Given a scope so a filter can exclude exactly this one, which is how the
    // "vectors found things, all of them excluded" case is built.
    let semantic = append_scoped(
        &root,
        VECTOR_ONLY,
        &[],
        None,
        "2026-01-02T00:00:00Z",
        Some("private"),
    );
    fallback_only(&root);
    (root, textual, semantic)
}

/// `fill_remaining: false` — the setting under test.
fn fallback_only(root: &Path) {
    fs::write(
        root.join("settings.json"),
        serde_json::to_vec(&json!({
            "retrieval": {
                "default_budget_records": 30,
                "query_instruction": "Retrieve durable memory directly applicable to the current request.",
                "vector": { "enabled": true, "score_threshold": 0.48 },
                "hybrid": { "order": ["vector", "fts"], "fill_remaining": false, "deduplicate": true }
            }
        }))
        .expect("settings json"),
    )
    .expect("settings file");
}

/// A semantic half that returns only the record the text index cannot find.
fn vector_answers(store_root: &Path, _request: &SearchRequest) -> Half {
    let records = crate::record::read_all(store_root)?
        .into_iter()
        .filter(|record| record.payload["rule"] == VECTOR_ONLY)
        .collect();
    Ok((records, Vec::new()))
}

fn vector_is_empty(_store_root: &Path, _request: &SearchRequest) -> Half {
    Ok((Vec::new(), Vec::new()))
}

fn vector_is_unavailable(_store_root: &Path, _request: &SearchRequest) -> Half {
    Err(crate::kernel::error::Error::Projection(
        "index unreachable".into(),
    ))
}

fn assembled(root: &Path, half: fn(&Path, &SearchRequest) -> Half, filter: &Filter) -> Vec<Uuid> {
    with_semantic_half(half, || {
        assemble(root, "worker.v1", request("needle"), "test-owner", filter)
            .expect("context")
            .selected_record_ids
    })
}

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
