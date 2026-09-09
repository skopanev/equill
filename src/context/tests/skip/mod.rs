//! Lines an operator declared are not questions.
//!
//! What is at stake is not that the search returns nothing — an empty result
//! and an unasked question look identical from outside — but that the search is
//! never run, while everything the query did not choose is assembled exactly as
//! before. These tests measure the calls and check the contract separately.
use super::super::assemble;
use super::fixtures::records::append;
use super::fixtures::registries::registry;
use super::fixtures::support::{request, store};
use crate::context::retrieval::probe;
use crate::filter::Filter;
use serde_json::json;
use std::fs;
use std::path::Path;

const BUS: &str = "[BUS] unread: 3. To read: agentbus drain";
const BUS_RULE: &str = "^\\[BUS\\] unread:";

/// A store whose settings declare the given skip patterns, and one record for
/// the selectors to find.
fn configured(name: &str, patterns: &[&str]) -> std::path::PathBuf {
    let root = store(name);
    registry(
        &root,
        4_000,
        1_000,
        &["exact", "recency", "fts"],
        "agent.memory",
    );
    append(
        &root,
        "Measure before you optimise",
        &[],
        None,
        "2026-01-01T00:00:00Z",
    );
    settings(&root, patterns);
    root
}

fn settings(root: &Path, patterns: &[&str]) {
    fs::write(
        root.join("settings.json"),
        serde_json::to_vec(&json!({
            "retrieval": {
                "default_budget_records": 30,
                "query_instruction": "Retrieve durable memory directly applicable to the current request.",
                "vector": { "enabled": true, "score_threshold": 0.48 },
                "hybrid": { "order": ["vector", "fts"], "fill_remaining": true, "deduplicate": true },
                "skip_query_patterns": patterns
            }
        }))
        .expect("settings json"),
    )
    .expect("settings file");
}

fn assembled(root: &Path, query: &str) -> crate::context::ContextBundle {
    probe::reset();
    assemble(
        root,
        "worker.v1",
        request(query),
        "test-owner",
        &Filter::default(),
    )
    .expect("context")
}

/// The promise the setting makes is about the calls, not the results.
#[test]
fn a_matched_query_reaches_neither_the_projection_nor_the_embedder() {
    let root = configured("skip-calls", &[BUS_RULE]);

    assembled(&root, BUS);
    let skipped = probe::counts();
    assembled(&root, "what did we decide about compaction?");
    let asked = probe::counts();

    assert_eq!(
        skipped,
        (0, 0),
        "a skipped query still entered the search path"
    );
    assert!(
        asked.0 > 0,
        "the control question never reached the projection, so the measurement above proves nothing"
    );
    fs::remove_dir_all(root).expect("remove store");
}

/// The correction that matters: a match suppresses the query-driven half and
/// nothing else. An agent whose prompt happened to match must still be handed
/// what its coordinate and recency selectors chose.
#[test]
fn a_matched_query_still_assembles_what_the_query_did_not_choose() {
    let root = configured("skip-contract", &[BUS_RULE]);

    let bundle = assembled(&root, BUS);

    assert_eq!(
        bundle.selected_record_ids.len(),
        1,
        "the skip emptied the bundle instead of emptying the query path"
    );
    assert!(!bundle.receipt.empty);
    fs::remove_dir_all(root).expect("remove store");
}

/// A pattern matching every line is the worst case an operator can write, and
/// the contract still has to survive it: the selectors that never look at the
/// query are not reachable from a pattern at all.
#[test]
fn a_pattern_matching_everything_cannot_empty_the_contract() {
    let root = configured("skip-everything", &[".*"]);

    let bundle = assembled(&root, BUS);

    assert_eq!(bundle.selected_record_ids.len(), 1);
    assert_eq!(probe::counts(), (0, 0));
    fs::remove_dir_all(root).expect("remove store");
}

/// A request with no query is not a skippable one, and `.*` matches the empty
/// string. Checking only that the records survived would have missed this: the
/// bundle was right and the receipt was not, because a rule had been stamped
/// into it and into the digest a caller may already hold.
///
/// Compared against the same store before any pattern is configured, with the
/// same request, so the two receipts are comparable byte for byte.
#[test]
fn a_request_without_a_query_is_untouched_by_a_pattern_matching_everything() {
    let root = configured("skip-sessionstart", &[]);
    let before = assembled(&root, "");
    let before_bytes = serde_json::to_vec(&before.receipt).expect("receipt json");

    settings(&root, &[".*"]);
    let after = assembled(&root, "");
    let after_bytes = serde_json::to_vec(&after.receipt).expect("receipt json");

    assert_eq!(after.receipt.query_skipped_by, None);
    assert_eq!(
        after.bundle_digest, before.bundle_digest,
        "a pattern changed the bundle of a request that carries no query"
    );
    assert_eq!(
        String::from_utf8(after_bytes).expect("utf8"),
        String::from_utf8(before_bytes).expect("utf8"),
        "a pattern changed the receipt of a request that carries no query"
    );
    fs::remove_dir_all(root).expect("remove store");
}

/// The receipt says a skip applied and which rule did it. It never says what
/// was asked.
#[test]
fn the_receipt_names_the_rule_and_never_the_query() {
    let root = configured("skip-receipt", &[BUS_RULE]);

    let bundle = assembled(&root, BUS);
    let serialized = serde_json::to_string(&bundle.receipt).expect("receipt json");

    assert_eq!(bundle.receipt.query_skipped_by.as_deref(), Some(BUS_RULE));
    assert!(
        !serialized.contains("agentbus drain") && !serialized.contains("unread: 3"),
        "the receipt carried the query text:\n{serialized}"
    );
    fs::remove_dir_all(root).expect("remove store");
}

/// A store that configures nothing behaves as it always did, and its receipt is
/// the shape it always was: the field is absent, not null.
#[test]
fn a_store_that_configures_no_patterns_is_unchanged() {
    let root = configured("skip-none", &[]);

    let bundle = assembled(&root, BUS);
    let serialized = serde_json::to_string(&bundle.receipt).expect("receipt json");

    assert!(
        probe::counts().0 > 0,
        "the query path was skipped with no rule to skip it"
    );
    assert_eq!(bundle.receipt.query_skipped_by, None);
    assert!(
        !serialized.contains("query_skipped_by"),
        "an unconfigured store's receipt grew a field:\n{serialized}"
    );
    fs::remove_dir_all(root).expect("remove store");
}

/// Matched against the raw query, so what an operator writes is what they can
/// predict from the line they see. A case-folding or trimming step before the
/// match would make the same pattern behave differently from how it reads.
#[test]
fn the_pattern_is_matched_against_the_raw_query() {
    let root = configured("skip-raw", &[BUS_RULE]);

    let exact = assembled(&root, BUS);
    let lowercased = assembled(&root, "[bus] unread: 3. To read: agentbus drain");
    let indented = assembled(&root, "  [BUS] unread: 3. To read: agentbus drain");

    assert_eq!(exact.receipt.query_skipped_by.as_deref(), Some(BUS_RULE));
    assert_eq!(
        lowercased.receipt.query_skipped_by, None,
        "the query was case-folded before the match"
    );
    assert_eq!(
        indented.receipt.query_skipped_by, None,
        "the query was trimmed before the match, so an anchored pattern does not read as written"
    );
    fs::remove_dir_all(root).expect("remove store");
}

/// The health check reads the same settings, so a pattern that will not compile
/// is a store that reports itself broken rather than one that quietly stopped
/// filtering.
#[test]
fn doctor_refuses_a_store_whose_pattern_will_not_compile() {
    let root = configured("skip-doctor", &["^\\[BUS\\] unread:"]);
    assert!(
        crate::command::doctor::report(Some(&root), false, false).is_ok(),
        "a store with a valid pattern must stay healthy"
    );

    settings(&root, &["("]);
    let error = crate::command::doctor::report(Some(&root), false, false)
        .expect_err("doctor accepted an unparseable pattern");

    assert!(
        error.to_string().contains("skip_query_patterns"),
        "doctor did not say which setting is wrong: {error}"
    );
    fs::remove_dir_all(root).expect("remove store");
}
