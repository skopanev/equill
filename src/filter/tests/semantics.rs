use super::super::{Filter, matches};
use super::support::*;
use serde_json::json;
#[test]
fn repeated_flags_are_and_while_commas_inside_one_flag_are_or() {
    let both = filter(&["source=owner", "severity=must"], false);
    let either = filter(&["severity=must,should"], false);
    let one_wrong = filter(&["source=owner", "severity=should"], false);

    assert!(matches(&record(), &both));
    assert!(matches(&record(), &either));
    assert!(!matches(&record(), &one_wrong));
}

#[test]
fn scalars_compare_by_value_and_arrays_by_membership() {
    assert!(matches(&record(), &filter(&["project=alpha"], false)));
    assert!(matches(&record(), &filter(&["project=beta"], false)));
    assert!(!matches(&record(), &filter(&["project=gamma"], false)));
    // Numbers and booleans are written the way a caller would type them.
    assert!(matches(&record(), &filter(&["weight=3"], false)));
    assert!(!matches(&record(), &filter(&["weight=4"], false)));
    assert!(matches(&record(), &filter(&["active=true"], false)));
}

#[test]
fn null_and_not_null_ask_about_presence_directly() {
    assert!(matches(&record(), &filter(&["revoked_at=null"], false)));
    assert!(!matches(&record(), &filter(&["revoked_at=!null"], false)));
    // A field the record never carries is absent, which reads the same as null.
    assert!(matches(&record(), &filter(&["expires_at=null"], false)));
    assert!(!matches(&record(), &filter(&["expires_at=!null"], false)));
    // Presence questions ignore the absence policy: they are the question.
    assert!(matches(&record(), &filter(&["revoked_at=null"], true)));
}

#[test]
fn negation_rejects_every_listed_value() {
    assert!(matches(&record(), &filter(&["severity=!should"], false)));
    assert!(!matches(&record(), &filter(&["severity=!must"], false)));
    assert!(!matches(
        &record(),
        &filter(&["severity=!must,should"], false)
    ));
    assert!(matches(&record(), &filter(&["project=!gamma"], false)));
}

#[test]
fn absent_fields_match_by_default_and_are_dropped_under_strict() {
    let lenient = filter(&["expires_at=2030-01-01"], false);
    let strict = filter(&["expires_at=2030-01-01"], true);
    let explicit_null = filter(&["revoked_at=2030-01-01"], true);

    assert!(matches(&record(), &lenient));
    assert!(!matches(&record(), &strict));
    assert!(!matches(&record(), &explicit_null));
}

#[test]
fn malformed_flags_are_refused_at_parse_time() {
    for flag in [
        "source",
        "=owner",
        "source=",
        "revoked_at=!null,owner",
        ".source=owner",
    ] {
        Filter::parse(&[flag.to_string()], false).expect_err(&format!("{flag} must be refused"));
    }
    Filter::parse(&[], false).expect("no filters is a valid filter");
}

#[test]
fn null_can_be_one_alternative_among_others() {
    let mut scalar = record();
    scalar.payload = json!({ "role": "backend" });
    let mut listed = record();
    listed.payload = json!({ "role": ["backend", "frontend"] });
    let mut other = record();
    other.payload = json!({ "role": "design" });
    let mut explicit_null = record();
    explicit_null.payload = json!({ "role": null });
    let mut absent = record();
    absent.payload = json!({ "rule": "no role at all" });

    let asked = filter(&["role=backend,null"], false);
    for matching in [&scalar, &listed, &explicit_null, &absent] {
        assert!(matches(matching, &asked), "{:?}", matching.payload);
    }
    assert!(!matches(&other, &asked));
    // Under --strict the presence half keeps its meaning: an absent field is
    // still the null the caller explicitly asked for.
    let strict = filter(&["role=backend,null"], true);
    assert!(matches(&absent, &strict) && matches(&explicit_null, &strict));
    assert!(!matches(&other, &strict));
    Filter::parse(&["role=!backend,null".to_string()], false)
        .expect_err("negating a list with null is ambiguous");
}
