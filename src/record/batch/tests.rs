use super::{append_batch, is_batch};
use crate::command::init;
use crate::schema::{self, TypeDefinition};
use serde_json::json;
use std::fs;
use std::path::PathBuf;
use uuid::Uuid;

fn store() -> PathBuf {
    let root = std::env::temp_dir().join(format!("equill-batch-{}", Uuid::now_v7()));
    init::create(&root, "owner", "agent.memory").expect("initialize");
    schema::register(
        &root,
        TypeDefinition {
            type_name: "agent.lesson.v1".into(),
            uri: "equill://agent.lesson/v1".into(),
            owner: "owner".into(),
            payload_schema: json!({
                "$schema": "https://json-schema.org/draft/2020-12/schema",
                "type": "object",
                "properties": { "rule": { "type": "string" } },
                "required": ["rule"],
                "additionalProperties": false
            }),
            lifecycle: Default::default(),
        },
        "owner",
    )
    .expect("register schema");
    root
}

fn line(rule: &str) -> String {
    json!({
        "namespace": "agent.memory",
        "type": "agent.lesson.v1",
        "observed_at": "2026-01-01T00:00:00Z",
        "payload": { "rule": rule }
    })
    .to_string()
        + "\n"
}

/// Partial success is the useful answer: one malformed line should not cost the
/// other thirty-nine, and the caller needs to know which line to fix.
#[test]
fn a_rejected_line_stops_only_itself_and_names_its_reason() {
    let root = store();
    let source = root.join("batch.jsonl");
    let bad = json!({
        "namespace": "agent.memory",
        "type": "agent.lesson.v1",
        "observed_at": "2026-01-01T00:00:00Z",
        "payload": { "rule": 42 }
    })
    .to_string();
    fs::write(&source, line("First.") + &bad + "\n" + &line("Third.")).expect("input");

    let report = append_batch(&root, &source, "owner").expect("batch");

    assert!(!report.ok);
    assert_eq!(report.stored, 2);
    assert_eq!(report.rejected, 1);
    assert_eq!(report.records[1].line, 2);
    assert!(report.records[1].id.is_none());
    let reason = report.records[1].error.as_deref().expect("reason");
    assert!(reason.contains("does not match"), "{reason}");
    assert_eq!(crate::record::read_all(&root).expect("records").len(), 2);
    fs::remove_dir_all(root).expect("cleanup");
}

/// A single-record file keeps its old shape, so existing callers see no change.
#[test]
fn one_record_is_not_a_batch() {
    let root = store();
    let single = root.join("one.jsonl");
    let many = root.join("many.jsonl");
    fs::write(&single, line("Only.")).expect("single");
    fs::write(&many, line("First.") + &line("Second.")).expect("many");

    assert!(!is_batch(&single).expect("single"));
    assert!(is_batch(&many).expect("many"));
    fs::remove_dir_all(root).expect("cleanup");
}
