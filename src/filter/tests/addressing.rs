use super::super::{candidate_limit, matches, scope_size, validate};
use super::stores::scoped_store;
use super::support::*;
use serde_json::json;
#[test]
fn dotted_paths_reach_nested_objects_and_lists_of_them() {
    assert!(matches(
        &record(),
        &filter(&["evidence.kind=commit"], false)
    ));
    assert!(matches(&record(), &filter(&["evidence.digest=abc"], false)));
    assert!(!matches(
        &record(),
        &filter(&["evidence.kind=review"], false)
    ));
}

/// Retraction and expiry live in nullable fields, so asking about presence is
/// the ordinary case, not an edge case.
/// A selector reads an absent coordinate as "applies to everything". The filter
/// agrees by default so the two do not disagree invisibly, and `--strict` is
/// how a caller asks for the other reading.
#[test]
fn an_unknown_field_names_itself_instead_of_returning_nothing() {
    let error = validate(&filter(&["sorce=owner"], false), &[definition()])
        .expect_err("unknown field")
        .to_string();
    let nested = validate(&filter(&["evidence.sha=abc"], false), &[definition()])
        .expect_err("unknown nested field")
        .to_string();

    assert!(error.contains("unknown field sorce"), "{error}");
    assert!(error.contains("agent.lesson.v1"), "{error}");
    assert!(nested.contains("evidence.sha"), "{nested}");
    validate(&filter(&["source=owner"], false), &[definition()]).expect("declared field");
    validate(&filter(&["evidence.kind=commit"], false), &[definition()]).expect("nested field");
}

#[test]
fn filtered_search_requests_the_whole_corpus_or_refuses_explicitly() {
    assert_eq!(candidate_limit(137, 20).expect("whole corpus"), 137);
    assert_eq!(candidate_limit(3, 20).expect("requested page"), 20);
    assert!(candidate_limit(usize::from(u16::MAX) + 1, 20).is_err());
}

/// `evidence` exists on both halves. A bare name has to reach the payload's,
/// because that is the schema the caller was reading; checking the envelope
/// first rejected an ordinary payload field for a name collision it never
/// caused.
#[test]
fn a_bare_name_validates_against_the_payload_even_when_the_envelope_collides() {
    let mut item = record();
    item.payload = json!({
        "rule": "Run",
        "evidence": [{ "repo_sha": "7e866311", "kind": "payload-kind" }]
    });

    // Declared in the payload schema, so it validates and matches there.
    validate(
        &filter(&["evidence.repo_sha=7e866311"], false),
        &[definition()],
    )
    .expect("a payload field wins a bare name");
    assert!(matches(
        &item,
        &filter(&["evidence.repo_sha=7e866311"], false)
    ));
    validate(
        &filter(&["payload.evidence.repo_sha=7e866311"], false),
        &[definition()],
    )
    .expect("the explicit payload half agrees");
    assert!(matches(
        &item,
        &filter(&["payload.evidence.repo_sha=7e866311"], false)
    ));

    // The envelope half is reached only by naming it, and it has its own names.
    validate(
        &filter(&["record.evidence.sha256=abc"], false),
        &[definition()],
    )
    .expect("sha256 is an envelope evidence name");
    assert!(matches(
        &item,
        &filter(&["record.evidence.kind=commit"], false)
    ));
    assert!(!matches(
        &item,
        &filter(&["record.evidence.kind=payload-kind"], false)
    ));
    // Neither half declares it, so nothing can answer it.
    validate(
        &filter(&["record.evidence.repo_sha=x"], false),
        &[definition()],
    )
    .expect_err("repo_sha is not an envelope name");
    validate(&filter(&["evidence.nowhere=x"], false), &[definition()])
        .expect_err("declared by neither half");
}

/// Advising a caller to narrow is useless if narrowing changes nothing. The
/// pool is counted inside the namespace and type they asked for, so a store
/// too large as a whole is still searchable one type at a time.
#[test]
fn the_candidate_pool_is_counted_inside_the_requested_scope() {
    let root = scoped_store();

    let whole = scope_size(&root, None, None).expect("whole store");
    let by_type = scope_size(&root, None, Some("agent.lesson.v1")).expect("one type");
    let by_namespace = scope_size(&root, Some("agent.memory"), None).expect("one namespace");
    let absent = scope_size(&root, Some("no.such.namespace"), None).expect("empty scope");

    assert_eq!(whole, 3);
    assert_eq!(by_type, 2);
    assert_eq!(by_namespace, 3);
    assert_eq!(absent, 0);
    // A scope that fits is scanned whole; one that does not says so with both
    // numbers rather than silently returning less.
    assert_eq!(candidate_limit(by_type, 20).expect("fits"), 20);
    assert_eq!(candidate_limit(137, 20).expect("fits"), 137);
    let refused = candidate_limit(crate::projection::MAX_SCAN as usize + 1, 20)
        .expect_err("past the scan bound")
        .to_string();
    assert!(
        refused.contains("narrow it with --type or --namespace"),
        "{refused}"
    );
    std::fs::remove_dir_all(root).expect("cleanup");
}
