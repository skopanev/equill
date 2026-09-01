use super::super::assemble;
use super::super::model::{ExclusionReason, Strategy};
use super::fixtures::records::append;
use super::fixtures::registries::registry;
use super::fixtures::support::{request, store};
use crate::filter::Filter;
use crate::projection::SearchRequest;
use crate::record::StoredRecord;
use crate::vector::{RejectedHit, with_semantic_half};
use crate::{projection, record};
use std::fs;

#[test]
fn degraded_fts_is_visible_and_no_match_is_explicit() {
    let root = store("degraded-fts");
    registry(&root, 4_000, 1_000, &["fts"], "agent.memory");
    append(
        &root,
        "Searchable needle",
        &[],
        None,
        "2026-01-01T00:00:00Z",
    );
    let record = record::read_all(&root).expect("records").remove(0);
    projection::mark_degraded(&root, &record, "synthetic projection fault");
    let bundle = assemble(
        &root,
        "worker.v1",
        request("needle"),
        "test-owner",
        &Filter::default(),
    )
    .expect("degraded context");

    assert!(bundle.receipt.degraded);
    assert!(bundle.receipt.empty);
    assert_eq!(bundle.receipt.degraded_strategies, vec![Strategy::Fts]);
    assert!(bundle.content.is_empty());
    fs::remove_dir_all(root).expect("remove store");
}

#[test]
fn unauthorized_records_are_named_in_the_receipt() {
    let root = store("unauthorized");
    registry(&root, 4_000, 1_000, &["exact"], "agent.other");
    let id = append(
        &root,
        "Private synthetic rule",
        &[],
        None,
        "2026-01-01T00:00:00Z",
    );
    let bundle = assemble(
        &root,
        "worker.v1",
        request("private"),
        "test-owner",
        &Filter::default(),
    )
    .expect("context");

    assert!(
        bundle
            .receipt
            .excluded
            .iter()
            .any(|item| { item.id == id && item.reason == ExclusionReason::Unauthorized })
    );
    fs::remove_dir_all(root).expect("remove store");
}

/// A hybrid selector degrades differently from a broken projection: the text
/// half still answers, so the bundle is not empty — it is merely less than it
/// asked for, and the receipt is where that difference is recorded.
fn seed(name: &str) -> std::path::PathBuf {
    let root = store(name);
    registry(&root, 4_000, 1_000, &["hybrid"], "agent.memory");
    append(
        &root,
        "Searchable needle",
        &[],
        None,
        "2026-01-01T00:00:00Z",
    );
    root
}

fn half(
    store_root: &std::path::Path,
    _request: &SearchRequest,
) -> Result<(Vec<StoredRecord>, Vec<RejectedHit>), crate::kernel::error::Error> {
    Ok((crate::record::read_all(store_root)?, Vec::new()))
}

fn failing(
    _store_root: &std::path::Path,
    _request: &SearchRequest,
) -> Result<(Vec<StoredRecord>, Vec<RejectedHit>), crate::kernel::error::Error> {
    // Any failure of the semantic half reaches the merge the same way; the
    // point is the account the receipt gives, not which fault produced it.
    Err(crate::kernel::error::Error::Projection(
        "index unreachable".into(),
    ))
}

#[test]
fn a_hybrid_bundle_records_what_answered_it() {
    let root = seed("hybrid-receipt");

    let bundle = with_semantic_half(half, || {
        assemble(
            &root,
            "worker.v1",
            request("needle"),
            "test-owner",
            &Filter::default(),
        )
        .expect("hybrid context")
    });

    let semantic = bundle
        .receipt
        .semantic
        .as_ref()
        .expect("a hybrid selector ran, so the receipt owes an account of it");
    assert_eq!(semantic.answered_by, "hybrid");
    assert!(semantic.fallback.is_none(), "nothing stood in");
    // The version names the shape: a reader that knows only v1 is told plainly
    // that this receipt carries a field it has never seen.
    assert_eq!(bundle.receipt.schema, "equill.context-receipt.v2");
    assert!(!bundle.receipt.empty, "the record was not selected");
    fs::remove_dir_all(root).expect("remove store");
}

#[test]
fn a_bundle_that_lost_its_index_says_which_half_answered() {
    let root = seed("hybrid-fallback");

    let bundle = with_semantic_half(failing, || {
        assemble(
            &root,
            "worker.v1",
            request("needle"),
            "test-owner",
            &Filter::default(),
        )
        .expect("text stands in")
    });

    let semantic = bundle.receipt.semantic.as_ref().expect("still an account");
    assert_eq!(
        semantic.answered_by, "fts",
        "the bundle claimed semantics it never got"
    );
    assert!(
        semantic
            .fallback
            .as_deref()
            .is_some_and(|reason| reason.contains("unreachable")),
        "the reason is in the receipt, not only in the log: {:?}",
        semantic.fallback
    );
    fs::remove_dir_all(root).expect("remove store");
}

#[test]
fn a_text_only_bundle_keeps_the_shape_it_always_had() {
    let root = store("hybrid-untouched");
    registry(&root, 4_000, 1_000, &["fts"], "agent.memory");
    append(
        &root,
        "Searchable needle",
        &[],
        None,
        "2026-01-01T00:00:00Z",
    );

    // Substituted anyway: a profile that did not ask for semantics must not
    // acquire them because something else on the machine could answer.
    let bundle = with_semantic_half(half, || {
        assemble(
            &root,
            "worker.v1",
            request("needle"),
            "test-owner",
            &Filter::default(),
        )
        .expect("text context")
    });

    assert!(
        bundle.receipt.semantic.is_none(),
        "a text profile grew a semantic account it never asked for"
    );
    assert_eq!(bundle.receipt.schema, "equill.context-receipt.v1");
    fs::remove_dir_all(root).expect("remove store");
}
