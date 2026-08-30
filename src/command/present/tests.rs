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

    assert_eq!(rows, "Run the checks.\towner");
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

    assert_eq!(row, "agent.lesson.v1\talpha,beta\tmust");
}

#[test]
fn a_field_the_record_does_not_carry_prints_as_empty_rather_than_failing() {
    let fields = vec!["rule".to_string(), "absent".to_string()];
    let row = records(&[record("Run.", "owner")], Format::Text, &fields).expect("text");
    let object = records(&[record("Run.", "owner")], Format::Jsonl, &fields).expect("jsonl");

    assert_eq!(row, "Run.\t");
    assert_eq!(object, "{\"rule\":\"Run.\"}");
}
