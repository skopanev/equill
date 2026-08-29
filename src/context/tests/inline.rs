use super::super::{assemble, inline_request};
use super::fixtures::records::append;
use super::fixtures::registries::registry_with_modes;
use super::fixtures::support::store;
use crate::filter::Filter;
use serde_json::json;
use std::fs;

/// A question asked from the command line should not require inventing a file
/// or remembering that `at` is mandatory.
#[test]
fn an_inline_request_defaults_to_now_and_parses_coordinates() {
    let request = inline_request(
        Some("retry sweep".into()),
        vec!["scope=alpha".into(), "phase=one,two".into()],
        vec!["must".into()],
        Vec::new(),
        None,
        false,
    )
    .expect("inline request");

    assert_eq!(request.query, "retry sweep");
    assert_eq!(request.tags, vec!["must".to_string()]);
    assert_eq!(request.coordinates["scope"], json!("alpha"));
    // A comma lists alternatives, the way a record holds several values.
    assert_eq!(request.coordinates["phase"], json!(["one", "two"]));
    // `at` defaults to a real instant rather than to an empty string.
    request
        .at
        .parse::<jiff::Timestamp>()
        .expect("at is RFC3339");
}

#[test]
fn a_malformed_coordinate_is_refused_with_its_own_text() {
    for entry in ["scope", "=alpha", "scope="] {
        let error = inline_request(
            None,
            vec![entry.into()],
            Vec::new(),
            Vec::new(),
            None,
            false,
        )
        .expect_err("malformed coordinate");
        assert!(error.to_string().contains(entry), "{error}");
    }
}

/// An empty bundle and a misunderstood coordinate look identical to a caller.
/// The receipt has to tell them apart, or the caller guesses — which is how the
/// twenty minutes in the field report were spent.
#[test]
fn the_receipt_names_a_coordinate_that_matched_nothing() {
    let root = store("unmatched");
    registry_with_modes(&root, 4_000, 1_000, &["exact"], "agent.memory", json!({}));
    append(
        &root,
        "Run the checks",
        &["must"],
        None,
        "2026-01-01T00:00:00Z",
    );
    let request = inline_request(
        Some("checks".into()),
        vec!["scope=absent-value".into()],
        Vec::new(),
        Vec::new(),
        Some("2026-01-05T00:00:00Z".into()),
        false,
    )
    .expect("request");

    let bundle = assemble(
        &root,
        "worker.v1",
        request,
        "test-owner",
        &Filter::default(),
    )
    .expect("context");

    let unmatched = &bundle.receipt.unmatched_coordinates;
    assert_eq!(unmatched.len(), 1);
    assert_eq!(unmatched[0].key, "scope");
    // The selector declares the name, so this is not a typo: it is a value no
    // record carries, under exact comparison.
    assert!(unmatched[0].declared);
    assert!(unmatched[0].exact_only);
    fs::remove_dir_all(root).expect("cleanup");
}
