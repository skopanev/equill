use super::super::{candidate_limit, matches, validate};
use super::support::*;
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
