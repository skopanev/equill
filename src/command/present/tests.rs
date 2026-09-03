use super::{Format, records};
use crate::record::StoredRecord;
use serde_json::json;
use uuid::Uuid;

fn record(rule: &str, source: &str) -> StoredRecord {
    StoredRecord {
        id: Uuid::now_v7(),
        namespace: "agent.memory".into(),
        type_name: "agent.lesson.v1".into(),
        actor: "owner".into(),
        recorded_at: "2026-01-01T00:00:00Z".into(),
        observed_at: "2026-01-01T00:00:00Z".into(),
        valid_at: "2026-01-01T00:00:00Z".into(),
        payload: json!({ "rule": rule, "source": source, "project": ["alpha", "beta"] }),
        evidence: Vec::new(),
        tags: vec!["must".into()],
        supersedes: None,
    }
}

#[test]
fn selected_fields_print_in_the_order_they_were_asked_for() {
    let fields = vec!["rule".to_string(), "source".to_string()];
    let rows =
        records(&[record("Run the checks.", "owner")], Format::Text, &fields).expect("text output");

    assert_eq!(rows, "Rule: Run the checks.\nSource: owner");
}

#[test]
fn jsonl_keeps_one_object_per_line_and_honours_the_selection() {
    let all = records(
        &[record("First.", "owner"), record("Second.", "agent")],
        Format::Jsonl,
        &[],
    )
    .expect("jsonl");
    let narrowed = records(
        &[record("First.", "owner")],
        Format::Jsonl,
        &["rule".to_string()],
    )
    .expect("jsonl subset");

    assert_eq!(all.lines().count(), 2);
    assert!(all.contains("\"rule\":\"First.\""));
    assert_eq!(narrowed, "{\"rule\":\"First.\"}");
}

/// Envelope names are addressable beside payload fields, and a list collapses
/// to something a person can read on one line.
#[test]
fn envelope_names_and_lists_are_printable() {
    let fields = vec![
        "type".to_string(),
        "project".to_string(),
        "tags".to_string(),
    ];
    let row = records(&[record("Run.", "owner")], Format::Text, &fields).expect("text");

    assert_eq!(
        row,
        "Type: agent.lesson.v1\nProject: alpha, beta\nTags: must"
    );
}

#[test]
fn a_field_the_record_does_not_carry_prints_as_empty_rather_than_failing() {
    let fields = vec!["rule".to_string(), "absent".to_string()];
    let row = records(&[record("Run.", "owner")], Format::Text, &fields).expect("text");
    let object = records(&[record("Run.", "owner")], Format::Jsonl, &fields).expect("jsonl");

    assert_eq!(row, "Rule: Run.\nAbsent:");
    assert_eq!(object, "{\"rule\":\"Run.\"}");
}

pub(super) fn typed(type_name: &str, payload: serde_json::Value) -> StoredRecord {
    StoredRecord {
        id: Uuid::now_v7(),
        namespace: "agent.playbook".into(),
        type_name: type_name.into(),
        actor: "owner".into(),
        recorded_at: "2026-01-01T00:00:00Z".into(),
        observed_at: "2026-01-01T00:00:00Z".into(),
        valid_at: "2026-01-01T00:00:00Z".into(),
        payload,
        evidence: Vec::new(),
        tags: Vec::new(),
        supersedes: None,
    }
}

/// The four things the old line did wrong, asserted as four absences on one
/// output: no identifier a reader cannot use, no columns, no quoted blob, and
/// no field printed without saying what it is.
#[test]
fn the_text_answer_carries_no_uuid_no_tabs_and_no_escaped_json() {
    let printed = records(
        &[typed(
            "agent.role.v1",
            json!({
                "role": "Release reviewer",
                "process": "Pre-merge audit",
                "steps": [{ "do": "Reply \"go\" when the gate is green" }]
            }),
        )],
        Format::Text,
        &[],
    )
    .expect("text");

    assert!(!printed.contains('\t'), "columns survived: {printed}");
    assert!(
        !printed.contains("{\"") && !printed.contains("\\\""),
        "a nested value arrived as escaped json: {printed}"
    );
    assert!(
        printed.contains("Reply \"go\" when the gate is green"),
        "the quote inside the step did not reach the reader: {printed}"
    );
    assert!(printed.contains("Steps:"), "the steps were lost: {printed}");
    assert!(
        !printed
            .split_whitespace()
            .any(|word| word.len() == 36 && word.matches('-').count() == 4),
        "an identifier is still being printed: {printed}"
    );
}
