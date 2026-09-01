//! One controlled answer, checked on all three surfaces.
//!
//! The semantic half is substituted rather than served by a live Qdrant, so the
//! assertions are about the contract — what merges, in what order, and what the
//! answer admits about itself — and not about what a provider happened to
//! return that day. The text half is real: it comes from the store's own
//! projection, so the merge is exercised against a list nobody staged.
use super::catchup::{add, store};
use super::support::{cli, failing, first_only, half, request, staged};
use crate::vector::{SearchStrategy, search, with_semantic_half};
use std::fs;

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

/// A substitution inside another must hand the outer one back.
///
/// Measured by asking, not by reading the slot: what matters is that the outer
/// body still gets the answer it arranged for, and a test that inspected the
/// thread-local would pass even if the search had stopped consulting it.
#[test]
fn an_inner_substitution_hands_the_outer_one_back() {
    let root = store("cross-surface-nested");
    add(&root, "a rule about deployment");
    add(&root, "another deployment rule");
    add(&root, "deployment again");
    let outer_leads = staged(&root)[0].id;

    with_semantic_half(half, || {
        let inner = with_semantic_half(first_only, || {
            search(&root, &request(), SearchStrategy::Hybrid).expect("inner answers")
        });
        // The inner substitution is in force while it is in force.
        assert_eq!(
            inner.hits[0].record.id,
            crate::record::read_all(&root).expect("ledger")[0].id,
            "the inner half never took effect, so nothing was nested"
        );
        // And once it leaves, the outer one is answering again.
        let after = search(&root, &request(), SearchStrategy::Hybrid).expect("outer answers");
        // `answered_by` is what tells the two apart. Position alone does not:
        // when the substitution is lost the live half fails, hybrid falls back
        // to text, and text can lead with the same record the outer half did —
        // which is exactly how the first version of this test passed whether
        // the guard restored or cleared.
        assert_eq!(
            after.answered_by, "hybrid",
            "the inner substitution cleared the outer one, so the live half answered"
        );
        assert_eq!(
            after.hits[0].record.id, outer_leads,
            "the outer half came back but stopped leading its own record"
        );
        // Even when the inner body panics rather than returns.
        let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            with_semantic_half(first_only, || panic!("inner body fails"))
        }));
        assert!(panicked.is_err(), "the panic did not happen");
        let recovered = search(&root, &request(), SearchStrategy::Hybrid).expect("outer answers");
        assert_eq!(
            recovered.answered_by, "hybrid",
            "a panicking inner substitution took the outer one with it"
        );
        assert_eq!(recovered.hits[0].record.id, outer_leads);
    });
    fs::remove_dir_all(root).expect("cleanup");
}
