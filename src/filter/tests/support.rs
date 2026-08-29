use super::super::{Filter, matches, validate};
use crate::record::{EvidenceRef, StoredRecord};
use crate::schema::TypeDefinition;
use serde_json::json;
use uuid::Uuid;

pub fn filter(flags: &[&str], strict: bool) -> Filter {
    Filter::parse(
        &flags
            .iter()
            .map(|flag| (*flag).to_string())
            .collect::<Vec<_>>(),
        strict,
    )
    .expect("filter parses")
}

pub fn record() -> StoredRecord {
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
        "evidence": [{ "kind": "commit", "digest": "abc", "at": { "repo": { "sha": "deadbeef" } } }],
        "weight": 3,
        "active": true
        }),
    }
}

pub fn definition() -> TypeDefinition {
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
                            "digest": { "type": "string" },
                            "repo_sha": { "type": "string" },
                            "at": { "type": "object" }
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

/// supersedes is part of the envelope a caller may ask about — it is how a
/// retraction is found — so it must both validate and match. It was missing
/// from the list, which made `--where supersedes=null` pass in code and fail
/// at the command line: two answers to one question.
#[test]
fn supersedes_is_filterable_like_the_rest_of_the_envelope() {
    let mut replaced = record();
    let target = Uuid::now_v7();
    replaced.supersedes = Some(target);
    let original = record();

    validate(&filter(&["supersedes=null"], false), &[definition()])
        .expect("supersedes is a known envelope name");
    assert!(matches(&original, &filter(&["supersedes=null"], false)));
    assert!(!matches(&original, &filter(&["supersedes=!null"], false)));
    assert!(matches(&replaced, &filter(&["supersedes=!null"], false)));
    assert!(matches(
        &replaced,
        &filter(&[&format!("supersedes={target}")], false)
    ));
    assert!(!matches(
        &replaced,
        &filter(&[&format!("supersedes={}", Uuid::now_v7())], false)
    ));
}

/// A path that meets a list has to keep walking the rest of itself. Passing
/// only the current segment answered a different, shorter question and quietly
/// returned the wrong records for anything nested below a list.
#[test]
fn a_dotted_path_survives_a_list_in_the_middle() {
    let item = record();

    assert!(matches(
        &item,
        &filter(&["evidence.at.repo.sha=deadbeef"], false)
    ));
    assert!(!matches(
        &item,
        &filter(&["evidence.at.repo.sha=other"], false)
    ));
    // The shallower path still works, and a name that exists nowhere does not.
    assert!(matches(&item, &filter(&["evidence.kind=commit"], false)));
    assert!(!matches(
        &item,
        &filter(&["evidence.at.repo.branch=main"], true)
    ));
}

/// `role=backend,null` is one question — "backend, or nothing said about role"
/// — and answering it must not need two searches. Negating that mixture is the
/// ambiguous case and is refused by name rather than guessed at.
/// A bare name means the payload, and the explicit halves reach either side.
/// Printing and filtering resolve the same way, or one word means two things.
#[test]
fn explicit_halves_address_payload_and_envelope_apart() {
    let mut item = record();
    item.actor = "envelope-actor".into();
    item.payload = json!({ "actor": "payload-actor", "rule": "Run" });

    assert!(matches(&item, &filter(&["actor=payload-actor"], false)));
    assert!(matches(
        &item,
        &filter(&["payload.actor=payload-actor"], false)
    ));
    assert!(matches(
        &item,
        &filter(&["record.actor=envelope-actor"], false)
    ));
    assert!(!matches(
        &item,
        &filter(&["record.actor=payload-actor"], false)
    ));
    validate(
        &filter(&["record.actor=x", "payload.rule=y"], false),
        &[definition()],
    )
    .expect("both halves are addressable");
    let unknown = validate(&filter(&["record.nonsense=x"], false), &[definition()])
        .expect_err("a prefixed path that names nothing must fail")
        .to_string();
    assert!(unknown.contains("record.nonsense"), "{unknown}");
    validate(&filter(&["payload.nonsense=x"], false), &[definition()])
        .expect_err("an undeclared payload field must fail");
}
