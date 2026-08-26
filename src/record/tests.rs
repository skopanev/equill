use super::{RecordDraft, append};
use crate::command::init;
use crate::schema::{self, TypeDefinition};
use serde_json::json;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(1);

fn store() -> PathBuf {
    let suffix = NEXT.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!("equill-record-{}-{suffix}", std::process::id()));
    let _ = fs::remove_dir_all(&path);
    init::create(&path, "writer", "agent.memory").expect("initialize");
    schema::register(
        &path,
        TypeDefinition {
            type_name: "agent.lesson.v1".into(),
            uri: "equill://agent.lesson/v1".into(),
            owner: "schema-owner".into(),
            payload_schema: json!({
                "$schema": "https://json-schema.org/draft/2020-12/schema",
                "type": "object",
                "properties": { "rule": { "type": "string" } },
                "required": ["rule"],
                "additionalProperties": false
            }),
        },
        "writer",
    )
    .expect("register schema");
    path
}

fn draft(payload: serde_json::Value) -> RecordDraft {
    RecordDraft {
        namespace: "agent.memory".into(),
        type_name: "agent.lesson.v1".into(),
        observed_at: "2026-01-01T12:00:00Z".into(),
        valid_at: None,
        payload,
        evidence: Vec::new(),
        tags: vec!["testing".into()],
        supersedes: None,
    }
}

#[test]
fn appends_valid_record_without_payload_in_receipt() {
    let path = store();
    let report =
        append(&path, draft(json!({ "rule": "Run checks." })), "writer").expect("append record");
    let contents = fs::read_to_string(path.join(&report.ledger)).expect("read ledger");
    let scan = crate::integrity::scan(&path).expect("full integrity scan");

    assert_eq!(contents.lines().count(), 1);
    assert_eq!(scan.records, 1);
    assert!(
        !serde_json::to_string(&report)
            .expect("report")
            .contains("Run checks")
    );
    fs::remove_dir_all(path).expect("remove test store");
}

#[test]
fn rejects_invalid_payload_and_actor() {
    let path = store();
    let payload =
        append(&path, draft(json!({ "rule": 42 })), "writer").expect_err("reject payload");
    let actor = append(&path, draft(json!({ "rule": "safe" })), "guest").expect_err("reject actor");

    assert!(payload.to_string().contains("does not match"));
    assert!(actor.to_string().contains("not allowed"));
    fs::remove_dir_all(path).expect("remove test store");
}
