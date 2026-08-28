use super::{RecordDraft, append};
use crate::command::init;
use crate::projection::{self, ProjectionState, SearchRequest};
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
    let search = projection::search(
        &path,
        &SearchRequest {
            query: "Run checks".into(),
            namespace: Some("agent.memory".into()),
            type_name: Some("agent.lesson.v1".into()),
            limit: 10,
        },
    )
    .expect("search projection");

    assert_eq!(contents.lines().count(), 1);
    assert_eq!(scan.records, 1);
    assert_eq!(report.projection, ProjectionState::Ready);
    assert_eq!(search.hits.len(), 1);
    assert!(
        !serde_json::to_string(&report)
            .expect("report")
            .contains("Run checks")
    );
    let rebuilt = projection::rebuild(&path).expect("rebuild projection");
    assert_eq!(rebuilt.records, 1);
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

#[test]
fn invalid_payload_names_the_field_and_the_constraint() {
    // The author of a lesson appends JSONL by hand and cannot see the registered
    // contract. "does not match" alone sends them hunting through the schema.
    let suffix = NEXT.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "equill-record-detail-{}-{suffix}",
        std::process::id()
    ));
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
                "properties": {
                    "rule": { "type": "string", "maxLength": 500 },
                    "source": { "type": "string", "enum": ["gate", "panel", "owner"] }
                },
                "required": ["rule", "source"],
                "additionalProperties": false
            }),
        },
        "writer",
    )
    .expect("register schema");

    let long_rule = "x".repeat(1070);
    let error = append(
        &path,
        draft(json!({ "rule": long_rule, "source": "gm" })),
        "writer",
    )
    .expect_err("reject payload");
    let message = error.to_string();

    assert!(message.contains("/rule"), "{message}");
    assert!(message.contains("longer than 500"), "{message}");
    assert!(message.contains("/source"), "{message}");
    assert!(message.contains("\"gm\""), "{message}");
    // The offending value must not bury the reason that follows it.
    assert!(message.len() < 600, "message is {} bytes", message.len());

    let missing = append(
        &path,
        draft(json!({ "rule": "long enough rule" })),
        "writer",
    )
    .expect_err("reject missing field");
    assert!(
        missing
            .to_string()
            .contains("\"source\" is a required property"),
        "{missing}"
    );

    fs::remove_dir_all(path).expect("remove test store");
}
