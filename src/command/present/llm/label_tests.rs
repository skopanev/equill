//! Which blocks get a namespace and type, and which stay compact.
use super::answer;
use super::coverage_tests::record;
use serde_json::json;

/// A record that carries its own namespace, for the cases where what tells two
/// records apart is what they are rather than what they say.
fn elsewhere(namespace: &str, payload: serde_json::Value) -> crate::record::StoredRecord {
    let mut item = record("agent.lesson.v1", payload);
    item.namespace = namespace.to_owned();
    item
}

/// A group where the payload separates some members and not others.
///
/// Two records differ only in namespace and a third differs in payload. The
/// payload fields settle the third, and the first two are still identical —
/// they get the label, and only they do. Labelling the whole group would bury
/// the compact answer under provenance nobody asked for; labelling none leaves
/// two blocks a reader cannot tell apart.
#[test]
fn only_the_blocks_that_still_read_alike_are_labelled() {
    let out = answer(&[
        elsewhere(
            "ns.one",
            json!({ "rule": "Measure first.", "project": "alpha" }),
        ),
        elsewhere(
            "ns.two",
            json!({ "rule": "Measure first.", "project": "alpha" }),
        ),
        elsewhere(
            "ns.one",
            json!({ "rule": "Measure first.", "project": "beta" }),
        ),
    ]);

    assert!(
        out.contains("type: ns.one/agent.lesson.v1")
            && out.contains("type: ns.two/agent.lesson.v1"),
        "the two records that differ only in namespace are still identical:\n{out}"
    );
    assert_eq!(
        out.matches("type: ").count(),
        2,
        "the record the payload already distinguished was labelled too:\n{out}"
    );
    assert!(
        out.contains("project: beta"),
        "the third record lost its distinguishing field:\n{out}"
    );
}

/// One record keeps the compact shape: there is nothing to tell it apart from.
#[test]
fn a_lone_record_is_not_labelled_with_its_namespace() {
    let out = answer(&[elsewhere(
        "ns.one",
        json!({ "rule": "Measure first.", "project": "alpha" }),
    )]);

    assert!(
        !out.contains("type: "),
        "a lone record was labelled:\n{out}"
    );
}

/// The same fact recorded twice is still said once — labelling must not turn
/// exact duplicates into two blocks.
#[test]
fn exact_duplicates_still_coalesce_after_labelling() {
    let payload = json!({ "rule": "Measure first.", "project": "alpha" });
    let out = answer(&[
        elsewhere("ns.one", payload.clone()),
        elsewhere("ns.one", payload),
    ]);

    assert_eq!(out.matches("Measure first.").count(), 1, "{out}");
    assert!(!out.contains("type: "), "{out}");
}

/// A rule whose category fields are present but null names no category, and the
/// record still reaches the answer through the generic fallback.
#[test]
fn a_rule_with_null_category_fields_is_not_lost() {
    let out = answer(&[record(
        "agent.rule.v1",
        json!({ "module": null, "rules": null, "rule": "sample-null-category" }),
    )]);

    assert!(out.contains("sample-null-category"), "{out}");
}

/// The type the ticket was opened for: no renderer knows it, and it still
/// reaches the reader.
#[test]
fn a_project_record_reaches_the_answer_through_the_fallback() {
    let out = answer(&[record(
        "agent.project.v1",
        json!({ "project": "sample-project", "lane_limit": 4 }),
    )]);

    assert!(out.contains("sample-project"), "{out}");
    assert!(out.contains("lane_limit: 4"), "{out}");
}
