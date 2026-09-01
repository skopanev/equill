//! One controlled answer, checked on all three surfaces.
//!
//! The semantic half is substituted rather than served by a live Qdrant, so the
//! assertions are about the contract — what merges, in what order, and what the
//! answer admits about itself — and not about what a provider happened to
//! return that day. The text half is real: it comes from the store's own
//! projection, so the merge is exercised against a list nobody staged.
use super::catchup::{add, store};
use crate::projection::SearchRequest;
use crate::record::StoredRecord;
use crate::vector::{SearchStrategy, search, with_semantic_half};
use std::fs;

/// The substitute names ONE record, and text will find all of them.
///
/// A half that returned everything would be indistinguishable from no merge at
/// all: the counts would match whether the text list was consulted or quietly
/// dropped. With a strict subset, only a real union reaches the full count, and
/// the one record both halves name is the one whose position proves the
/// fusion — agreement has to outrank a record that leads only the text list.
fn staged(store_root: &std::path::Path) -> Vec<StoredRecord> {
    let mut records = crate::record::read_all(store_root).expect("ledger");
    records.split_off(records.len() - 1)
}

fn half(
    store_root: &std::path::Path,
    _request: &SearchRequest,
) -> Result<(Vec<StoredRecord>, Vec<crate::vector::RejectedHit>), crate::kernel::error::Error> {
    Ok((staged(store_root), Vec::new()))
}

fn failing(
    _store_root: &std::path::Path,
    _request: &SearchRequest,
) -> Result<(Vec<StoredRecord>, Vec<crate::vector::RejectedHit>), crate::kernel::error::Error> {
    Err(crate::vector::model::vector_error(
        "index unreachable in this test",
    ))
}

fn request() -> SearchRequest {
    SearchRequest {
        query: Some("deployment".into()),
        namespace: None,
        type_name: None,
        limit: 10,
    }
}

#[test]
fn a_hybrid_answer_merges_both_halves_and_says_so() {
    let root = store("cross-surface-hybrid");
    add(&root, "a rule about deployment");
    add(&root, "another deployment rule");
    add(&root, "deployment again");

    let report = with_semantic_half(half, || {
        search(&root, &request(), SearchStrategy::Hybrid).expect("hybrid answers")
    });

    assert_eq!(report.answered_by, "hybrid", "text half was never asked");
    // Three, from a semantic half that named one: the text list was consulted
    // and merged, not discarded. A concatenation would have returned four.
    assert_eq!(report.returned_count, 3, "a half was dropped or duplicated");
    assert_eq!(
        report.hits[0].record.id,
        staged(&root)[0].id,
        "the record both halves named did not outrank one only text found"
    );
    assert!(
        report.total_matches.is_none(),
        "an approximate half cannot prove a total"
    );
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn the_same_hybrid_question_answers_the_same_way_twice() {
    let root = store("cross-surface-order");
    add(&root, "a rule about deployment");
    add(&root, "another deployment rule");
    add(&root, "deployment again");

    let first = with_semantic_half(half, || {
        search(&root, &request(), SearchStrategy::Hybrid).expect("hybrid answers")
    });
    let again = with_semantic_half(half, || {
        search(&root, &request(), SearchStrategy::Hybrid).expect("hybrid answers")
    });

    let ids = |report: &crate::vector::StrategySearchReport| {
        report
            .hits
            .iter()
            .map(|hit| hit.record.id)
            .collect::<Vec<_>>()
    };
    assert_eq!(
        ids(&first),
        ids(&again),
        "two identical questions disagreed"
    );
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn an_unreachable_index_answers_from_text_and_names_the_reason() {
    let root = store("cross-surface-fallback");
    add(&root, "a rule about deployment");

    let report = with_semantic_half(failing, || {
        search(&root, &request(), SearchStrategy::Hybrid).expect("text stands in")
    });

    assert_eq!(
        report.answered_by, "fts",
        "a failed half was reported as one"
    );
    assert!(
        report
            .fallback
            .as_deref()
            .is_some_and(|reason| reason.contains("unreachable")),
        "the fall back is in the report, not only in the outcome: {:?}",
        report.fallback
    );
    assert_eq!(report.returned_count, 1, "text lost its own answer");
    fs::remove_dir_all(root).expect("cleanup");
}

/// The adapter answers with the same contract the command line does, and the
/// rule that decides which half runs is the request, not the surface.
#[test]
fn the_adapter_asks_both_halves_for_a_question_and_only_text_for_an_enumeration() {
    let root = store("cross-surface-mcp");
    add(&root, "a rule about deployment");
    add(&root, "an unrelated rule");

    let asked = with_semantic_half(half, || {
        crate::mcp::tools_call(
            &root,
            "owner",
            false,
            "search",
            &serde_json::json!({ "query": "deployment", "where": ["type=agent.lesson.v1"] }),
        )
        .expect("mcp search")
    });
    let enumerated = with_semantic_half(half, || {
        crate::mcp::tools_call(
            &root,
            "owner",
            false,
            "search",
            &serde_json::json!({ "where": ["type=agent.lesson.v1"] }),
        )
        .expect("mcp filter")
    });

    // A filter narrows what may come back; it does not turn a question into an
    // enumeration, so the question still gets both halves.
    assert_eq!(asked["answered_by"], "hybrid", "{asked}");
    assert!(asked["total_matches"].is_null(), "{asked}");
    // A request with no question is an enumeration, and only text can walk the
    // scope it asks for — so it keeps the exact total it always had.
    assert_eq!(enumerated["answered_by"], "fts", "{enumerated}");
    assert_eq!(enumerated["total_matches"], 2, "{enumerated}");
    // Nothing the filter admits is lost by taking the semantic route: both
    // records still come back, because the text half enumerated the same pool
    // it always did and the merge only reorders.
    assert_eq!(asked["returned_count"], 2, "{asked}");
    fs::remove_dir_all(root).expect("cleanup");
}

/// The command line is the third surface, and it was the one the merge could
/// most easily miss: it reaches the same core through its own argument
/// handling, its own default, and its own rendering.
#[test]
fn the_command_line_merges_both_halves_and_falls_back_in_the_open() {
    let root = store("cross-surface-cli");
    add(&root, "a rule about deployment");
    add(&root, "another deployment rule");
    add(&root, "deployment again");

    let merged = with_semantic_half(half, || cli(&root)).expect("hybrid answers");
    let degraded = with_semantic_half(failing, || cli(&root)).expect("text stands in");

    // No strategy was named, so the default had to choose hybrid, ask both
    // halves and merge them: the semantic half named one record, text found
    // three, and the answer carries three.
    assert!(merged.contains("\"answered_by\":\"hybrid\""), "{merged}");
    assert!(merged.contains("\"returned_count\":3"), "{merged}");
    assert!(!merged.contains("\"total_matches\""), "{merged}");
    // The record both halves named leads the answer.
    let first = staged(&root)[0].id.to_string();
    let leading = merged
        .find(&first)
        .expect("the agreed record is missing entirely");
    for other in crate::record::read_all(&root).expect("ledger") {
        if other.id.to_string() != first {
            assert!(
                merged.find(&other.id.to_string()).expect("record") > leading,
                "agreement did not lead the answer"
            );
        }
    }
    // And when the index cannot answer, the command line says so rather than
    // returning a quietly smaller answer that looks like the same thing.
    assert!(degraded.contains("\"answered_by\":\"fts\""), "{degraded}");
    assert!(degraded.contains("unreachable"), "{degraded}");
    fs::remove_dir_all(root).expect("cleanup");
}

fn cli(root: &std::path::Path) -> Result<String, crate::kernel::error::Error> {
    crate::command::query::search(
        true,
        root.to_path_buf(),
        Some("deployment".into()),
        None,
        None,
        10,
        None,
        Vec::new(),
        false,
        crate::command::cli::FormatArg::Jsonl,
        Vec::new(),
        false,
    )
}
