use super::{Filter, candidate_limit, matches, validate};
use crate::record::{EvidenceRef, StoredRecord};
use crate::schema::TypeDefinition;
use serde_json::json;
use uuid::Uuid;

fn filter(flags: &[&str], strict: bool) -> Filter {
    Filter::parse(
        &flags
            .iter()
            .map(|flag| (*flag).to_string())
            .collect::<Vec<_>>(),
        strict,
    )
    .expect("filter parses")
}

fn record() -> StoredRecord {
    StoredRecord {
        id: Uuid::now_v7(),
        namespace: "agent.memory".into(),
        type_name: "agent.lesson.v1".into(),
        actor: "owner".into(),
        recorded_at: "2026-01-01T00:00:00Z".into(),
        observed_at: "2026-01-01T00:00:00Z".into(),
        valid_at: "2026-01-02T00:00:00Z".into(),
        evidence: vec![EvidenceRef {
            kind: "commit".into(),
            reference: "synthetic-ref".into(),
            sha256: None,
        }],
        tags: vec!["must".into(), "core".into()],
        supersedes: None,
        payload: json!({
        "rule": "Run the checks.",
        "source": "owner",
        "severity": "must",
        "project": ["alpha", "beta"],
        "revoked_at": null,
        "evidence": [{ "kind": "commit", "digest": "abc" }],
        "weight": 3,
        "active": true
        }),
    }
}

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

/// A selector reads an absent coordinate as "applies to everything". The filter
/// agrees by default so the two do not disagree invisibly, and `--strict` is
/// how a caller asks for the other reading.
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
fn malformed_flags_are_refused_at_parse_time() {
    for flag in [
        "source",
        "=owner",
        "source=",
        "revoked_at=null,owner",
        ".source=owner",
    ] {
        Filter::parse(&[flag.to_string()], false).expect_err(&format!("{flag} must be refused"));
    }
    Filter::parse(&[], false).expect("no filters is a valid filter");
}

#[test]
fn filtered_search_requests_the_whole_corpus_or_refuses_explicitly() {
    assert_eq!(candidate_limit(137, 20).expect("whole corpus"), 137);
    assert_eq!(candidate_limit(3, 20).expect("requested page"), 20);
    assert!(candidate_limit(usize::from(u16::MAX) + 1, 20).is_err());
}

fn definition() -> TypeDefinition {
    TypeDefinition {
        type_name: "agent.lesson.v1".into(),
        uri: "equill://agent.lesson/v1".into(),
        owner: "owner".into(),
        payload_schema: json!({
            "type": "object",
            "properties": {
                "rule": { "type": "string" },
                "source": { "type": "string" },
                "severity": { "type": "string" },
                "project": { "type": "array", "items": { "type": "string" } },
                "revoked_at": { "type": ["string", "null"] },
                "weight": { "type": "number" },
                "active": { "type": "boolean" },
                "evidence": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "kind": { "type": "string" },
                            "digest": { "type": "string" }
                        }
                    }
                }
            }
        }),
        lifecycle: Default::default(),
    }
}

/// The envelope carries what every record has whatever its type. A caller
/// asking about tags or the actor is asking an ordinary question, and it would
/// be strange for the same name to be printable by --fields but unfilterable.
#[test]
fn envelope_names_are_filterable_beside_payload_fields() {
    let item = record();
    let id = item.id.to_string();

    for flag in [
        "namespace=agent.memory",
        "type=agent.lesson.v1",
        "actor=owner",
        "valid_at=2026-01-02T00:00:00Z",
        "tags=must",
        "tags=core",
        "evidence.kind=commit",
        "evidence.reference=synthetic-ref",
        "supersedes=null",
    ] {
        assert!(matches(&item, &filter(&[flag], false)), "{flag} must match");
    }
    assert!(matches(&item, &filter(&[&format!("id={id}")], false)));
    assert!(!matches(&item, &filter(&["actor=someone-else"], false)));
    assert!(!matches(&item, &filter(&["tags=absent-tag"], true)));
    assert!(matches(&item, &filter(&["type=!agent.lesson.v2"], false)));
    // Envelope names are always legal: no schema declares or omits them.
    validate(
        &filter(&["tags=must", "evidence.kind=commit"], false),
        &[definition()],
    )
    .expect("envelope names need no declaration");
}

/// A payload field of the same name still wins, because that is the name the
/// caller was reading in the schema when they typed it.
#[test]
fn a_payload_field_shadows_an_envelope_name() {
    let mut item = record();
    item.payload = json!({ "actor": "payload-actor" });

    assert!(matches(&item, &filter(&["actor=payload-actor"], false)));
    assert!(!matches(&item, &filter(&["actor=owner"], false)));
}
