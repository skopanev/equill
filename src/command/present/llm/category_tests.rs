//! Which section a rule lands in, and when two records are the same record.
use super::answer;
use super::coverage_tests::record;
use serde_json::json;

/// A free-form field was acting as a whitelist for the renderer.
///
/// `module` is asked first and still wins when both name a category — changing
/// that is a different decision. What changes is `module` holding something
/// this formatter does not know: it used to stop there and answer "no
/// category", and the record fell out of the answer entirely even though
/// `rules` named one.
#[test]
fn an_unknown_module_does_not_veto_a_recognized_rules_field() {
    let out = answer(&[record(
        "agent.rule.v1",
        json!({ "module": "process", "rules": "comm", "rule": "sample-rule-text" }),
    )]);

    assert!(
        out.contains("COMMUNICATION RULES"),
        "the rule missed its section: {out}"
    );
    assert!(out.contains("sample-rule-text"), "{out}");
}

/// The same, when `module` is not a string at all: a shape the schema permits
/// and the formatter must not trip over.
#[test]
fn a_non_string_module_does_not_veto_a_recognized_rules_field() {
    let out = answer(&[record(
        "agent.rule.v1",
        json!({ "module": ["process"], "rules": "tickets", "rule": "sample-ticket-text" }),
    )]);

    assert!(out.contains("TICKETING RULES"), "{out}");
    assert!(out.contains("sample-ticket-text"), "{out}");
}

/// When both fields name a category the older precedence stands: `module`
/// decides. Asserted so that a later change to it is a decision somebody makes
/// rather than a side effect.
#[test]
fn module_still_wins_when_both_fields_name_a_category() {
    let out = answer(&[record(
        "agent.rule.v1",
        json!({ "module": "tickets", "rules": "comm", "rule": "sample-both-text" }),
    )]);

    let ticketing = out.find("TICKETING RULES").expect("ticketing section");
    let text = out.find("sample-both-text").expect("rule text");
    assert!(
        text > ticketing,
        "the rule left the section module chose: {out}"
    );
}

/// Two records that read the same but say different things stay two.
///
/// Collapsing on rendered text answers one question where two were asked: a
/// rule that holds for one project and a rule that holds for another are not
/// one rule, however identically they are worded.
#[test]
fn records_with_the_same_text_but_different_scope_stay_distinct() {
    let out = answer(&[
        record(
            "agent.lesson.v1",
            json!({ "rule": "Measure before claiming.", "project": "sample-alpha" }),
        ),
        record(
            "agent.lesson.v1",
            json!({ "rule": "Measure before claiming.", "project": "sample-beta" }),
        ),
    ]);

    // Counting the sentence is not the check: two identical bullets would pass
    // it while telling the reader nothing about which project each holds for.
    // What has to survive is the field that makes them different.
    assert!(
        out.contains("sample-alpha") && out.contains("sample-beta"),
        "the scopes that make these two different are not visible:\n{out}"
    );
    assert!(
        !out.contains("- Measure before claiming.\n- Measure before claiming."),
        "the answer repeats one sentence instead of distinguishing two records:\n{out}"
    );
}

/// The sections that hold bare sentences follow the same rule: a sentence
/// already printed is not an answer for a different record.
#[test]
fn processes_sharing_a_purpose_stay_distinguishable() {
    let out = answer(&[
        record(
            "agent.process.v2",
            json!({ "purpose": "Keep main green", "project": "sample-alpha" }),
        ),
        record(
            "agent.process.v2",
            json!({ "purpose": "Keep main green", "project": "sample-beta" }),
        ),
    ]);

    assert!(
        out.contains("sample-alpha") && out.contains("sample-beta"),
        "one process was absorbed by the other's sentence:\n{out}"
    );
}

/// A record recorded twice unchanged is one fact, and saying it twice tells the
/// reader nothing. Identity is namespace, type and payload — not the envelope,
/// which differs on every append.
#[test]
fn an_identical_record_recorded_twice_is_said_once() {
    let out = answer(&[
        record(
            "agent.lesson.v1",
            json!({ "rule": "Measure before claiming." }),
        ),
        record(
            "agent.lesson.v1",
            json!({ "rule": "Measure before claiming." }),
        ),
    ]);

    assert_eq!(out.matches("Measure before claiming.").count(), 1, "{out}");
}
